//! Screen capture: grab every display at native resolution, then crop a selection.
//!
//! The OS calls (via the `xcap` crate) sit in a thin wrapper at the bottom of this file.
//! Everything else (geometry, permission handling, cropping, encoding) is pure and unit-tested.
//! See `docs/spikes/capture.md` for why `xcap` was chosen and measured latency.

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use std::fmt;

/// One display, frozen at the moment of capture.
///
/// `x`, `y`, `width` and `height` are logical (points on macOS, DIPs on Windows), in the same
/// global space the overlay windows are positioned in. `image` holds the native pixels
/// (`width * scale_factor` by `height * scale_factor`, give or take rounding).
///
/// Pixels are kept as raw RGBA rather than PNG: capture yields RGBA, cropping needs RGBA, and
/// encoding is the slow part (~30 ms fast / ~450 ms small for a 3600x2338 Retina frame), so it
/// is done only when needed. The overlay should get [`encode_png`] with [`PngSpeed::Fast`]
/// (about 3 MB, decoded by the webview off the main thread); saved files use [`PngSpeed::Small`].
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayFrame {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub image: RgbaImage,
}

/// A selection in logical points, relative to the display's top-left corner.
/// Negative sizes (dragging up or left) are allowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A region in native pixels inside a frame's image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// Screen Recording is not allowed (macOS). Call [`request_permission`] and ask the user to
    /// enable xcreenshot in System Settings > Privacy & Security > Screen & System Audio Recording.
    PermissionDenied,
    NoDisplays,
    /// The selection does not cover any pixel of the display.
    EmptySelection,
    Backend(String),
    Encode(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied => f.write_str("Screen Recording permission is not granted"),
            Self::NoDisplays => f.write_str("no displays found"),
            Self::EmptySelection => f.write_str("the selection is empty"),
            Self::Backend(e) => write!(f, "screen capture failed: {e}"),
            Self::Encode(e) => write!(f, "PNG encoding failed: {e}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Tauri commands return errors as plain strings.
impl serde::Serialize for CaptureError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngSpeed {
    /// For showing a frame in the overlay right away.
    Fast,
    /// For files the user keeps.
    Small,
}

/// Display geometry as the OS reports it, before it becomes a [`DisplayFrame`].
#[derive(Debug, Clone, PartialEq)]
struct RawDisplay {
    id: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale_factor: f64,
}

/// Whether the OS reports display geometry in physical pixels (Windows) or points (macOS).
const OS_REPORTS_PHYSICAL: bool = cfg!(target_os = "windows");

/// What the OS layer returns for one display: its geometry and the captured pixels.
type RawCapture = (RawDisplay, Result<RgbaImage, String>);

/// Builds the frame for one display. `physical` says the OS geometry is in pixels and must be
/// divided by the scale factor.
fn to_frame(
    raw: RawDisplay,
    image: Result<RgbaImage, String>,
    physical: bool,
) -> Result<DisplayFrame, CaptureError> {
    let image = match image {
        Ok(img) if img.width() > 0 && img.height() > 0 => img,
        // An empty image means capture was blocked, e.g. Screen Recording revoked mid-session.
        Ok(_) => return Err(CaptureError::PermissionDenied),
        Err(e) => return Err(CaptureError::Backend(e)),
    };
    let scale = sane_scale(raw.scale_factor);
    let (x, y, width, height) = if physical {
        let to_logical = |v: f64| (v / scale).round();
        (
            to_logical(raw.x.into()) as i32,
            to_logical(raw.y.into()) as i32,
            to_logical(raw.width.into()) as u32,
            to_logical(raw.height.into()) as u32,
        )
    } else {
        (raw.x, raw.y, raw.width, raw.height)
    };
    Ok(DisplayFrame {
        id: raw.id,
        x,
        y,
        width,
        height,
        scale_factor: scale,
        image,
    })
}

/// Guards against a zero, negative or NaN scale factor from a misbehaving driver.
fn sane_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// [`capture_all`] with the OS calls passed in. `list` runs only when capture is permitted.
/// Fails if any display fails, so the overlay never shows a partly frozen desktop.
fn capture_with(
    permitted: bool,
    physical: bool,
    list: impl FnOnce() -> Result<Vec<RawCapture>, String>,
) -> Result<Vec<DisplayFrame>, CaptureError> {
    if !permitted {
        return Err(CaptureError::PermissionDenied);
    }
    let displays = list().map_err(CaptureError::Backend)?;
    if displays.is_empty() {
        return Err(CaptureError::NoDisplays);
    }
    displays
        .into_iter()
        .map(|(raw, image)| to_frame(raw, image, physical))
        .collect()
}

/// Maps a logical selection to native pixels, clamped to the image. Edges are rounded to the
/// nearest pixel, so a selection snapped to device pixels in the overlay maps exactly.
pub fn to_pixels(frame: &DisplayFrame, rect: Rect) -> Result<PixelRect, CaptureError> {
    let values = [rect.x, rect.y, rect.width, rect.height];
    if values.iter().any(|v| !v.is_finite()) {
        return Err(CaptureError::EmptySelection);
    }
    let scale = sane_scale(frame.scale_factor);
    let edge = |v: f64, max: u32| (v * scale).round().clamp(0.0, max.into()) as u32;
    let (w, h) = frame.image.dimensions();
    let (left, right) = ordered(rect.x, rect.x + rect.width);
    let (top, bottom) = ordered(rect.y, rect.y + rect.height);
    let (x0, x1) = (edge(left, w), edge(right, w));
    let (y0, y1) = (edge(top, h), edge(bottom, h));
    if x1 <= x0 || y1 <= y0 {
        return Err(CaptureError::EmptySelection);
    }
    Ok(PixelRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn ordered(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Cuts a logical selection out of a frame at native resolution.
pub fn crop(frame: &DisplayFrame, rect: Rect) -> Result<RgbaImage, CaptureError> {
    let p = to_pixels(frame, rect)?;
    Ok(image::imageops::crop_imm(&frame.image, p.x, p.y, p.width, p.height).to_image())
}

/// Encodes an image as PNG.
pub fn encode_png(image: &RgbaImage, speed: PngSpeed) -> Result<Vec<u8>, CaptureError> {
    let compression = match speed {
        PngSpeed::Fast => CompressionType::Fast,
        PngSpeed::Small => CompressionType::Default,
    };
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, compression, FilterType::Adaptive)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| CaptureError::Encode(e.to_string()))?;
    Ok(out)
}

/// Captures every display at native resolution. On macOS this returns
/// [`CaptureError::PermissionDenied`] without touching the screen when Screen Recording is off,
/// so it never triggers the system prompt by itself; call [`request_permission`] for that.
pub fn capture_all() -> Result<Vec<DisplayFrame>, CaptureError> {
    capture_with(has_permission(), OS_REPORTS_PHYSICAL, list_displays)
}

// ---- OS calls only below; exercised by the ignored test and `examples/capture.rs`. ----

fn list_displays() -> Result<Vec<RawCapture>, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let raw = |m: &xcap::Monitor| RawDisplay {
        id: m.id().unwrap_or_default(),
        x: m.x().unwrap_or_default(),
        y: m.y().unwrap_or_default(),
        width: m.width().unwrap_or_default(),
        height: m.height().unwrap_or_default(),
        scale_factor: m.scale_factor().map_or(1.0, f64::from),
    };
    let image = |m: &xcap::Monitor| m.capture_image().map_err(|e| e.to_string());
    Ok(monitors.iter().map(|m| (raw(m), image(m))).collect())
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
    use image::Rgba;

    fn raw(width: u32, height: u32, scale: f64) -> RawDisplay {
        RawDisplay {
            id: 7,
            x: -100,
            y: 50,
            width,
            height,
            scale_factor: scale,
        }
    }

    /// A frame whose pixel (x, y) is colored (x, y, 0), so crops can be checked by content.
    fn frame(width: u32, height: u32, scale: f64) -> DisplayFrame {
        let w = (f64::from(width) * scale).round() as u32;
        let h = (f64::from(height) * scale).round() as u32;
        DisplayFrame {
            id: 1,
            x: 0,
            y: 0,
            width,
            height,
            scale_factor: scale,
            image: RgbaImage::from_fn(w, h, |x, y| Rgba([x as u8, y as u8, 0, 255])),
        }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn px(x: u32, y: u32, width: u32, height: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn macos_geometry_is_already_logical() {
        let img = RgbaImage::new(3600, 2338);
        let f = to_frame(raw(1800, 1169, 2.0), Ok(img), false).unwrap();
        assert_eq!(
            (f.id, f.x, f.y, f.width, f.height),
            (7, -100, 50, 1800, 1169)
        );
        assert_eq!(f.scale_factor, 2.0);
        assert_eq!(f.image.dimensions(), (3600, 2338));
    }

    #[test]
    fn windows_geometry_is_converted_to_logical() {
        let img = RgbaImage::new(2560, 1440);
        let f = to_frame(raw(2560, 1440, 1.5), Ok(img), true).unwrap();
        // -100 / 1.5 = -66.7 and 50 / 1.5 = 33.3, rounded.
        assert_eq!((f.x, f.y, f.width, f.height), (-67, 33, 1707, 960));
    }

    #[test]
    fn bad_scale_factor_falls_back_to_one() {
        for bad in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            let f = to_frame(raw(10, 10, bad), Ok(RgbaImage::new(10, 10)), true).unwrap();
            assert_eq!(f.scale_factor, 1.0);
            assert_eq!((f.width, f.height), (10, 10));
        }
    }

    #[test]
    fn empty_image_means_permission_denied() {
        for img in [
            RgbaImage::new(0, 0),
            RgbaImage::new(0, 10),
            RgbaImage::new(10, 0),
        ] {
            let r = to_frame(raw(10, 10, 1.0), Ok(img), false);
            assert_eq!(r, Err(CaptureError::PermissionDenied));
        }
    }

    #[test]
    fn display_errors_are_backend_errors() {
        let r = to_frame(raw(1, 1, 1.0), Err("boom".into()), false);
        assert_eq!(r, Err(CaptureError::Backend("boom".into())));
    }

    #[test]
    fn denied_permission_skips_the_os_calls() {
        let r = capture_with(false, false, || panic!("must not capture"));
        assert_eq!(r, Err(CaptureError::PermissionDenied));
    }

    #[test]
    fn capture_needs_every_display() {
        let ok = (raw(2, 2, 1.0), Ok(RgbaImage::new(2, 2)));
        let bad = (raw(2, 2, 1.0), Err("gone".to_string()));
        let frames = capture_with(true, false, || Ok(vec![ok.clone(), ok.clone()])).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(
            capture_with(true, false, || Ok(vec![ok, bad])),
            Err(CaptureError::Backend("gone".into()))
        );
        assert_eq!(
            capture_with(true, false, || Ok(vec![])),
            Err(CaptureError::NoDisplays)
        );
        assert_eq!(
            capture_with(true, false, || Err("no list".into())),
            Err(CaptureError::Backend("no list".into()))
        );
    }

    #[test]
    fn maps_points_to_pixels_at_1x() {
        let f = frame(100, 80, 1.0);
        assert_eq!(
            to_pixels(&f, rect(10.0, 20.0, 30.0, 40.0)),
            Ok(px(10, 20, 30, 40))
        );
    }

    #[test]
    fn maps_points_to_pixels_at_2x() {
        let f = frame(100, 80, 2.0);
        assert_eq!(
            to_pixels(&f, rect(10.0, 20.0, 30.0, 40.0)),
            Ok(px(20, 40, 60, 80))
        );
        // Half-point edges land on whole Retina pixels.
        assert_eq!(to_pixels(&f, rect(0.5, 0.5, 1.0, 1.0)), Ok(px(1, 1, 2, 2)));
    }

    #[test]
    fn maps_points_to_pixels_at_1_5x() {
        let f = frame(100, 80, 1.5);
        assert_eq!(
            to_pixels(&f, rect(10.0, 20.0, 30.0, 40.0)),
            Ok(px(15, 30, 45, 60))
        );
        // 1.5 -> 2 and 4.5 -> 5 (round half away from zero): 3 pixels for 2 points.
        assert_eq!(to_pixels(&f, rect(1.0, 1.0, 2.0, 2.0)), Ok(px(2, 2, 3, 3)));
    }

    #[test]
    fn whole_display_maps_to_whole_image() {
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let f = frame(101, 67, scale);
            let (w, h) = f.image.dimensions();
            let all = rect(0.0, 0.0, 101.0, 67.0);
            assert_eq!(to_pixels(&f, all), Ok(px(0, 0, w, h)), "scale {scale}");
        }
    }

    #[test]
    fn negative_size_is_normalized() {
        let f = frame(100, 80, 2.0);
        assert_eq!(
            to_pixels(&f, rect(40.0, 60.0, -30.0, -40.0)),
            to_pixels(&f, rect(10.0, 20.0, 30.0, 40.0))
        );
    }

    #[test]
    fn clamps_to_the_display() {
        let f = frame(100, 80, 2.0);
        assert_eq!(
            to_pixels(&f, rect(-10.0, -10.0, 30.0, 30.0)),
            Ok(px(0, 0, 40, 40))
        );
        assert_eq!(
            to_pixels(&f, rect(90.0, 70.0, 50.0, 50.0)),
            Ok(px(180, 140, 20, 20))
        );
        assert_eq!(
            to_pixels(&f, rect(-1e9, -1e9, 2e9, 2e9)),
            Ok(px(0, 0, 200, 160))
        );
    }

    #[test]
    fn empty_selections_are_rejected() {
        let f = frame(100, 80, 2.0);
        let cases = [
            rect(10.0, 10.0, 0.0, 20.0),
            rect(10.0, 10.0, 20.0, 0.0),
            rect(10.0, 10.0, 0.1, 20.0), // under half a pixel
            rect(200.0, 10.0, 20.0, 20.0),
            rect(10.0, 200.0, 20.0, 20.0),
            rect(-50.0, 10.0, 20.0, 20.0),
            rect(10.0, -50.0, 20.0, 20.0),
            rect(f64::NAN, 0.0, 10.0, 10.0),
            rect(0.0, 0.0, f64::INFINITY, 10.0),
        ];
        for r in cases {
            assert_eq!(to_pixels(&f, r), Err(CaptureError::EmptySelection), "{r:?}");
            assert_eq!(crop(&f, r), Err(CaptureError::EmptySelection), "{r:?}");
        }
    }

    #[test]
    fn crop_returns_the_native_pixels() {
        let f = frame(100, 80, 2.0);
        let img = crop(&f, rect(10.0, 5.0, 4.0, 3.0)).unwrap();
        assert_eq!(img.dimensions(), (8, 6));
        assert_eq!(img.get_pixel(0, 0), &Rgba([20, 10, 0, 255]));
        assert_eq!(img.get_pixel(7, 5), &Rgba([27, 15, 0, 255]));
    }

    #[test]
    fn png_round_trips_at_both_speeds() {
        let f = frame(20, 10, 1.0);
        for speed in [PngSpeed::Fast, PngSpeed::Small] {
            let png = encode_png(&f.image, speed).unwrap();
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            let back = image::load_from_memory(&png).unwrap().to_rgba8();
            assert_eq!(back, f.image);
        }
    }

    #[test]
    fn png_reports_encoder_errors() {
        // The encoder rejects zero-sized images.
        let r = encode_png(&RgbaImage::new(0, 0), PngSpeed::Fast);
        assert!(matches!(r, Err(CaptureError::Encode(_))));
    }

    #[test]
    fn errors_have_plain_messages() {
        let all = [
            CaptureError::PermissionDenied,
            CaptureError::NoDisplays,
            CaptureError::EmptySelection,
            CaptureError::Backend("x".into()),
            CaptureError::Encode("y".into()),
        ];
        for e in &all {
            assert!(!e.to_string().is_empty());
        }
        let json = serde_json::to_string(&CaptureError::Backend("x".into())).unwrap();
        assert_eq!(json, "\"screen capture failed: x\"");
    }

    #[test]
    fn permission_check_does_not_prompt() {
        // Preflight never shows UI, so it is safe in CI; the value depends on the machine.
        let _ = has_permission();
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn other_platforms_need_no_permission() {
        assert!(has_permission());
        assert!(request_permission());
    }

    // Prints latency and geometry: `cargo run --release --example capture`.
    #[test]
    #[ignore = "needs a real display and Screen Recording permission"]
    fn captures_every_display_at_native_resolution() {
        for f in capture_all().unwrap() {
            let expected = (f64::from(f.width) * f.scale_factor).round() as i64;
            assert!((i64::from(f.image.width()) - expected).abs() <= 1);
        }
    }
}
