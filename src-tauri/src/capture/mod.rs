//! Screen capture: grab every display at native resolution, then crop a selection.
//!
//! The OS calls (via the `xcap` crate) are the few functions at the bottom of this file.
//! Geometry, cropping, error handling and encoding are pure and unit-tested.
//! See `docs/spikes/capture.md` for the choice of `xcap` and measured latency.

mod geometry;

pub use geometry::{Bounds, PixelRect, Rect, crop, crop_pixels};

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

/// A connected display, before capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Display {
    pub id: u32,
    /// Points on macOS, pixels on Windows (see [`DisplayFrame`]).
    pub bounds: Bounds,
}

/// One display, frozen at the moment of capture.
///
/// `logical` is in points (macOS) or DIPs (Windows); `physical` is in pixels. On macOS the
/// logical bounds are authoritative and physical is derived from them, as Tauri does. On
/// Windows the physical bounds are authoritative: with mixed DPI, logical origins of different
/// monitors cannot be placed exactly without the full virtual-desktop layout, so overlay windows
/// should be positioned with Tauri's `PhysicalPosition`.
///
/// `image` is raw RGBA at native resolution (`physical.width` x `physical.height`). The overlay
/// gets these bytes as-is; PNG encoding is only for the final crop.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayFrame {
    pub id: u32,
    pub logical: Bounds,
    pub physical: Bounds,
    /// As reported by the OS. Informational only: it can disagree with the image (rotated
    /// displays, a failed display-mode query), so cropping measures the scale from the image.
    pub scale_factor: f64,
    pub image: RgbaImage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// Screen Recording is not allowed (macOS). Callers check [`has_permission`] before
    /// [`capture_all`] and report this; [`request_permission`] shows the system prompt.
    PermissionDenied,
    /// The OS returned an empty image for every display.
    EmptyImage,
    NoDisplays,
    /// The selection does not cover any pixel of the display.
    EmptySelection,
    Backend(String),
    Encode(String),
}

impl CaptureError {
    fn kind(&self) -> &'static str {
        match self {
            Self::PermissionDenied => "permissionDenied",
            Self::EmptyImage => "emptyImage",
            Self::NoDisplays => "noDisplays",
            Self::EmptySelection => "emptySelection",
            Self::Backend(_) => "backend",
            Self::Encode(_) => "encode",
        }
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied => f.write_str("Screen Recording permission is not granted"),
            Self::EmptyImage => f.write_str("the screen capture came back empty"),
            Self::NoDisplays => f.write_str("no displays found"),
            Self::EmptySelection => f.write_str("the selection is empty"),
            Self::Backend(e) => write!(f, "screen capture failed: {e}"),
            Self::Encode(e) => write!(f, "PNG encoding failed: {e}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Serializes as `{ "kind": "permissionDenied", "message": "..." }` so the frontend can branch
/// on `kind`.
impl serde::Serialize for CaptureError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("CaptureError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

/// Builds a frame from the OS geometry and image. `os_bounds` are points on macOS and pixels
/// on Windows; this is the one place that difference is handled.
fn to_frame(
    id: u32,
    os_bounds: Bounds,
    scale_factor: f64,
    image: RgbaImage,
) -> Result<DisplayFrame, CaptureError> {
    if image.width() == 0 || image.height() == 0 {
        return Err(CaptureError::EmptyImage);
    }
    if os_bounds.width == 0 || os_bounds.height == 0 {
        return Err(CaptureError::Backend(format!("display {id} has no size")));
    }
    let (logical, physical) = if cfg!(target_os = "windows") {
        geometry::from_pixels(os_bounds, scale_factor, &image)
    } else {
        geometry::from_points(os_bounds, &image)
    };
    Ok(DisplayFrame {
        id,
        logical,
        physical,
        scale_factor,
        image,
    })
}

/// How long one display may take before it is skipped, so a stalled driver cannot block the
/// capture (its thread is left to finish on its own).
const DISPLAY_TIMEOUT: Duration = Duration::from_secs(1);

/// Displays whose capture thread is still running (stalled past the timeout).
static IN_FLIGHT: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Runs `capture` for every display at once, so total time is the slowest display. A display
/// that panics or takes longer than `timeout` gets an error instead. A display whose previous
/// capture is still stuck is skipped, so a hung driver costs at most one thread per display.
fn capture_each<F>(
    ids: &[u32],
    capture: F,
    timeout: Duration,
    in_flight: &'static Mutex<Vec<u32>>,
) -> Vec<Result<DisplayFrame, CaptureError>>
where
    F: Fn(u32) -> Result<DisplayFrame, CaptureError> + Send + Sync + 'static,
{
    let lock = || in_flight.lock().unwrap_or_else(PoisonError::into_inner);
    let capture = Arc::new(capture);
    let (tx, rx) = mpsc::channel();
    let mut results: Vec<_> = ids
        .iter()
        .map(|id| Err(CaptureError::Backend(format!("display {id} timed out"))))
        .collect();
    for (i, &id) in ids.iter().enumerate() {
        if lock().contains(&id) {
            results[i] = Err(CaptureError::Backend(format!(
                "display {id} is still stuck"
            )));
            continue;
        }
        lock().push(id);
        let (capture, tx) = (Arc::clone(&capture), tx.clone());
        std::thread::spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| capture(id)))
                .unwrap_or_else(|_| Err(CaptureError::Backend("capture panicked".into())));
            lock().retain(|&d| d != id);
            let _ = tx.send((i, result));
        });
    }
    drop(tx);
    let deadline = Instant::now() + timeout;
    for _ in 0..ids.len() {
        let left = deadline.saturating_duration_since(Instant::now());
        let Ok((i, result)) = rx.recv_timeout(left) else {
            break;
        };
        results[i] = result;
    }
    results
}

