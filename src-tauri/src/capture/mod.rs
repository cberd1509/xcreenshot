//! Screen capture: grab every display at native resolution, then crop a selection.
//!
//! The OS calls (via the `xcap` crate) are the few functions at the bottom of this file.
//! Geometry, cropping, error handling and encoding are pure and unit-tested.
//! See `docs/spikes/capture.md` for the choice of `xcap` and measured latency.

mod geometry;

pub use geometry::{Bounds, PixelRect, Rect, clamp_rect, crop, crop_pixels};

use image::RgbaImage;
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
    /// Copying the crop to the clipboard failed.
    Clipboard(String),
    /// Writing the saved file failed.
    Write(String),
    /// The Save dialog failed (not a cancel).
    Dialog(String),
    /// The page sent a copy or save request that does not make sense.
    Invalid(String),
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
            Self::Clipboard(_) => "clipboard",
            Self::Write(_) => "write",
            Self::Dialog(_) => "dialog",
            Self::Invalid(_) => "invalid",
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
            Self::Clipboard(e) => write!(f, "could not copy to the clipboard: {e}"),
            Self::Write(e) => write!(f, "could not save the file: {e}"),
            Self::Dialog(e) => write!(f, "the Save dialog failed: {e}"),
            Self::Invalid(e) => write!(f, "invalid request: {e}"),
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

/// The same for warm-ups. Kept apart, so a warm-up still running never makes a hotkey capture
/// skip its display as stuck.
static WARMING: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Runs `capture` for every display at once, so total time is the slowest display. A display
/// that panics or takes longer than `timeout` gets an error instead. A display whose previous
/// capture is still stuck is skipped, so a hung driver costs at most one thread per display.
fn capture_each<T, F>(
    ids: &[u32],
    capture: F,
    timeout: Duration,
    in_flight: &'static Mutex<Vec<u32>>,
) -> Vec<Result<T, CaptureError>>
where
    F: Fn(u32) -> Result<T, CaptureError> + Send + Sync + 'static,
    T: Send + 'static,
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

/// Image pixels per logical unit (a point on macOS, a DIP on Windows), per axis: 2 on Retina.
/// Written into every PNG, so other apps show it at the size it had on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Density {
    x: f64,
    y: f64,
}

impl Density {
    pub const ONE: Self = Self { x: 1.0, y: 1.0 };

    /// The scale per axis. An axis that is not finite and above 0 counts as 1×.
    pub const fn new(x: f64, y: f64) -> Self {
        const fn valid(scale: f64) -> f64 {
            if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            }
        }
        Self {
            x: valid(x),
            y: valid(y),
        }
    }

    /// A frame's `pixels` over its `logical` size, per axis. An axis with no logical size
    /// counts as 1×.
    pub fn of(pixels: (u32, u32), logical: (u32, u32)) -> Self {
        let axis = |px: u32, logical: u32| f64::from(px) / f64::from(logical);
        Self::new(axis(pixels.0, logical.0), axis(pixels.1, logical.1))
    }

    /// The PNG `pHYs` value per axis, in pixels per metre: the scale times the DPI of one
    /// logical unit, 72 for a macOS point and 96 for a Windows DIP. On macOS that is 2835 at 1×
    /// and 5669 (144 DPI) at 2×.
    pub fn pixels_per_metre(self) -> (u32, u32) {
        let dpi = if cfg!(target_os = "windows") {
            96.0
        } else {
            72.0
        };
        self.pixels_per_metre_at(dpi)
    }

    fn pixels_per_metre_at(self, dpi: f64) -> (u32, u32) {
        // The scale is finite and above 0. `as` saturates a huge one, and 0 would mean no
        // density, so the least is 1.
        let ppm = |scale: f64| ((dpi * scale / 0.0254).round() as u32).max(1);
        (ppm(self.x), ppm(self.y))
    }
}

/// Encodes the crop as PNG for copy and save. `Fast` compression: on a full Retina display it
/// is several times faster than any level that is smaller, and those save only a few percent
/// (see `docs/spikes/capture.md`). The PNG holds the pixels and their `density` (`pHYs`) and
/// nothing else: no text or EXIF chunks.
pub fn encode_png(image: &RgbaImage, density: Density) -> Result<Vec<u8>, CaptureError> {
    let encode = |e: png::EncodingError| CaptureError::Encode(e.to_string());
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // The filter after the compression, which sets one of its own.
    encoder.set_compression(png::Compression::Fast);
    encoder.set_filter(png::Filter::Adaptive);
    let (xppu, yppu) = density.pixels_per_metre();
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu,
        yppu,
        unit: png::Unit::Meter,
    }));
    let mut writer = encoder.write_header().map_err(encode)?;
    writer.write_image_data(image.as_raw()).map_err(encode)?;
    writer.finish().map_err(encode)?;
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

