//! Copy and Save at a larger size: whole factors only, Lanczos3, pure.
//!
//! The page offers 2×, 3× and 4× under the Copy and Save carets, and disables a factor whose
//! output would pass [`MAX_SIDE`] on either side (`src/overlay/annotate/upscale.ts` mirrors
//! these rules; their tests pin the same values). Rust checks the factor again, so a broken page
//! can't ask for more. See issue #107 for the spike that picked Lanczos3.

use crate::capture::CaptureError;
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;

/// The factors on offer, besides 1×.
pub const FACTORS: [u32; 3] = [2, 3, 4];

/// The largest output, in px per side: a 256 MiB RGBA buffer.
pub const MAX_SIDE: u32 = 8192;

/// `width × height` px at `factor`, or `None` past [`MAX_SIDE`] or an overflow.
pub fn scaled(width: u32, height: u32, factor: u32) -> Option<(u32, u32)> {
    let w = width.checked_mul(factor)?;
    let h = height.checked_mul(factor)?;
    (w <= MAX_SIDE && h <= MAX_SIDE).then_some((w, h))
}

/// Whether a `width × height` px selection may be output at `factor`: 1×, always, or one of
/// [`FACTORS`] that stays within [`MAX_SIDE`].
pub fn allowed(width: u32, height: u32, factor: u32) -> bool {
    factor == 1 || (FACTORS.contains(&factor) && scaled(width, height, factor).is_some())
}

/// `len` zeroed bytes, or an error rather than an abort when there isn't the memory: the
/// largest output is 256 MiB, and the layer and the PNG come on top of it.
pub fn buffer(len: usize) -> Result<Vec<u8>, CaptureError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|e| CaptureError::Encode(format!("{len} bytes: {e}")))?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// `image` at `factor`, with Lanczos3. The factor must be [`allowed`] and above 1.
///
/// Alpha is not premultiplied: captures are opaque, and that skips two passes.
pub fn upscale(image: &RgbaImage, factor: u32) -> Result<RgbaImage, CaptureError> {
    let (w, h) = image.dimensions();
    if factor == 1 || !allowed(w, h, factor) {
        return Err(CaptureError::Invalid(format!("{w}x{h} px at {factor}x")));
    }
    let failed = |e: &dyn std::fmt::Display| CaptureError::Encode(format!("upscale: {e}"));
    let src = ImageRef::new(w, h, image.as_raw(), PixelType::U8x4).map_err(|e| failed(&e))?;
    let (ow, oh) = (w * factor, h * factor);
    // Checked: a `usize` may be 32 bits.
    let len = (ow as usize)
        .checked_mul(oh as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| failed(&"output too large"))?;
    let mut dst =
        Image::from_vec_u8(ow, oh, buffer(len)?, PixelType::U8x4).map_err(|e| failed(&e))?;
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
        .use_alpha(false);
    Resizer::new()
        .resize(&src, &mut dst, &options)
        .map_err(|e| failed(&e))?;
    RgbaImage::from_raw(ow, oh, dst.into_vec()).ok_or_else(|| failed(&"wrong buffer size"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn scales_both_sides_up_to_the_cap() {
        assert_eq!(scaled(1000, 700, 4), Some((4000, 2800)));
        assert_eq!(scaled(2048, 10, 4), Some((8192, 40)));
        assert_eq!(scaled(2049, 10, 4), None);
        assert_eq!(scaled(10, 2731, 3), None);
        assert_eq!(scaled(u32::MAX, 1, 2), None);
        assert_eq!(scaled(1, u32::MAX, 2), None);
    }

    #[test]
    fn allows_one_and_the_offered_factors_that_fit() {
        assert!(allowed(1000, 700, 1));
        assert!(allowed(9000, 9000, 1));
        assert!(allowed(1000, 700, 2));
        assert!(allowed(1000, 700, 4));
        // A full Retina screen allows only 2×.
        assert!(allowed(3600, 2338, 2));
        assert!(!allowed(3600, 2338, 3));
        // Not offered.
        assert!(!allowed(10, 10, 0));
        assert!(!allowed(10, 10, 5));
        assert!(!allowed(10, 10, 8));
    }

    #[test]
    fn upscales_keeping_the_aspect_ratio() {
        let image = RgbaImage::from_fn(5, 3, |x, y| Rgba([x as u8 * 50, y as u8 * 80, 9, 255]));
        let out = upscale(&image, 3).unwrap();
        assert_eq!(out.dimensions(), (15, 9));
        // Flat areas stay flat and opaque: no ringing on a solid image.
        let flat = RgbaImage::from_pixel(4, 4, Rgba([10, 200, 30, 255]));
        let out = upscale(&flat, 4).unwrap();
        assert_eq!(out.dimensions(), (16, 16));
        assert!(out.pixels().all(|p| p.0 == [10, 200, 30, 255]));
    }

    #[test]
    fn refuses_a_factor_that_is_not_allowed() {
        let image = RgbaImage::new(3000, 1);
        assert!(matches!(upscale(&image, 3), Err(CaptureError::Invalid(_))));
        for factor in [1, 5] {
            let refused = upscale(&RgbaImage::new(2, 2), factor);
            assert!(matches!(refused, Err(CaptureError::Invalid(_))));
        }
    }

    #[test]
    fn a_buffer_too_large_for_memory_is_an_error_not_an_abort() {
        assert_eq!(buffer(8).unwrap(), [0; 8]);
        assert!(matches!(buffer(usize::MAX), Err(CaptureError::Encode(_))));
    }

    #[test]
    fn an_empty_image_stays_empty() {
        let out = upscale(&RgbaImage::new(0, 0), 2).unwrap();
        assert_eq!(out.dimensions(), (0, 0));
    }
}
