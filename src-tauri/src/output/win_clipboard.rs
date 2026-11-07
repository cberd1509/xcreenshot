//! The Windows clipboard: a PNG with its density (`pHYs`) as the registered "PNG" format, then a
//! `CF_DIBV5` with the same density for apps that only read bitmaps. Windows makes `CF_DIB` and
//! `CF_BITMAP` from the `CF_DIBV5` when an app asks.
//!
//! The clipboard plugin (arboard) writes both without a density, so a capture at 150% pasted
//! into Word came out 1.5× too large (#78). The bitmap follows arboard's otherwise: bottom-up
//! rows (Word cannot paste a negative height), BGRA with straight alpha and bit-field masks.
//!
//! Unlike macOS's TIFF, the bitmap is made on every copy: making it only when an app asks
//! would need the owner window to handle `WM_RENDERFORMAT`.
//!
//! Everything here is pure and tested: the formats, the order of the clipboard calls
//! ([`write`], over a [`System`]). [`os`] is the Win32 [`System`], Windows only.

use crate::capture::{CaptureError, Density, encode_png};
use image::RgbaImage;
use std::time::Duration;

/// The size of a `BITMAPV5HEADER`.
const HEADER_SIZE: usize = 124;

/// How often to try opening the clipboard, which another app may hold for a moment, and how
/// long to wait between tries, like Chromium and arboard.
const OPEN_ATTEMPTS: u32 = 10;
const OPEN_WAIT: Duration = Duration::from_millis(5);

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

/// A clipboard format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    DibV5,
}

/// The clipboard calls [`write`] makes, one each: [`os::Win32`] on Windows, a fake in tests.
pub trait System {
    /// System memory holding one format's bytes.
    type Block;
    fn alloc(&mut self, bytes: &[u8]) -> Result<Self::Block, CaptureError>;
    /// Frees a block the clipboard did not take.
    fn free(&mut self, block: Self::Block);
    /// One try at opening the clipboard.
    fn open(&mut self) -> Result<(), CaptureError>;
    /// Waits before the next try at opening.
    fn wait(&mut self, time: Duration);
    /// Removes what is on the clipboard and makes this app its owner.
    fn empty(&mut self) -> Result<(), CaptureError>;
    /// Puts `block` on the clipboard as `format`. On success the clipboard owns it; on failure
    /// it comes back.
    fn set(
        &mut self,
        format: Format,
        block: Self::Block,
    ) -> Result<(), (Self::Block, CaptureError)>;
    fn close(&mut self);
}

/// Replaces the clipboard's contents with `contents`.
///
/// Both blocks are made before the clipboard opens, so other apps wait as little as possible
/// and a failure to make them leaves the clipboard alone. Opening is tried
/// [`OPEN_ATTEMPTS`] times; if it fails the old contents stay. Only after that is the clipboard
/// emptied, and then a copy is lost only if the OS refuses both formats. One format is enough
/// to paste, so a copy with either counts as done (the other failure is logged).
pub fn write<S: System>(system: &mut S, contents: &Contents) -> Result<(), CaptureError> {
    let png = system.alloc(&contents.png)?;
    let dib = match system.alloc(&contents.dib) {
        Ok(dib) => dib,
        Err(e) => {
            system.free(png);
            return Err(e);
        }
    };
    let emptied = open(system).and_then(|()| system.empty().inspect_err(|_| system.close()));
    if let Err(e) = emptied {
        system.free(png);
        system.free(dib);
        return Err(e);
    }
    let result = match (
        set(system, Format::Png, png),
        set(system, Format::DibV5, dib),
    ) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Err(_)) => Err(e),
        (Ok(()), Err(e)) | (Err(e), Ok(())) => {
            log::warn!("copied with one clipboard format only: {e}");
            Ok(())
        }
    };
    system.close();
    result
}

fn open<S: System>(system: &mut S) -> Result<(), CaptureError> {
    for _ in 1..OPEN_ATTEMPTS {
        if system.open().is_ok() {
            return Ok(());
        }
        system.wait(OPEN_WAIT);
    }
    system.open()
}

/// Sets `block`, freeing it if the clipboard did not take it.
fn set<S: System>(system: &mut S, format: Format, block: S::Block) -> Result<(), CaptureError> {
    system.set(format, block).map_err(|(block, e)| {
        system.free(block);
        e
    })
}