/// Keeps every display that was captured. A display that failed (asleep, just unplugged) or
/// mirrors another one (same logical bounds) is skipped, so it cannot block the others.
/// Fails only if nothing was captured, with the first error.
fn keep_captured(
    results: Vec<Result<DisplayFrame, CaptureError>>,
) -> Result<Vec<DisplayFrame>, CaptureError> {
    let mut frames: Vec<DisplayFrame> = Vec::with_capacity(results.len());
    let mut first_error = None;
    for result in results {
        match result {
            Ok(f) if frames.iter().any(|k| k.logical == f.logical) => {
                log::info!("skipping display {}: mirrors another display", f.id);
            }
            Ok(f) => frames.push(f),
            Err(e) => {
                log::warn!("skipping display: {e}");
                first_error.get_or_insert(e);
            }
        }
    }
    if frames.is_empty() {
        return Err(first_error.unwrap_or(CaptureError::NoDisplays));
    }
    Ok(frames)
}

/// Encodes the final crop as PNG for saving or copying.
pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, CaptureError> {
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Default, FilterType::Adaptive)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| CaptureError::Encode(e.to_string()))?;
    Ok(out)
}

// ---- OS calls only below. They need a real screen, so `examples/capture.rs` exercises them. ----

/// Captures every display at native resolution, in parallel. Does not check permission: on
/// macOS call [`has_permission`] first, since without it the OS may return wallpaper-only
/// frames that look valid.
pub fn capture_all() -> Result<Vec<DisplayFrame>, CaptureError> {
    let ids: Vec<u32> = displays()?.iter().map(|d| d.id).collect();
    capture_displays(&ids)
}

/// Like [`capture_all`], for displays already listed with [`displays`].
pub fn capture_displays(ids: &[u32]) -> Result<Vec<DisplayFrame>, CaptureError> {
    keep_captured(capture_each(
        ids,
        capture_display,
        DISPLAY_TIMEOUT,
        &IN_FLIGHT,
    ))
}

