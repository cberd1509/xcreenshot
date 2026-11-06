//! What a copy puts on the Windows clipboard, pure: a PNG with its density (`pHYs`) as the
//! registered "PNG" format, then a `CF_DIBV5` with the same density for apps that only read
//! bitmaps. Windows makes `CF_DIB` and `CF_BITMAP` from the `CF_DIBV5` when an app asks.
//!
//! The clipboard plugin (arboard) writes both without a density, so a capture at 150% pasted
//! into Word came out 1.5× too large (#78). The bitmap follows arboard's otherwise: bottom-up
//! rows (Word cannot paste a negative height), BGRA with straight alpha and bit-field masks.
//!
//! Unlike macOS's TIFF, the bitmap is made on every copy: making it only when an app asks
//! would need a clipboard owner window handling `WM_RENDERFORMAT`.
//!
//! The Win32 calls are in [`os`], Windows only.

use crate::capture::{CaptureError, Density, encode_png};
use image::RgbaImage;

/// The size of a `BITMAPV5HEADER`.
pub const HEADER_SIZE: usize = 124;

/// The formats for one copy.
pub struct Contents {
    /// The registered "PNG" format, which Office, browsers and most apps read. It goes on the
    /// clipboard first, since some apps take the first format they can read.
    pub png: Vec<u8>,
    /// `CF_DIBV5`: a `BITMAPV5HEADER` and its pixels.
    pub dib: Vec<u8>,
}

/// The formats for `image` at `density`. The bitmap is made while the PNG encodes, so a copy
/// waits for the PNG alone.
pub fn contents(image: &RgbaImage, density: Density) -> Result<Contents, CaptureError> {
    std::thread::scope(|s| {
        let dib = s.spawn(|| dibv5(image, density.pixels_per_metre()));
        let png = encode_png(image, density);
        let dib = dib.join().expect("making the bitmap does not panic");
        Ok(Contents {
            png: png?,
            dib: dib?,
        })
    })
}

/// A `CF_DIBV5` of `image`: the header, with `ppm` (pixels per metre per axis) in
/// `bV5XPelsPerMeter`/`bV5YPelsPerMeter`, then the rows bottom-up as BGRA.
pub fn dibv5(image: &RgbaImage, ppm: (u32, u32)) -> Result<Vec<u8>, CaptureError> {
    let too_large = || CaptureError::Encode("the image is too large for the clipboard".into());
    let (width, height) = image.dimensions();
    let width = i32::try_from(width).map_err(|_| too_large())?;
    let height = i32::try_from(height).map_err(|_| too_large())?;
    let pixels = image.as_raw().len();
    let size_image = u32::try_from(pixels).map_err(|_| too_large())?;
    // LONG fields. A density that large is not real; keep it as the most that fits.
    let pels = |ppm: u32| ppm.min(i32::MAX as u32).to_le_bytes();

    // Every field not set here is 0: no palette, no endpoints or gammas (ignored for sRGB), no
    // profile.
    let mut out = vec![0; HEADER_SIZE + pixels];
    let mut put = |at: usize, bytes: [u8; 4]| out[at..at + 4].copy_from_slice(&bytes);
    put(0, (HEADER_SIZE as u32).to_le_bytes());
    put(4, width.to_le_bytes());
    // Positive: bottom-up.
    put(8, height.to_le_bytes());
    // One plane, 32 bits per pixel.
    put(12, [1, 0, 32, 0]);
    put(16, BI_BITFIELDS.to_le_bytes());
    put(20, size_image.to_le_bytes());
    put(24, pels(ppm.0));
    put(28, pels(ppm.1));
    for (i, mask) in MASKS.into_iter().enumerate() {
        put(40 + 4 * i, mask.to_le_bytes());
    }
    put(56, LCS_SRGB.to_le_bytes());
    put(108, LCS_GM_IMAGES.to_le_bytes());

    let source = image.rows().rev().flatten();
    for (to, from) in out[HEADER_SIZE..]
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(source)
    {
        let [r, g, b, a] = from.0;
        *to = [b, g, r, a];
    }
    Ok(out)
}