/// The Win32 [`System`]. Thin: each method is one call.
#[cfg(target_os = "windows")]
pub(crate) mod os {
    use super::{Format, System};
    use crate::capture::CaptureError;
    use std::sync::{Mutex, PoisonError, mpsc};
    use std::time::Duration;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_DIBV5;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
        WINDOW_STYLE,
    };
    use windows::core::{Error, w};

    fn error(what: &str, e: Error) -> CaptureError {
        CaptureError::Clipboard(format!("{what}: {e}"))
    }

    /// The clipboard, opened with the app's owner window.
    pub struct Win32 {
        owner: HWND,
        png: u32,
    }

    impl Win32 {
        pub fn new() -> Result<Self, CaptureError> {
            // SAFETY: a static, NUL-terminated name.
            let png = unsafe { RegisterClipboardFormatW(w!("PNG")) };
            if png == 0 {
                return Err(error("could not register PNG", Error::from_thread()));
            }
            Ok(Self {
                owner: owner()?,
                png,
            })
        }
    }

    /// The clipboard needs an owner window: opened without one, `EmptyClipboard` leaves it
    /// with no owner and `SetClipboardData` may fail. This is a hidden message-only window,
    /// made once on its own thread, which keeps it and handles its messages for the life of
    /// the app (a window goes away with the thread that made it).
    fn owner() -> Result<HWND, CaptureError> {
        static OWNER: Mutex<isize> = Mutex::new(0);
        let mut owner = OWNER.lock().unwrap_or_else(PoisonError::into_inner);
        if *owner == 0 {
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("clipboard owner".into())
                .spawn(move || {
                    // SAFETY: a message-only window of the system STATIC class.
                    let window = unsafe {
                        CreateWindowExW(
                            WINDOW_EX_STYLE(0),
                            w!("STATIC"),
                            w!("xcreenshot clipboard"),
                            WINDOW_STYLE(0),
                            0,
                            0,
                            0,
                            0,
                            Some(HWND_MESSAGE),
                            None,
                            None,
                            None,
                        )
                    };
                    let made = window.is_ok();
                    let _ = tx.send(window.map(|w| w.0 as isize));
                    let mut msg = MSG::default();
                    // SAFETY: this thread's own message queue. 0 is WM_QUIT, -1 an error.
                    while made && unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
                        // SAFETY: a message GetMessageW just filled in.
                        unsafe { DispatchMessageW(&msg) };
                    }
                })
                .map_err(|e| CaptureError::Clipboard(format!("no owner thread: {e}")))?;
            *owner = rx
                .recv()
                .map_err(|e| CaptureError::Clipboard(format!("no owner window: {e}")))?
                .map_err(|e| error("no owner window", e))?;
        }
        Ok(HWND(*owner as *mut core::ffi::c_void))
    }

    impl System for Win32 {
        type Block = HGLOBAL;

        fn alloc(&mut self, bytes: &[u8]) -> Result<HGLOBAL, CaptureError> {
            // SAFETY: a new block of `bytes.len()` bytes, filled while locked, or freed.
            unsafe {
                let block = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
                    .map_err(|e| error("out of memory", e))?;
                let at = GlobalLock(block);
                if at.is_null() {
                    let e = Error::from_thread();
                    let _ = GlobalFree(Some(block));
                    return Err(error("could not lock memory", e));
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), at.cast::<u8>(), bytes.len());
                // Returns an error once unlocked, which is the expected result.
                let _ = GlobalUnlock(block);
                Ok(block)
            }
        }

        fn free(&mut self, block: HGLOBAL) {
            // SAFETY: a block from `alloc` that the clipboard did not take.
            let _ = unsafe { GlobalFree(Some(block)) };
        }

        fn open(&mut self) -> Result<(), CaptureError> {
            // SAFETY: the owner window lives as long as the app.
            unsafe { OpenClipboard(Some(self.owner)) }
                .map_err(|e| error("the clipboard is busy", e))
        }

        fn wait(&mut self, time: Duration) {
            std::thread::sleep(time);
        }

        fn empty(&mut self) -> Result<(), CaptureError> {
            // SAFETY: the clipboard is open on this thread.
            unsafe { EmptyClipboard() }.map_err(|e| error("could not empty it", e))
        }

        fn set(&mut self, format: Format, block: HGLOBAL) -> Result<(), (HGLOBAL, CaptureError)> {
            let id = match format {
                Format::Png => self.png,
                Format::DibV5 => u32::from(CF_DIBV5.0),
            };
            // SAFETY: the clipboard is open on this thread and `block` is unlocked.
            unsafe { SetClipboardData(id, Some(HANDLE(block.0))) }
                .map(|_| ())
                .map_err(|e| (block, error("the clipboard refused the image", e)))
        }

        fn close(&mut self) {
            // SAFETY: this thread opened the clipboard.
            let _ = unsafe { CloseClipboard() };
        }
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

    /// A clipboard that records each call. Blocks are numbered by `alloc`: the PNG is 0 and
    /// the bitmap 1.
    #[derive(Default)]
    struct Fake {
        calls: Vec<&'static str>,
        /// Fails this many opens before one works.
        busy: u32,
        fail_alloc: Option<usize>,
        fail_empty: bool,
        refuse: Vec<Format>,
        made: usize,
        freed: Vec<usize>,
        taken: Vec<(Format, usize)>,
    }

    fn failed(what: &str) -> CaptureError {
        CaptureError::Clipboard(what.into())
    }

    impl System for Fake {
        type Block = usize;
        fn alloc(&mut self, _bytes: &[u8]) -> Result<usize, CaptureError> {
            if self.fail_alloc == Some(self.made) {
                return Err(failed("alloc"));
            }
            self.made += 1;
            Ok(self.made - 1)
        }
        fn free(&mut self, block: usize) {
            self.freed.push(block);
        }
        fn open(&mut self) -> Result<(), CaptureError> {
            self.calls.push("open");
            if self.busy > 0 {
                self.busy -= 1;
                return Err(failed("busy"));
            }
            Ok(())
        }
        fn wait(&mut self, time: Duration) {
            assert_eq!(time, OPEN_WAIT);
            self.calls.push("wait");
        }
        fn empty(&mut self) -> Result<(), CaptureError> {
            self.calls.push("empty");
            if self.fail_empty {
                return Err(failed("empty"));
            }
            Ok(())
        }
        fn set(&mut self, format: Format, block: usize) -> Result<(), (usize, CaptureError)> {
            self.calls.push(match format {
                Format::Png => "set Png",
                Format::DibV5 => "set DibV5",
            });
            if self.refuse.contains(&format) {
                return Err((block, failed("refused")));
            }
            self.taken.push((format, block));
            Ok(())
        }
        fn close(&mut self) {
            self.calls.push("close");
        }
    }

    impl Fake {
        fn write(&mut self) -> Result<(), CaptureError> {
            let contents = Contents {
                png: vec![1],
                dib: vec![2],
            };
            let result = write(self, &contents);
            // Every block made is either on the clipboard or freed, once.
            let mut blocks: Vec<_> = self.taken.iter().map(|&(_, b)| b).collect();
            blocks.extend(&self.freed);
            blocks.sort();
            assert_eq!(blocks, (0..self.made).collect::<Vec<_>>());
            result
        }
    }

    #[test]
    fn writes_the_png_then_the_bitmap_and_frees_nothing_the_clipboard_took() {
        let mut fake = Fake::default();
        fake.write().unwrap();
        assert_eq!(
            fake.calls,
            ["open", "empty", "set Png", "set DibV5", "close"]
        );
        assert_eq!(fake.taken, [(Format::Png, 0), (Format::DibV5, 1)]);
    }

    #[test]
    fn waits_for_a_busy_clipboard_then_gives_up_without_emptying_it() {
        let mut fake = Fake {
            busy: OPEN_ATTEMPTS - 1,
            ..Fake::default()
        };
        fake.write().unwrap();
        let opens = fake.calls.iter().filter(|&&c| c == "open").count();
        let waits = fake.calls.iter().filter(|&&c| c == "wait").count();
        assert_eq!(
            (opens, waits),
            (OPEN_ATTEMPTS as usize, OPEN_ATTEMPTS as usize - 1)
        );

        let mut fake = Fake {
            busy: OPEN_ATTEMPTS,
            ..Fake::default()
        };
        assert_eq!(fake.write(), Err(failed("busy")));
        assert!(!fake.calls.iter().any(|&c| c == "empty" || c == "close"));
        assert_eq!(fake.freed, [0, 1]);
    }

    #[test]
    fn a_failed_allocation_never_opens_the_clipboard() {
        for at in [0, 1] {
            let mut fake = Fake {
                fail_alloc: Some(at),
                ..Fake::default()
            };
            assert_eq!(fake.write(), Err(failed("alloc")));
            assert!(fake.calls.is_empty(), "{at}");
        }
    }

    #[test]
    fn a_failed_empty_frees_both_and_closes() {
        let mut fake = Fake {
            fail_empty: true,
            ..Fake::default()
        };
        assert_eq!(fake.write(), Err(failed("empty")));
        assert_eq!(fake.calls.last().copied(), Some("close"));
        assert_eq!(fake.freed, [0, 1]);
    }

    #[test]
    fn one_format_is_enough_and_the_refused_one_is_freed() {
        for (refused, kept) in [(Format::DibV5, Format::Png), (Format::Png, Format::DibV5)] {
            let mut fake = Fake {
                refuse: vec![refused],
                ..Fake::default()
            };
            fake.write().unwrap();
            assert_eq!(fake.taken.len(), 1);
            assert_eq!(fake.taken[0].0, kept);
            assert_eq!(fake.freed.len(), 1);
            assert_eq!(fake.calls.last().copied(), Some("close"));
        }
    }

    #[test]
    fn both_formats_refused_fails() {
        let mut fake = Fake {
            refuse: vec![Format::Png, Format::DibV5],
            ..Fake::default()
        };
        assert_eq!(fake.write(), Err(failed("refused")));
        assert_eq!(fake.freed, [0, 1]);
        assert_eq!(fake.calls.last().copied(), Some("close"));
    }
}
