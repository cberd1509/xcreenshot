//! Pure geometry: logical/physical display bounds and mapping a selection to native pixels.

use super::{CaptureError, DisplayFrame};
use image::RgbaImage;

/// A display's position and size in one coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
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

/// A rect in a frame's native pixels, from its top-left corner. The overlay page sends the
/// selection this way, so the crop is exactly the size its badge showed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Same rule as Tauri's `dpi::validate_scale_factor`; anything else falls back to 1.
fn valid_scale(scale: f64) -> f64 {
    if scale.is_sign_positive() && scale.is_normal() {
        scale
    } else {
        1.0
    }
}

fn scaled(v: i32, by: f64) -> i32 {
    (f64::from(v) * by).round() as i32
}

/// The OS gave logical bounds (macOS points). Physical bounds use the scale measured from the
/// image, which stays right when the OS-reported scale is off (rotated displays, a failed
/// display-mode query). This matches how Tauri reports macOS monitor positions.
pub(super) fn from_points(logical: Bounds, image: &RgbaImage) -> (Bounds, Bounds) {
    let scale = f64::from(image.width()) / f64::from(logical.width.max(1));
    let physical = Bounds {
        x: scaled(logical.x, scale),
        y: scaled(logical.y, scale),
        width: image.width(),
        height: image.height(),
    };
    (logical, physical)
}

/// The OS gave physical bounds (Windows pixels). Logical bounds divide by that monitor's own
/// scale, so on mixed-DPI setups logical origins of different monitors do not line up exactly;
/// the physical bounds are the ones to position windows with.
pub(super) fn from_pixels(
    physical: Bounds,
    scale_factor: f64,
    image: &RgbaImage,
) -> (Bounds, Bounds) {
    let to_logical = 1.0 / valid_scale(scale_factor);
    let physical = Bounds {
        width: image.width(),
        height: image.height(),
        ..physical
    };
    let logical = Bounds {
        x: scaled(physical.x, to_logical),
        y: scaled(physical.y, to_logical),
        width: (f64::from(physical.width) * to_logical).round() as u32,
        height: (f64::from(physical.height) * to_logical).round() as u32,
    };
    (logical, physical)
}