const BI_BITFIELDS: u32 = 3;
/// Red, green, blue and alpha in each little-endian pixel: BGRA bytes.
const MASKS: [u32; 4] = [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000];
/// `'sRGB'`.
const LCS_SRGB: u32 = 0x7352_4742;
const LCS_GM_IMAGES: u32 = 4;

/// Writes the formats to the clipboard. Called off the main thread.
#[cfg(target_os = "windows")]
pub(crate) mod os {
    use super::Contents;
    use crate::capture::CaptureError;
    use std::time::Duration;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_DIBV5;
    use windows::core::{Error, w};

    /// Another app may hold the clipboard for a moment: try this often, 5 ms apart, like
    /// Chromium and arboard.
    const OPEN_ATTEMPTS: u32 = 10;

    fn error(what: &str, e: Error) -> CaptureError {
        CaptureError::Clipboard(format!("{what}: {e}"))
    }

    /// Movable global memory holding one format, freed when dropped unless the clipboard took
    /// it.
    struct Block(HGLOBAL);

    impl Block {
        fn new(bytes: &[u8]) -> Result<Self, CaptureError> {
            // SAFETY: a new block of `bytes.len()` bytes, filled while locked.
            unsafe {
                let block = Self(
                    GlobalAlloc(GMEM_MOVEABLE, bytes.len())
                        .map_err(|e| error("out of memory", e))?,
                );
                let at = GlobalLock(block.0);
                if at.is_null() {
                    return Err(error("could not lock memory", Error::from_thread()));
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), at.cast::<u8>(), bytes.len());
                // Returns an error once unlocked, which is the expected result.
                let _ = GlobalUnlock(block.0);
                Ok(block)
            }
        }