/// The displays [`capture_all`] captures (mirrors excluded), with their OS bounds: points on
/// macOS, pixels on Windows, the authoritative units for placing windows on each platform.
/// Lists displays only: no capture and no permission needed.
pub fn displays() -> Result<Vec<Display>, CaptureError> {
    let monitors = xcap::Monitor::all().map_err(backend)?;
    let mut out: Vec<Display> = Vec::with_capacity(monitors.len());
    for m in &monitors {
        let Ok(id) = m.id() else { continue };
        if is_mirror(id) || out.iter().any(|d| d.id == id) {
            continue;
        }
        // A display that cannot report its bounds (asleep, unplugged) is skipped, not fatal.
        match os_bounds(m) {
            Ok(bounds) => out.push(Display { id, bounds }),
            Err(e) => log::warn!("skipping display {id}: {e}"),
        }
    }
    Ok(out)
}

fn os_bounds(m: &xcap::Monitor) -> Result<Bounds, CaptureError> {
    Ok(Bounds {
        x: m.x().map_err(backend)?,
        y: m.y().map_err(backend)?,
        width: m.width().map_err(backend)?,
        height: m.height().map_err(backend)?,
    })
}

/// `xcap` monitors cannot cross threads on Windows, so each thread looks its display up again.
fn capture_display(id: u32) -> Result<DisplayFrame, CaptureError> {
    let monitors = xcap::Monitor::all().map_err(backend)?;
    let m = monitors
        .iter()
        .find(|m| m.id().ok() == Some(id))
        .ok_or_else(|| CaptureError::Backend(format!("display {id} disappeared")))?;
    let bounds = os_bounds(m)?;
    let scale = m.scale_factor().map_err(backend)?;
    to_frame(
        id,
        bounds,
        scale.into(),
        m.capture_image().map_err(backend)?,
    )
}

fn backend(e: xcap::XCapError) -> CaptureError {
    CaptureError::Backend(e.to_string())
}

/// A display that shows another display's content (macOS mirroring).
#[cfg(target_os = "macos")]
fn is_mirror(id: u32) -> bool {
    objc2_core_graphics::CGDisplayMirrorsDisplay(id) != 0
}

#[cfg(not(target_os = "macos"))]
fn is_mirror(_id: u32) -> bool {
    false
}

/// Whether the app may capture the screen. Never shows a prompt.
#[cfg(target_os = "macos")]
pub fn has_permission() -> bool {
    objc2_core_graphics::CGPreflightScreenCaptureAccess()
}

/// Shows the system Screen Recording prompt (only the first time; later calls return the
/// current state). The grant usually takes effect only after the app restarts.
#[cfg(target_os = "macos")]
pub fn request_permission() -> bool {
    objc2_core_graphics::CGRequestScreenCaptureAccess()
}

