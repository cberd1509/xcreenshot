//! CoreGraphics capture (macOS): the `CGWindowListCreateImage` call xcap makes, with fewer copies
//! of the frame (#55). xcap copies the image's pixels into a `CFData`, copies that again into a
//! `Vec`, copies the rows a third time and then swaps blue and red in a second pass. Here the
//! `CFData` bytes go straight into the RGBA frame in one pass (`backend::bgra_to_rgba`).
//!
//! Also the pieces ScreenCaptureKit shares: the display's geometry and the pixel copy. OS calls
//! only; the decisions are in `backend.rs`.

use super::{Bounds, CaptureError, backend};
use image::RgbaImage;
use objc2_core_foundation::{CFRetained, CGRect, CGSize};
use objc2_core_graphics::{
    CGDataProvider, CGDisplayBounds, CGDisplayCopyDisplayMode, CGDisplayMode, CGImage,
    CGImageAlphaInfo, CGImageByteOrderInfo, CGWindowImageOption, CGWindowListOption,
};

/// Display `id`'s bounds in points and its native size in pixels, from its display mode.
pub fn geometry(id: u32) -> (Bounds, (usize, usize)) {
    let points = points(CGDisplayBounds(id));
    let mode = CGDisplayCopyDisplayMode(id);
    let pixels = backend::pixel_size(
        (points.width, points.height),
        CGDisplayMode::pixel_width(mode.as_deref()),
        CGDisplayMode::width(mode.as_deref()),
    );
    (points, pixels)
}

fn points(rect: CGRect) -> Bounds {
    Bounds {
        x: rect.origin.x as i32,
        y: rect.origin.y as i32,
        width: rect.size.width as u32,
        height: rect.size.height as u32,
    }
}

/// Captures display `id` at its native resolution, as xcap does: every window on it, so
/// xcreenshot's own windows must be hidden (they are, until the capture is done). Nothing is
/// kept: the image and its pixel copy are freed on return.
pub fn capture(id: u32) -> Result<(Bounds, f64, RgbaImage), CaptureError> {
    let rect = CGDisplayBounds(id);
    let image = to_rgba(&*shoot(rect)?, "CoreGraphics")?;
    let points = points(rect);
    let scale = f64::from(image.width()) / f64::from(points.width.max(1));
    Ok((points, scale, image))
}

/// Captures 1 point of display `id` and drops it, so the first capture does not pay the
/// one-time setup.
pub fn warm_up(id: u32) -> Result<(), CaptureError> {
    let origin = CGDisplayBounds(id).origin;
    shoot(CGRect::new(origin, CGSize::new(1.0, 1.0))).map(drop)
}

fn shoot(rect: CGRect) -> Result<CFRetained<CGImage>, CaptureError> {
    // Deprecated (#49): `backend.rs` falls back to ScreenCaptureKit when it stops working, and
    // `capture::support` checks that it still exists before it is called.
    #[allow(deprecated)]
    objc2_core_graphics::CGWindowListCreateImage(
        rect,
        CGWindowListOption::OptionAll,
        0,
        CGWindowImageOption::Default,
    )
    .ok_or_else(|| CaptureError::Backend("CoreGraphics returned no image".into()))
}

/// Copies a BGRA image (8 bits per channel, little-endian with alpha first, rows maybe padded)
/// into an RGBA one. Anything else is refused rather than shown with the wrong colours. `what`
/// names the backend in the error.
pub fn to_rgba(image: &CGImage, what: &str) -> Result<RgbaImage, CaptureError> {
    let bad = |why: &str| CaptureError::Backend(format!("{what} image: {why}"));
    let alpha_first = matches!(
        CGImage::alpha_info(Some(image)),
        CGImageAlphaInfo::PremultipliedFirst
            | CGImageAlphaInfo::NoneSkipFirst
            | CGImageAlphaInfo::First
    );
    if CGImage::bits_per_pixel(Some(image)) != 32
        || CGImage::byte_order_info(Some(image)) != CGImageByteOrderInfo::Order32Little
        || !alpha_first
    {
        return Err(bad("not BGRA"));
    }
    let data = CGDataProvider::data(CGImage::data_provider(Some(image)).as_deref())
        .ok_or_else(|| bad("no pixels"))?;
    // SAFETY: `data` is immutable and alive for the copy.
    let bytes = unsafe { data.as_bytes_unchecked() };
    backend::bgra_to_rgba(
        bytes,
        CGImage::width(Some(image)),
        CGImage::height(Some(image)),
        CGImage::bytes_per_row(Some(image)),
    )
    .ok_or_else(|| bad("wrong size"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_is_in_points_and_pixels() {
        let main = objc2_core_graphics::CGMainDisplayID();
        let (points, (w, h)) = geometry(main);
        assert!(points.width > 0 && points.height > 0);
        assert!(w >= points.width as usize && h >= points.height as usize);
    }

    #[test]
    fn a_capture_matches_the_display_geometry() {
        // Without permission CoreGraphics may show the system prompt; never ask from a test.
        if !super::super::has_permission() {
            return;
        }
        let main = objc2_core_graphics::CGMainDisplayID();
        let (points, scale, image) = capture(main).unwrap();
        assert_eq!(points, geometry(main).0);
        assert_eq!(
            image.width(),
            (f64::from(points.width) * scale).round() as u32
        );
        warm_up(main).unwrap();
    }
}
