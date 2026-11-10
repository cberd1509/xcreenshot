//! Copy and save: the default file name and folder, the `.png` extension, writing the file and
//! the system clipboard.
//!
//! The overlay's commands (`overlay/export.rs`) wire these to the Save dialog and the prefs.

/// Annotations on the copied or saved image.
pub mod compose;
/// Copies reach the clipboard in the order they were asked for.
pub(crate) mod order;
/// The Save dialog on macOS.
#[cfg(any(target_os = "macos", test))]
pub(crate) mod save_panel;
/// Copy and Save at 2×, 3× and 4×.
pub mod upscale;
/// The Windows clipboard.
#[cfg(any(target_os = "windows", test))]
pub(crate) mod win_clipboard;

use crate::capture::{CaptureError, Density};
use chrono::NaiveDateTime;
use image::RgbaImage;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Runtime};

/// The Save dialog's default file name, from the local time:
/// `xcreenshot-2026-11-06-13.22.05.png`.
pub fn file_name(now: NaiveDateTime) -> String {
    now.format("xcreenshot-%Y-%m-%d-%H.%M.%S.png").to_string()
}

/// Where a path picked in the Save dialog goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    /// Write here.
    Write(PathBuf),
    /// `.png` had to be added and that file exists. The dialog never asked about overwriting
    /// it, so ask again with this name.
    Ask(PathBuf),
}

/// The dialog's PNG filter adds `.png` where the platform supports it. Otherwise it is added
/// here (`shot` and `shot.jpg` become `shot.png` and `shot.jpg.png`), but only to a name that
/// is free: we never write to a file the user did not see.
pub fn png_target(path: PathBuf, exists: impl Fn(&Path) -> bool) -> Target {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        return Target::Write(path);
    }
    let mut name = OsString::from(path);
    name.push(".png");
    let path = PathBuf::from(name);
    if exists(&path) {
        Target::Ask(path)
    } else {
        Target::Write(path)
    }
}

/// The folder the Save dialog opens in: the last one picked if it still exists, otherwise the
/// platform default (see [`default_dir`]).
pub fn save_dir(
    last: Option<PathBuf>,
    default: Option<PathBuf>,
    is_dir: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    last.filter(|d| is_dir(d)).or(default)
}

/// The first default folder: the Desktop on macOS, where its own screenshots go, and Pictures
/// on Windows, next to the Snipping Tool's Screenshots folder.
pub fn default_dir(desktop: Option<PathBuf>, pictures: Option<PathBuf>) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        desktop.or(pictures)
    } else {
        pictures.or(desktop)
    }
}

/// Writes `bytes` as the whole of `path`: to a new temp file in the same folder, renamed over
/// the target. A replaced file never keeps old bytes after the new ones (aCropalypse,
/// CVE-2023-28303), and a failed write leaves no half-written file.
///
/// A symlink is followed and the file it points to is replaced. A replaced file keeps its
/// permissions.
pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = if path.is_symlink() {
        fs::canonicalize(path)?
    } else {
        path.to_owned()
    };
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        Some(_) => Path::new("."),
        None => return Err(io::Error::new(io::ErrorKind::InvalidInput, "no file name")),
    };
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(bytes)?;
    if let Ok(existing) = fs::metadata(&target) {
        fs::set_permissions(temp.path(), existing.permissions())?;
    }
    temp.as_file().sync_all()?;
    // On failure the temp file is dropped, which deletes it.
    temp.persist(&target).map_err(|e| e.error)?;
    // Make the rename durable. The file is complete either way, so a failure is only logged.
    #[cfg(unix)]
    if let Err(e) = fs::File::open(dir).and_then(|d| d.sync_all()) {
        log::warn!(
            "saved {}, but could not sync its folder: {e}",
            target.display()
        );
    }
    Ok(())
}

/// Puts an image at its pixel density on the system clipboard. Called off the main thread (it
/// may block on it).
pub type Clipboard = Box<dyn Fn(&RgbaImage, Density) -> Result<(), CaptureError> + Send + Sync>;

/// The real clipboard. macOS: a PNG with its density, plus TIFF on request (`macos::write`),
/// written on the main thread. Windows: a PNG and a bitmap, both with the density
/// (`win_clipboard`). Elsewhere: the clipboard plugin, without a density.
///
/// The copy is numbered now, when it is asked for, so an older copy never replaces a newer one
/// (`order`).
pub fn os_clipboard<R: Runtime>(app: &AppHandle<R>) -> Clipboard {
    static ORDER: order::CopyOrder = order::CopyOrder::new();
    order::ordered(&ORDER, platform_clipboard(app))
}