/// Captures one pixel of the first display and drops it. The first capture in a process pays a
/// one-time OS setup (about 60–150 ms on macOS); doing it at startup keeps it off the hotkey.
/// Same timeout as a capture, and its own stuck-display guard ([`WARMING`]), so a hotkey pressed
/// meanwhile still captures. Needs permission like [`capture_all`].
pub fn warm_up() -> Result<(), CaptureError> {
    let display = displays()?.first().ok_or(CaptureError::NoDisplays)?.id;
    let pixel = |id| monitor(id)?.capture_region(0, 0, 1, 1).map_err(backend);
    capture_each(&[display], pixel, DISPLAY_TIMEOUT, &WARMING)
        .pop()
        .unwrap_or(Err(CaptureError::NoDisplays))
        .map(drop)
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
fn monitor(id: u32) -> Result<xcap::Monitor, CaptureError> {
    xcap::Monitor::all()
        .map_err(backend)?
        .into_iter()
        .find(|m| m.id().ok() == Some(id))
        .ok_or_else(|| CaptureError::Backend(format!("display {id} disappeared")))
}

fn capture_display(id: u32) -> Result<DisplayFrame, CaptureError> {
    let m = monitor(id)?;
    let bounds = os_bounds(&m)?;
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

    #[test]
    fn a_hotkey_during_the_warm_up_still_captures() {
        // A warm-up of display 77 that is still running (stalled past its timeout).
        let (unblock, blocked) = mpsc::channel::<()>();
        let blocked = Mutex::new(blocked);
        let warm = capture_each(
            &[77],
            move |_| {
                blocked
                    .lock()
                    .unwrap()
                    .recv()
                    .map_err(|_| CaptureError::EmptyImage)
            },
            Duration::from_millis(10),
            &WARMING,
        );
        assert!(warm[0].is_err(), "timed out");
        assert!(WARMING.lock().unwrap().contains(&77));
        // The hotkey captures display 77 anyway.
        let captured = capture_each(&[77], |id| ok_frame(id, 0), DISPLAY_TIMEOUT, &IN_FLIGHT);
        assert_eq!(captured[0].as_ref().unwrap().id, 77);
        unblock.send(()).unwrap();
    }

    fn fresh() -> &'static Mutex<Vec<u32>> {
        Box::leak(Box::default())
    }

    #[test]
    fn png_round_trips() {
        let img = RgbaImage::from_fn(20, 10, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let png = encode_png(&img, Density::new(2.0, 2.0)).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().to_rgba8(), img);
        // The encoder rejects zero-sized images.
        let r = encode_png(&RgbaImage::new(0, 0), Density::ONE);
        assert!(matches!(r, Err(CaptureError::Encode(_))));
    }

    #[test]
    fn density_is_the_logical_dpi_times_the_scale() {
        // macOS: a point is 1/72 inch.
        let ppm = |x, y| Density::new(x, y).pixels_per_metre_at(72.0);
        assert_eq!(ppm(1.0, 1.0), (2835, 2835), "72 DPI");
        assert_eq!(ppm(2.0, 2.0), (5669, 5669), "144 DPI");
        assert_eq!(ppm(1.5, 1.5), (4252, 4252), "108 DPI");
        assert_eq!(ppm(2.0, 1.0), (5669, 2835), "per axis");
        // Never 0 (no density) and never wrapped.
        assert_eq!(ppm(1e-9, 1e300), (1, u32::MAX), "clamped");
        // Windows: a DIP is 1/96 inch.
        let win = |s| Density::new(s, s).pixels_per_metre_at(96.0).0;
        assert_eq!((win(1.0), win(1.25), win(1.5)), (3780, 4724, 5669));
        let native = if cfg!(target_os = "windows") {
            96.0
        } else {
            72.0
        };
        assert_eq!(
            Density::ONE.pixels_per_metre(),
            Density::ONE.pixels_per_metre_at(native)
        );
    }

    #[test]
    fn density_is_pixels_over_logical_size() {
        let d = Density::new;
        assert_eq!(Density::of((3456, 2234), (1728, 1117)), d(2.0, 2.0));
        assert_eq!(Density::of((2880, 1620), (1920, 1080)), d(1.5, 1.5));
        assert_eq!(Density::of((200, 100), (100, 100)), d(2.0, 1.0));
        assert_eq!(
            Density::of((200, 100), (0, 50)),
            d(1.0, 2.0),
            "no logical size"
        );
        assert_eq!(Density::of((0, 0), (0, 10)), Density::ONE, "no pixels");
    }

    #[test]
    fn an_invalid_scale_counts_as_1x() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -0.0, -2.0] {
            assert_eq!(Density::new(bad, 2.0), Density::new(1.0, 2.0), "{bad}");
            assert_eq!(Density::new(2.0, bad), Density::new(2.0, 1.0), "{bad}");
        }
        assert_eq!(Density::new(f64::NAN, -1.0), Density::ONE);
        let ppm = Density::new(f64::NAN, f64::INFINITY).pixels_per_metre_at(72.0);
        assert_eq!(ppm, (2835, 2835));
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
            CaptureError::Clipboard("z".into()),
            CaptureError::Write("w".into()),
            CaptureError::Dialog("d".into()),
        ]
        .map(|e| json(e)["kind"].as_str().unwrap().to_owned());
        assert_eq!(
            kinds,
            [
                "emptyImage",
                "noDisplays",
                "emptySelection",
                "encode",
                "clipboard",
                "write",
                "dialog"
            ]
        );
    }

    /// Pixel bounds and the cursor only line up if the process is Per-Monitor v2 DPI aware.
    /// The manifest (`windows-app-manifest.xml`) sets it, in test binaries too.
    #[test]
    #[cfg(target_os = "windows")]
    fn process_is_per_monitor_v2_dpi_aware() {
        use windows::Win32::UI::HiDpi::{
            AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            GetThreadDpiAwarenessContext,
        };
        // SAFETY: plain queries with no pointers.
        let v2 = unsafe {
            AreDpiAwarenessContextsEqual(
                GetThreadDpiAwarenessContext(),
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            )
        };
        assert!(v2.as_bool());
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn other_platforms_need_no_permission() {
        assert!(has_permission());
        assert!(request_permission());
    }
}