/// Maps a logical selection to native pixels, clamped to the image. The scale comes from the
/// image itself (pixels per point on each axis), not the OS-reported scale factor. Edges are
/// rounded to the nearest pixel, so a selection snapped to device pixels maps exactly.
fn to_pixels(frame: &DisplayFrame, rect: Rect) -> Result<PixelRect, CaptureError> {
    let finite = [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .all(|v| v.is_finite());
    let (w, h) = frame.image.dimensions();
    if !finite || frame.logical.width == 0 || frame.logical.height == 0 {
        return Err(CaptureError::EmptySelection);
    }
    let sx = f64::from(w) / f64::from(frame.logical.width);
    let sy = f64::from(h) / f64::from(frame.logical.height);
    let edge = |v: f64, s: f64, max: u32| (v * s).round().clamp(0.0, max.into()) as u32;
    let (x0, x1) = (
        rect.x.min(rect.x + rect.width),
        rect.x.max(rect.x + rect.width),
    );
    let (y0, y1) = (
        rect.y.min(rect.y + rect.height),
        rect.y.max(rect.y + rect.height),
    );
    let (x0, x1) = (edge(x0, sx, w), edge(x1, sx, w));
    let (y0, y1) = (edge(y0, sy, h), edge(y1, sy, h));
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

/// Cuts a logical selection out of a frame at native resolution.
pub fn crop(frame: &DisplayFrame, rect: Rect) -> Result<RgbaImage, CaptureError> {
    crop_pixels(&frame.image, to_pixels(frame, rect)?)
}

/// Cuts a pixel rect out of an image, clamped to it. Empty after clamping is an error.
pub fn crop_pixels(image: &RgbaImage, rect: PixelRect) -> Result<RgbaImage, CaptureError> {
    let (w, h) = image.dimensions();
    let (x0, y0) = (rect.x.min(w), rect.y.min(h));
    let x1 = rect.x.saturating_add(rect.width).min(w);
    let y1 = rect.y.saturating_add(rect.height).min(h);
    if x1 <= x0 || y1 <= y0 {
        return Err(CaptureError::EmptySelection);
    }
    let p = PixelRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    };
    let stride = w as usize * 4;
    let (left, row) = (p.x as usize * 4, p.width as usize * 4);
    let src = image.as_raw();
    let mut out = vec![0; row * p.height as usize];
    for (y, dst) in (p.y as usize..).zip(out.chunks_exact_mut(row)) {
        let start = y * stride + left;
        dst.copy_from_slice(&src[start..start + row]);
    }
    RgbaImage::from_raw(p.width, p.height, out).ok_or(CaptureError::EmptySelection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// A frame of `width` x `height` points with a `px_w` x `px_h` image whose pixel (x, y) is
    /// colored (x, y, 0), so crops can be checked by content.
    fn frame(width: u32, height: u32, px_w: u32, px_h: u32, scale_factor: f64) -> DisplayFrame {
        let image = RgbaImage::from_fn(px_w, px_h, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        let logical = Bounds {
            x: 0,
            y: 0,
            width,
            height,
        };
        let (logical, physical) = from_points(logical, &image);
        DisplayFrame {
            id: 1,
            logical,
            physical,
            scale_factor,
            image,
        }
    }

    /// A frame whose image is exactly `scale` times its logical size.
    fn scaled_frame(width: u32, height: u32, scale: f64) -> DisplayFrame {
        let px = |v: u32| (f64::from(v) * scale).round() as u32;
        frame(width, height, px(width), px(height), scale)
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

    fn bounds(x: i32, y: i32, width: u32, height: u32) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn points_geometry_derives_physical_from_the_image() {
        let img = RgbaImage::new(3600, 2338);
        let (logical, physical) = from_points(bounds(-1800, 100, 1800, 1169), &img);
        assert_eq!(logical, bounds(-1800, 100, 1800, 1169));
        assert_eq!(physical, bounds(-3600, 200, 3600, 2338));
    }

    #[test]
    fn pixel_geometry_divides_by_the_monitor_scale() {
        let img = RgbaImage::new(2560, 1440);
        let (logical, physical) = from_pixels(bounds(-100, 50, 2560, 1440), 1.5, &img);
        assert_eq!(physical, bounds(-100, 50, 2560, 1440));
        // -100 / 1.5 = -66.7, 50 / 1.5 = 33.3 and 2560 / 1.5 = 1706.7, rounded.
        assert_eq!(logical, bounds(-67, 33, 1707, 960));
    }

    #[test]
    fn bad_scale_factor_is_treated_as_one() {
        let img = RgbaImage::new(10, 10);
        for bad in [0.0, -2.0, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE / 2.0] {
            let (logical, _) = from_pixels(bounds(4, 4, 10, 10), bad, &img);
            assert_eq!(logical, bounds(4, 4, 10, 10), "{bad}");
        }
    }

    // Shared with `toPhysical` in src/overlay/geometry.test.ts: keep both in step.
    #[test]
    fn maps_points_to_pixels_at_each_scale() {
        let cases = [
            (1.0, rect(10.0, 20.0, 30.0, 40.0), px(10, 20, 30, 40)),
            (2.0, rect(10.0, 20.0, 30.0, 40.0), px(20, 40, 60, 80)),
            (2.0, rect(0.5, 0.5, 1.0, 1.0), px(1, 1, 2, 2)), // half points are whole pixels
            (1.5, rect(10.0, 20.0, 30.0, 40.0), px(15, 30, 45, 60)),
            (1.5, rect(1.0, 1.0, 2.0, 2.0), px(2, 2, 3, 3)), // 1.5 -> 2, 4.5 -> 5
        ];
        for (scale, r, expected) in cases {
            let f = scaled_frame(100, 80, scale);
            assert_eq!(to_pixels(&f, r), Ok(expected), "scale {scale}, {r:?}");
        }
    }

    #[test]
    fn whole_display_maps_to_whole_image() {
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let f = scaled_frame(101, 67, scale);
            let (w, h) = f.image.dimensions();
            let all = rect(0.0, 0.0, 101.0, 67.0);
            assert_eq!(to_pixels(&f, all), Ok(px(0, 0, w, h)), "scale {scale}");
        }
    }

    #[test]
    fn rotated_display_uses_the_image_scale() {
        // Portrait 2x display: the display mode reports an unrotated pixel width, so the OS
        // scale factor comes out as 3600 / 1169 = 3.08. The image says 2x on both axes.
        let f = frame(1169, 1800, 2338, 3600, 3600.0 / 1169.0);
        assert_eq!(
            to_pixels(&f, rect(100.0, 1000.0, 50.0, 60.0)),
            Ok(px(200, 2000, 100, 120))
        );
    }

    #[test]
    fn scale_factor_mismatch_is_ignored() {
        // The OS said 1.0 (display-mode query failed) but the image is 2x.
        let f = frame(100, 80, 200, 160, 1.0);
        assert_eq!(
            to_pixels(&f, rect(10.0, 10.0, 20.0, 20.0)),
            Ok(px(20, 20, 40, 40))
        );
        // Non-uniform: each axis uses its own scale.
        let f = frame(100, 80, 200, 80, 2.0);
        assert_eq!(
            to_pixels(&f, rect(10.0, 10.0, 20.0, 20.0)),
            Ok(px(20, 10, 40, 20))
        );
    }

    #[test]
    fn negative_size_is_normalized() {
        let f = scaled_frame(100, 80, 2.0);
        assert_eq!(
            to_pixels(&f, rect(40.0, 60.0, -30.0, -40.0)),
            to_pixels(&f, rect(10.0, 20.0, 30.0, 40.0))
        );
    }

    // Shared with `toPhysical` in src/overlay/geometry.test.ts.
    #[test]
    fn clamps_to_the_display() {
        let f = scaled_frame(100, 80, 2.0);
        let cases = [
            (rect(-10.0, -10.0, 30.0, 30.0), px(0, 0, 40, 40)),
            (rect(90.0, 70.0, 50.0, 50.0), px(180, 140, 20, 20)),
            (rect(-1e9, -1e9, 2e9, 2e9), px(0, 0, 200, 160)),
        ];
        for (r, expected) in cases {
            assert_eq!(to_pixels(&f, r), Ok(expected), "{r:?}");
        }
    }

    #[test]
    fn empty_selections_are_rejected() {
        let f = scaled_frame(100, 80, 2.0);
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
            assert_eq!(crop(&f, r), Err(CaptureError::EmptySelection), "{r:?}");
        }
        let zero = frame(0, 0, 0, 0, 1.0);
        let r = rect(0.0, 0.0, 1.0, 1.0);
        assert_eq!(crop(&zero, r), Err(CaptureError::EmptySelection));
    }

    #[test]
    fn crop_returns_the_native_pixels() {
        let f = scaled_frame(100, 80, 2.0);
        let img = crop(&f, rect(10.0, 5.0, 4.0, 3.0)).unwrap();
        let expected = RgbaImage::from_fn(8, 6, |x, y| Rgba([20 + x as u8, 10 + y as u8, 0, 255]));
        assert_eq!(img, expected);
    }

    #[test]
    fn crops_a_pixel_rect_clamped_to_the_image() {
        let img = scaled_frame(100, 80, 2.0).image; // 200 x 160 px
        let at = |x, y, w, h| crop_pixels(&img, px(x, y, w, h)).map(|i| i.dimensions());
        assert_eq!(at(20, 10, 8, 6), Ok((8, 6)));
        assert_eq!(at(190, 150, 50, 50), Ok((10, 10)));
        assert_eq!(at(0, 0, u32::MAX, u32::MAX), Ok((200, 160)));
        let cut = crop_pixels(&img, px(20, 10, 2, 1)).unwrap();
        assert_eq!(cut.as_raw(), &[20, 10, 0, 255, 21, 10, 0, 255]);
        for empty in [
            px(5, 5, 0, 5),
            px(5, 5, 5, 0),
            px(200, 0, 5, 5),
            px(0, 160, 5, 5),
        ] {
            let r = crop_pixels(&img, empty);
            assert_eq!(r, Err(CaptureError::EmptySelection), "{empty:?}");
        }
    }

    #[test]
    fn pixel_rect_comes_from_json() {
        let r: PixelRect = serde_json::from_str(r#"{"x":1,"y":2,"width":3,"height":4}"#).unwrap();
        assert_eq!(r, px(1, 2, 3, 4));
        let negative = r#"{"x":-1,"y":0,"width":1,"height":1}"#;
        assert!(serde_json::from_str::<PixelRect>(negative).is_err());
    }
}