        /// Hands the block to the open clipboard as format `id`.
        fn set(self, id: u32) -> Result<(), CaptureError> {
            // SAFETY: the clipboard is open on this thread. On success the system owns the
            // block; on failure `self` is dropped and frees it.
            unsafe { SetClipboardData(id, Some(HANDLE(self.0.0))) }
                .map_err(|e| error("the clipboard refused the image", e))?;
            std::mem::forget(self);
            Ok(())
        }
    }

    impl Drop for Block {
        fn drop(&mut self) {
            // SAFETY: this block is ours and no longer locked.
            let _ = unsafe { GlobalFree(Some(self.0)) };
        }
    }

    /// The open clipboard, closed when dropped.
    struct Open;

    impl Open {
        fn new() -> Result<Self, CaptureError> {
            for _ in 1..OPEN_ATTEMPTS {
                // SAFETY: no owner window, as arboard does.
                if unsafe { OpenClipboard(None) }.is_ok() {
                    return Ok(Self);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            // SAFETY: as above; the last try reports its error.
            unsafe { OpenClipboard(None) }
                .map(|()| Self)
                .map_err(|e| error("the clipboard is busy", e))
        }
    }

    impl Drop for Open {
        fn drop(&mut self) {
            // SAFETY: this thread opened the clipboard.
            let _ = unsafe { CloseClipboard() };
        }
    }

    /// Replaces the clipboard's contents with `contents`. The memory is filled before the
    /// clipboard opens, so other apps wait as little as possible.
    pub fn write(contents: &Contents) -> Result<(), CaptureError> {
        // SAFETY: a static, NUL-terminated name.
        let png_id = unsafe { RegisterClipboardFormatW(w!("PNG")) };
        if png_id == 0 {
            return Err(error("could not register PNG", Error::from_thread()));
        }
        let png = Block::new(&contents.png)?;
        let dib = Block::new(&contents.dib)?;
        let _open = Open::new()?;
        // SAFETY: the clipboard is open on this thread.
        unsafe { EmptyClipboard() }.map_err(|e| error("could not empty it", e))?;
        png.set(png_id)?;
        dib.set(u32::from(CF_DIBV5.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::tests::{assert_clean_png, sample};

    fn u32_at(dib: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(dib[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn the_header_carries_the_size_format_and_density() {
        let image = sample(5, 3);
        let dib = dibv5(&image, (5669, 3780)).unwrap();
        assert_eq!(dib.len(), HEADER_SIZE + 5 * 3 * 4);
        assert_eq!(u32_at(&dib, 0), 124, "bV5Size");
        assert_eq!((u32_at(&dib, 4), u32_at(&dib, 8)), (5, 3), "bottom-up");
        assert_eq!(dib[12..16], [1, 0, 32, 0], "one plane, 32 bits");
        assert_eq!(u32_at(&dib, 16), BI_BITFIELDS);
        assert_eq!(u32_at(&dib, 20), 5 * 3 * 4, "bV5SizeImage");
        assert_eq!((u32_at(&dib, 24), u32_at(&dib, 28)), (5669, 3780));
        assert_eq!((u32_at(&dib, 32), u32_at(&dib, 36)), (0, 0), "no palette");
        let masks = [40, 44, 48, 52].map(|at| u32_at(&dib, at));
        assert_eq!(masks, [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000]);
        assert_eq!(u32_at(&dib, 56), LCS_SRGB);
        assert!(dib[60..108].iter().all(|&b| b == 0), "endpoints and gammas");
        assert_eq!(u32_at(&dib, 108), LCS_GM_IMAGES);
        assert!(dib[112..124].iter().all(|&b| b == 0), "no profile");
    }

    #[test]
    fn rows_go_bottom_up_as_bgra_with_straight_alpha() {
        let image = RgbaImage::from_fn(2, 3, |x, y| image::Rgba([10 * y as u8, x as u8, 200, 128]));
        let dib = dibv5(&image, (1, 1)).unwrap();
        let (pixels, rest) = dib[HEADER_SIZE..].as_chunks::<4>();
        assert!(rest.is_empty());
        assert_eq!(pixels.len(), 6);
        for (i, p) in pixels.iter().enumerate() {
            let (x, y) = (i as u32 % 2, 2 - i as u32 / 2);
            let [r, g, b, a] = image.get_pixel(x, y).0;
            assert_eq!(*p, [b, g, r, a], "pixel {x},{y}");
        }
    }

    #[test]
    fn an_empty_image_is_only_the_header() {
        for (w, h) in [(0, 0), (0, 4), (4, 0)] {
            let dib = dibv5(&RgbaImage::new(w, h), (1, 1)).unwrap();
            assert_eq!(dib.len(), HEADER_SIZE, "{w}x{h}");
        }
    }

    #[test]
    fn a_density_past_a_long_is_kept_as_the_most_that_fits() {
        let dib = dibv5(&sample(1, 1), (u32::MAX, 2835)).unwrap();
        assert_eq!(
            (u32_at(&dib, 24), u32_at(&dib, 28)),
            (i32::MAX as u32, 2835)
        );
    }

    #[test]
    fn both_formats_carry_the_density() {
        let image = sample(9, 4);
        for density in [
            Density::ONE,
            Density::new(1.5, 1.5),
            Density::new(2.0, 1.25),
        ] {
            let Contents { png, dib } = contents(&image, density).unwrap();
            assert_clean_png(&png, &image, density);
            let ppm = density.pixels_per_metre();
            assert_eq!((u32_at(&dib, 24), u32_at(&dib, 28)), ppm);
            assert_eq!(dib, dibv5(&image, ppm).unwrap());
        }
    }

    #[test]
    fn an_unencodable_image_fails() {
        assert!(contents(&RgbaImage::new(0, 0), Density::ONE).is_err());
    }
}