#[cfg(not(target_os = "macos"))]
pub fn has_permission() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn request_permission() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(width: u32, height: u32) -> Bounds {
        Bounds {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    fn ok_frame(id: u32, x: i32) -> Result<DisplayFrame, CaptureError> {
        let b = Bounds { x, ..bounds(2, 2) };
        to_frame(id, b, 1.0, RgbaImage::new(2, 2))
    }

    fn ids(frames: &[DisplayFrame]) -> Vec<u32> {
        frames.iter().map(|f| f.id).collect()
    }

    #[test]
    fn frame_uses_the_platform_geometry() {
        // Windows reports bounds in pixels, macOS in points; both describe the same display.
        let os_bounds = if cfg!(target_os = "windows") {
            bounds(3600, 2338)
        } else {
            bounds(1800, 1169)
        };
        let f = to_frame(7, os_bounds, 2.0, RgbaImage::new(3600, 2338)).unwrap();
        assert_eq!(f.id, 7);
        assert_eq!(f.scale_factor, 2.0);
        assert_eq!(f.logical, bounds(1800, 1169));
        assert_eq!(f.physical, bounds(3600, 2338));
    }

    #[test]
    fn empty_image_or_geometry_is_an_error() {
        for img in [
            RgbaImage::new(0, 0),
            RgbaImage::new(0, 10),
            RgbaImage::new(10, 0),
        ] {
            let r = to_frame(1, bounds(10, 10), 1.0, img);
            assert_eq!(r, Err(CaptureError::EmptyImage));
        }
        let r = to_frame(1, bounds(0, 10), 1.0, RgbaImage::new(10, 10));
        assert!(matches!(r, Err(CaptureError::Backend(_))));
    }

    #[test]
    fn failed_displays_do_not_block_the_others() {
        let results = vec![
            Err(CaptureError::Backend("asleep".into())),
            ok_frame(2, 0),
            Err(CaptureError::EmptyImage),
            ok_frame(4, 2),
        ];
        assert_eq!(ids(&keep_captured(results).unwrap()), [2, 4]);
    }

    #[test]
    fn mirrored_displays_are_captured_once() {
        let results = vec![ok_frame(1, 0), ok_frame(2, 0), ok_frame(3, 2)];
        assert_eq!(ids(&keep_captured(results).unwrap()), [1, 3]);
    }

    #[test]
    fn fails_only_when_nothing_was_captured() {
        assert_eq!(keep_captured(vec![]), Err(CaptureError::NoDisplays));
        let results = vec![
            Err(CaptureError::EmptyImage),
            Err(CaptureError::Backend("gone".into())),
        ];
        assert_eq!(keep_captured(results), Err(CaptureError::EmptyImage));
    }

    #[test]
    fn captures_every_display_in_parallel() {
        let barrier = std::sync::Barrier::new(3);
        // Each capture waits for the other two, so this only finishes if all three run at once.
        let results = capture_each(
            &[1, 2, 3],
            move |id| {
                barrier.wait();
                if id == 2 {
                    panic!("driver crash");
                }
                ok_frame(id, id as i32 * 2)
            },
            Duration::from_secs(10),
            fresh(),
        );
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap().id, 1);
        assert!(matches!(&results[1], Err(CaptureError::Backend(_))));
        assert_eq!(results[2].as_ref().unwrap().id, 3);
    }

    #[test]
    fn a_stalled_display_is_skipped_after_the_timeout() {
        let in_flight = fresh();
        let started = Instant::now();
        let results = capture_each(
            &[1, 2],
            |id| {
                if id == 2 {
                    std::thread::sleep(Duration::from_secs(5));
                }
                ok_frame(id, 0)
            },
            Duration::from_millis(100),
            in_flight,
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(results[0].as_ref().unwrap().id, 1);
        assert_eq!(
            results[1],
            Err(CaptureError::Backend("display 2 timed out".into()))
        );
        // While display 2 is still stuck, the next capture skips it instead of piling up threads.
        let again = capture_each(
            &[1, 2],
            |id| ok_frame(id, 0),
            Duration::from_secs(1),
            in_flight,
        );
        assert_eq!(again[0].as_ref().unwrap().id, 1);
        assert_eq!(
            again[1],
            Err(CaptureError::Backend("display 2 is still stuck".into()))
        );
    }

    fn fresh() -> &'static Mutex<Vec<u32>> {
        Box::leak(Box::default())
    }

    #[test]
    fn png_round_trips() {
        let img = RgbaImage::from_fn(20, 10, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let png = encode_png(&img).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().to_rgba8(), img);
        // The encoder rejects zero-sized images.
        let r = encode_png(&RgbaImage::new(0, 0));
        assert!(matches!(r, Err(CaptureError::Encode(_))));
    }

    #[test]
    fn errors_serialize_with_a_kind() {
        let json = |e: CaptureError| serde_json::to_value(e).unwrap();
        assert_eq!(
            json(CaptureError::PermissionDenied),
            serde_json::json!({
                "kind": "permissionDenied",
                "message": "Screen Recording permission is not granted"
            })
        );
        assert_eq!(
            json(CaptureError::Backend("x".into())),
            serde_json::json!({ "kind": "backend", "message": "screen capture failed: x" })
        );
        let kinds = [
            CaptureError::EmptyImage,
            CaptureError::NoDisplays,
            CaptureError::EmptySelection,
            CaptureError::Encode("y".into()),
        ]
        .map(|e| json(e)["kind"].as_str().unwrap().to_owned());
        assert_eq!(
            kinds,
            ["emptyImage", "noDisplays", "emptySelection", "encode"]
        );
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn other_platforms_need_no_permission() {
        assert!(has_permission());
        assert!(request_permission());
    }
}