fn platform_clipboard<R: Runtime>(app: &AppHandle<R>) -> Clipboard {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        png_clipboard(move |png| {
            let (tx, rx) = std::sync::mpsc::channel();
            app.run_on_main_thread(move || {
                let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
                let _ = tx.send(macos::write(&pasteboard, png));
            })
            .map_err(|e| CaptureError::Clipboard(e.to_string()))?;
            rx.recv()
                .map_err(|e| CaptureError::Clipboard(e.to_string()))?
        })
    }
    #[cfg(target_os = "windows")]
    {
        // Written from this thread: no app handle needed.
        let _ = app;
        Box::new(|image, density| {
            let contents = win_clipboard::contents(image, density)?;
            win_clipboard::write(&mut win_clipboard::os::Win32::new()?, &contents)
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let app = app.clone();
        Box::new(move |image, _density| {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            let image = tauri::image::Image::new(image.as_raw(), image.width(), image.height());
            app.clipboard()
                .write_image(&image)
                .map_err(|e| CaptureError::Clipboard(e.to_string()))
        })
    }
}

/// A clipboard that encodes the image at its density, off the main thread, and hands `put` the
/// PNG. Tests give it a private pasteboard.
#[cfg(target_os = "macos")]
pub(crate) fn png_clipboard(
    put: impl Fn(Vec<u8>) -> Result<(), CaptureError> + Send + Sync + 'static,
) -> Clipboard {
    Box::new(move |image, density| put(crate::capture::encode_png(image, density)?))
}

/// The macOS pasteboard: the PNG as `public.png`, and TIFF for apps that only read that. TIFF
/// is made only if an app asks for it (an `NSPasteboardItem` data provider), so copying costs
/// the PNG alone. Main thread only.
#[cfg(target_os = "macos")]
pub(crate) mod macos {
    use crate::capture::CaptureError;
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
    use objc2_app_kit::{
        NSBitmapImageRep, NSPasteboard, NSPasteboardItem, NSPasteboardItemDataProvider,
        NSPasteboardType, NSPasteboardTypePNG, NSPasteboardTypeTIFF, NSPasteboardWriting,
    };
    use objc2_foundation::{NSArray, NSData};
    use std::cell::RefCell;

    define_class!(
        /// Turns the copied PNG into TIFF when an app asks for TIFF.
        #[unsafe(super(NSObject))]
        #[name = "XcreenshotTiffProvider"]
        #[ivars = Retained<NSData>]
        struct TiffProvider;

        unsafe impl NSObjectProtocol for TiffProvider {}

        unsafe impl NSPasteboardItemDataProvider for TiffProvider {
            #[unsafe(method(pasteboard:item:provideDataForType:))]
            fn provide(
                &self,
                _pasteboard: Option<&NSPasteboard>,
                item: &NSPasteboardItem,
                data_type: &NSPasteboardType,
            ) {
                let tiff = NSBitmapImageRep::imageRepWithData(self.ivars())
                    .and_then(|rep| rep.TIFFRepresentation());
                match tiff {
                    Some(tiff) => {
                        item.setData_forType(&tiff, data_type);
                    }
                    None => log::warn!("could not make TIFF from the copied PNG"),
                }
            }

            /// The pasteboard has new contents: drop the provider and its PNG.
            #[unsafe(method(pasteboardFinishedWithDataProvider:))]
            fn finished(&self, _pasteboard: &NSPasteboard) {
                PROVIDER.with(|p| {
                    let mut p = p.borrow_mut();
                    if p.as_deref().is_some_and(|kept| std::ptr::eq(kept, self))
                        && let Some(kept) = p.take()
                    {
                        // Released once this call returns, not while AppKit is calling it.
                        let _ = Retained::autorelease_ptr(kept);
                    }
                });
            }
        }
    );

    impl TiffProvider {
        fn new(png: Retained<NSData>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(png);
            // SAFETY: `NSObject`'s designated initializer.
            unsafe { msg_send![super(this), init] }
        }
    }

    thread_local! {
        /// The provider for what is on the pasteboard now, kept alive until the pasteboard is
        /// done with it.
        static PROVIDER: RefCell<Option<Retained<TiffProvider>>> = const { RefCell::new(None) };
    }

    /// Replaces the contents of `pasteboard` with `png`. Everything is prepared first, so a
    /// failure before the write leaves the old contents alone.
    pub fn write(pasteboard: &NSPasteboard, png: Vec<u8>) -> Result<(), CaptureError> {
        let refused =
            |what: &str| CaptureError::Clipboard(format!("the pasteboard refused {what}"));
        // SAFETY: AppKit constants, valid for the life of the process.
        let (png_type, tiff_type) = unsafe { (NSPasteboardTypePNG, NSPasteboardTypeTIFF) };
        let data = NSData::from_vec(png);
        let item = NSPasteboardItem::new();
        if !item.setData_forType(&data, png_type) {
            return Err(refused("the PNG"));
        }
        let provider = TiffProvider::new(data);
        let types = NSArray::from_slice(&[tiff_type]);
        if !item.setDataProvider_forTypes(ProtocolObject::from_ref(&*provider), &types) {
            return Err(refused("the TIFF provider"));
        }
        let items = NSArray::from_retained_slice(&[
            ProtocolObject::<dyn NSPasteboardWriting>::from_retained(item),
        ]);
        // AppKit needs the clear before the write. The item is complete by now, so only the
        // OS refusing the write itself loses the old contents.
        pasteboard.clearContents();
        if !pasteboard.writeObjects(&items) {
            return Err(refused("the image"));
        }
        PROVIDER.with(|p| p.replace(Some(provider)));
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::capture::{Density, encode_png};
        use image::RgbaImage;

        #[test]
        fn writes_the_png_and_makes_tiff_on_request() {
            // A private pasteboard: the user's clipboard is left alone.
            let pasteboard = NSPasteboard::pasteboardWithUniqueName();
            let image = RgbaImage::from_fn(7, 3, |x, y| image::Rgba([x as u8, y as u8, 1, 255]));
            let density = Density::new(2.0, 1.5);
            let png = encode_png(&image, density).unwrap();
            write(&pasteboard, png.clone()).unwrap();
            let (png_type, tiff_type) = unsafe { (NSPasteboardTypePNG, NSPasteboardTypeTIFF) };
            let copied = pasteboard.dataForType(png_type).unwrap();
            assert_eq!(copied.to_vec(), png, "exactly the encoded PNG");
            crate::output::tests::assert_clean_png(&copied.to_vec(), &image, density);
            let tiff = pasteboard.dataForType(tiff_type).expect("TIFF on request");
            let rep = NSBitmapImageRep::imageRepWithData(&tiff).unwrap();
            assert_eq!((rep.pixelsWide(), rep.pixelsHigh()), (7, 3));
            // The TIFF keeps the density: its size in points is the on-screen size.
            let size = rep.size();
            assert!((size.width - 3.5).abs() < 0.01, "{size:?}");
            assert!((size.height - 2.0).abs() < 0.01, "{size:?}");
        }

        #[test]
        fn the_png_is_released_when_the_pasteboard_moves_on() {
            let pasteboard = NSPasteboard::pasteboardWithUniqueName();
            let png = encode_png(&RgbaImage::new(2, 2), Density::ONE).unwrap();
            objc2::rc::autoreleasepool(|_| {
                write(&pasteboard, png).unwrap();
                assert!(PROVIDER.with(|p| p.borrow().is_some()));
                // Another app copies something.
                pasteboard.clearContents();
                assert!(PROVIDER.with(|p| p.borrow().is_none()));
            });
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::capture::encode_png;
    use chrono::NaiveDate;

    /// The chunk types of a PNG, checking that the chunks fill the file exactly up to IEND
    /// with nothing after it.
    pub(crate) fn png_chunks(png: &[u8]) -> Vec<String> {
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");
        let mut at = 8;
        let mut chunks = Vec::new();
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            chunks.push(String::from_utf8(png[at + 4..at + 8].to_vec()).unwrap());
            // Length, type, data, CRC.
            at += 12 + len;
        }
        assert_eq!(at, png.len(), "chunks end exactly at the end of the file");
        assert_eq!(chunks.last().map(String::as_str), Some("IEND"));
        chunks
    }

    /// Exactly IHDR, pHYs, IDAT and IEND (no text, EXIF or anything else), with `density` in
    /// pHYs, decoding to exactly `image`. The encoder splits IDAT only past 2 GiB.
    pub(crate) fn assert_clean_png(png: &[u8], image: &RgbaImage, density: Density) {
        assert_eq!(png_chunks(png), ["IHDR", "pHYs", "IDAT", "IEND"]);
        let reader = png::Decoder::new(io::Cursor::new(png)).read_info().unwrap();
        let dims = reader.info().pixel_dims.unwrap();
        assert_eq!((dims.xppu, dims.yppu), density.pixels_per_metre());
        assert_eq!(dims.unit, png::Unit::Meter);
        let decoded = image::load_from_memory(png).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), image.dimensions());
        assert_eq!(&decoded, image);
    }

    pub(crate) fn sample(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 13) as u8, (x ^ y) as u8, 255])
        })
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn encoded_pngs_carry_only_the_pixels_and_density() {
        let image = sample(33, 17);
        let d = Density::new;
        for density in [Density::ONE, d(2.0, 2.0), d(1.5, 1.5), d(1.25, 2.0)] {
            assert_clean_png(&encode_png(&image, density).unwrap(), &image, density);
        }
    }

    #[test]
    fn saving_over_a_larger_file_leaves_only_the_new_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        fs::write(&path, encode_png(&sample(200, 200), Density::ONE).unwrap()).unwrap();
        let small = sample(10, 5);
        let png = encode_png(&small, Density::ONE).unwrap();
        write_file(&path, &png).unwrap();
        let saved = fs::read(&path).unwrap();
        assert_eq!(saved.len(), png.len());
        assert_eq!(saved, png, "exactly the new bytes, nothing after IEND");
        assert_clean_png(&saved, &small, Density::ONE);
        assert_eq!(names(dir.path()), ["shot.png"], "no temp file left");
    }

    #[test]
    fn a_failed_write_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-folder").join("shot.png");
        assert!(write_file(&missing, b"png").is_err());
        assert!(!missing.exists());
        // A folder in the way of the rename: the temp file is removed.
        let blocked = dir.path().join("taken.png");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("inside"), b"x").unwrap();
        assert!(write_file(&blocked, b"png").is_err());
        assert_eq!(names(dir.path()), ["taken.png"], "the temp file is removed");
        assert!(write_file(Path::new("/"), b"png").is_err(), "no file name");
    }

    #[test]
    fn a_relative_path_writes_in_the_current_folder() {
        // `Path::parent` of a bare name is empty; it must mean the current folder.
        let name = format!("xcreenshot-relative-{}.png", std::process::id());
        write_file(Path::new(&name), b"png").unwrap();
        assert_eq!(fs::read(&name).unwrap(), b"png");
        fs::remove_file(&name).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacing_keeps_permissions_and_follows_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("synced");
        fs::create_dir(&real_dir).unwrap();
        let real = real_dir.join("real.png");
        fs::write(&real, vec![1; 500]).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("link.png");
        symlink(&real, &link).unwrap();

        write_file(&link, b"new").unwrap();
        assert!(link.is_symlink(), "the link stays a link");
        assert_eq!(fs::read(&real).unwrap(), b"new");
        let mode = fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
        assert_eq!(
            names(&real_dir),
            ["real.png"],
            "the temp file was next to the real file"
        );
    }

    #[test]
    fn file_name_uses_the_local_time() {
        let now = NaiveDate::from_ymd_opt(2026, 11, 6)
            .unwrap()
            .and_hms_opt(13, 22, 5)
            .unwrap();
        assert_eq!(file_name(now), "xcreenshot-2026-11-06-13.22.05.png");
        let early = NaiveDate::from_ymd_opt(2027, 1, 2)
            .unwrap()
            .and_hms_opt(3, 4, 0)
            .unwrap();
        assert_eq!(file_name(early), "xcreenshot-2027-01-02-03.04.00.png");
    }

    fn target(path: &str, taken: &[&str]) -> Target {
        png_target(PathBuf::from(path), |p| {
            taken.iter().any(|t| Path::new(t) == p)
        })
    }

    fn write(path: &str) -> Target {
        Target::Write(PathBuf::from(path))
    }

    #[test]
    fn keeps_a_png_extension_in_any_case() {
        for path in ["/a/shot.png", "/a/shot.PNG", "/a/x.y.Png"] {
            assert_eq!(target(path, &[path]), write(path), "{path}");
        }
    }

    #[test]
    fn adds_png_when_missing() {
        assert_eq!(target("/a/shot", &[]), write("/a/shot.png"));
        assert_eq!(target("/a/shot.jpg", &[]), write("/a/shot.jpg.png"));
        assert_eq!(target("/a/v1.2", &[]), write("/a/v1.2.png"));
        assert_eq!(target("/a/shot.", &[]), write("/a/shot..png"));
    }

    #[test]
    fn asks_again_rather_than_overwrite_a_file_the_user_did_not_see() {
        assert_eq!(
            target("/a/shot", &["/a/shot.png"]),
            Target::Ask(PathBuf::from("/a/shot.png"))
        );
    }

    #[test]
    fn opens_in_the_last_folder_while_it_exists() {
        let last = Some(PathBuf::from("/last"));
        let default = Some(PathBuf::from("/default"));
        assert_eq!(save_dir(last.clone(), default.clone(), |_| true), last);
        assert_eq!(save_dir(last, default.clone(), |_| false), default);
        assert_eq!(save_dir(None, default.clone(), |_| true), default);
        assert_eq!(save_dir(None, None, |_| true), None);
    }

    #[test]
    fn default_folder_follows_the_platform() {
        let desktop = Some(PathBuf::from("/Desktop"));
        let pictures = Some(PathBuf::from("/Pictures"));
        let expected = if cfg!(target_os = "macos") {
            &desktop
        } else {
            &pictures
        };
        assert_eq!(&default_dir(desktop.clone(), pictures.clone()), expected);
        // Either one when the other is missing.
        assert_eq!(default_dir(None, pictures.clone()), pictures);
        assert_eq!(default_dir(desktop.clone(), None), desktop);
        assert_eq!(default_dir(None, None), None);
    }
}
