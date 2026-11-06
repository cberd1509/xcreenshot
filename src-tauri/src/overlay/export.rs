//! Copy and save: the overlay's way out with the selection.
//!
//! Both crop the selection (native pixels on the calling window's display, as the size badge
//! showed it) from the original frame Rust kept, so the result is exactly the captured pixels.
//!
//! - **Copy** (Enter, ⌘C / Ctrl+C) crops and puts the image on the clipboard off the main
//!   thread, then closes the overlay.
//! - **Save** (⌘S / Ctrl+S) hides the overlay windows, which sit above the Save dialog, and
//!   releases Esc to the dialog. The session stays, and nothing can close it until the save
//!   ends. Once a path is picked the file is always written. Then the overlay closes; if the
//!   write failed, or the dialog was cancelled, the windows come back with the selection
//!   intact, so the user can try again, copy, change the selection or press Esc.
//!
//! On failure the command returns the error and the page says so; the overlay is never left
//! without a way out. The OS side is behind [`Clipboard`] and [`Saver`], faked in tests.

use super::display_id;
use super::window::{Overlay, close, close_if, hide, on_main, phase, reshow, sync_escape};
use crate::capture::{self, CaptureError, PixelRect, clamp_rect};
use crate::output::compose::{self, Layer, Marks};
use crate::output::{self, Clipboard, Target};
use crate::prefs;
use chrono::NaiveDateTime;
use image::{ImageBuffer, RgbaImage};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tauri::async_runtime::spawn_blocking;
use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};

/// The request header describing a copy or save. Must match `OUTPUT_HEADER` on the page.
pub const HEADER: &str = "x-xcreenshot-output";

/// Most blur rects one request may have. More is a broken or hostile page.
pub const MAX_BLURS: usize = 256;

/// The JSON in [`HEADER`]. `layer` is where the body's pixels go, relative to `rect`; it is
/// there only when the body has them.
#[derive(Debug, Deserialize)]
struct Head {
    session: u64,
    rect: PixelRect,
    #[serde(default)]
    layer: Option<PixelRect>,
    #[serde(default)]
    blurs: Vec<PixelRect>,
}

/// A copy or save request: the session, the selection in native px, and its annotations, whose
/// pixels are read in place from the request body.
#[derive(Debug, PartialEq)]
pub(crate) struct Output<'a> {
    pub session: u64,
    pub rect: PixelRect,
    pub marks: Marks<'a>,
}

fn invalid(e: impl std::fmt::Display) -> CaptureError {
    CaptureError::Invalid(e.to_string())
}

/// Reads a copy or save request: JSON in [`HEADER`], and the layer's straight-alpha RGBA as the
/// raw body (empty without a layer). See `docs/annotations.md`.
pub(crate) fn parse<'a>(
    body: &'a InvokeBody,
    headers: &HeaderMap,
) -> Result<Output<'a>, CaptureError> {
    let header = headers
        .get(HEADER)
        .ok_or_else(|| invalid(format!("missing {HEADER}")))?;
    let head: Head = serde_json::from_slice(header.as_bytes()).map_err(invalid)?;
    if head.blurs.len() > MAX_BLURS {
        return Err(invalid(format!(
            "{} blurs, at most {MAX_BLURS}",
            head.blurs.len()
        )));
    }
    // Each blur clipped to the selection; one outside it is dropped.
    let (w, h) = (head.rect.width, head.rect.height);
    let blurs = head
        .blurs
        .into_iter()
        .filter_map(|b| clamp_rect(b, w, h))
        .collect();
    let bytes: &[u8] = match body {
        InvokeBody::Raw(bytes) => bytes,
        // An empty body may arrive as JSON.
        InvokeBody::Json(_) => &[],
    };
    let layer = head
        .layer
        .map(|at| {
            let size = format!(
                "{} bytes for a {}x{} layer",
                bytes.len(),
                at.width,
                at.height
            );
            let pixels =
                ImageBuffer::from_raw(at.width, at.height, bytes).ok_or_else(|| invalid(size))?;
            Ok::<_, CaptureError>(Layer {
                x: at.x,
                y: at.y,
                pixels,
            })
        })
        .transpose()?;
    Ok(Output {
        session: head.session,
        rect: head.rect,
        marks: Marks { layer, blurs },
    })
}

/// Copies the selection, with its annotations, to the clipboard and closes the overlay. An
/// empty rect is an error and closes nothing. The request is raw: see [`compose`].
#[tauri::command]
pub(crate) async fn overlay_copy<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, Overlay>,
    request: Request<'_>,
) -> Result<(), CaptureError> {
    let out = parse(request.body(), request.headers())?;
    let clipboard = output::os_clipboard(window.app_handle());
    copy_with(&window, &state, &out, clipboard).await
}

async fn copy_with<R: Runtime>(
    window: &WebviewWindow<R>,
    state: &Overlay,
    out: &Output<'_>,
    clipboard: Clipboard,
) -> Result<(), CaptureError> {
    let started = Instant::now();
    let session = out.session;
    let Some((_, frame, saves)) = selected_frame(window, state, session) else {
        return Ok(());
    };
    // Cropped and annotated here: the annotations are read in place from the request.
    let image = annotated(&frame, out)?;
    drop(frame);
    let copied = spawn_blocking(move || {
        clipboard(&image)?;
        Ok(image.dimensions())
    })
    .await
    .unwrap_or_else(|e| Err(CaptureError::Clipboard(e.to_string())));
    let (width, height) = copied.inspect_err(|e| log::warn!("copy failed: {e}"))?;
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    log::info!("copied {width}x{height} px in {ms:.0} ms");
    // Only if nothing happened meanwhile: a save started (even cancelled) keeps the overlay.
    on_main(window.app_handle(), move |app| {
        close_if(app, |phase| phase.copy_may_close(session, saves));
    });
    Ok(())
}

/// The calling window's display, its original frame and the session's save count, or `None`
/// if the session or window is not an open overlay, or the Save dialog is open.
fn selected_frame<R: Runtime>(
    window: &WebviewWindow<R>,
    state: &Overlay,
    session: u64,
) -> Option<(u32, Arc<RgbaImage>, u32)> {
    let display = display_id(window.label())?;
    let mut phase = state.phase();
    let saves = phase.saves(session)?;
    Some((display, phase.image(session, display)?, saves))
}

/// The selection cropped from the frame, with its annotations on it.
fn annotated(frame: &RgbaImage, out: &Output) -> Result<RgbaImage, CaptureError> {
    let mut image = capture::crop_pixels(frame, out.rect)?;
    compose::apply(&mut image, &out.marks);
    Ok(image)
}

/// Asks where to save the selection, with its annotations, writes it and closes the overlay.
/// Returns once that is done: `Ok` when saved or cancelled, the error if the file could not be
/// written. The request is raw: see [`compose`].
#[tauri::command]
pub(crate) async fn overlay_save<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, Overlay>,
    request: Request<'_>,
) -> Result<(), CaptureError> {
    let out = parse(request.body(), request.headers())?;
    let app = window.app_handle().clone();
    let paths = app.path();
    let default_dir = output::default_dir(paths.desktop_dir().ok(), paths.picture_dir().ok());
    let now = chrono::Local::now().naive_local();
    let saver = OsSaver(app);
    save_with(&window, &state, &out, saver, now, default_dir).await
}

async fn save_with<R: Runtime>(
    window: &WebviewWindow<R>,
    state: &Overlay,
    out: &Output<'_>,
    saver: impl Saver,
    now: NaiveDateTime,
    default_dir: Option<PathBuf>,
) -> Result<(), CaptureError> {
    let session = out.session;
    let Some((display, frame, _)) = selected_frame(window, state, session) else {
        return Ok(());
    };
    // Checked before anything hides, so an empty selection just stays.
    let image = annotated(&frame, out)?;
    drop(frame);
    if !state.phase().begin_save(session) {
        return Ok(());
    }
    let app = window.app_handle().clone();
    // Queued before the dialog, which opens from the main thread too.
    on_main(&app, |app| {
        hide(app);
        // If Esc cannot be released the dialog still opens; `escape` ignores Esc while saving.
        if !sync_escape(app) {
            log::warn!("could not release Esc for the Save dialog");
        }
    });
    let saver = Arc::new(saver);
    let asker = Arc::clone(&saver);
    let chosen = spawn_blocking(move || choose(&*asker, now, default_dir))
        .await
        .unwrap_or_else(|e| Err(e.to_string()))
        .map_err(CaptureError::Dialog);
    let path = match chosen {
        Ok(Some(path)) => path,
        Ok(None) => {
            log::info!("save cancelled");
            on_main(&app, move |app| back(app, session, display));
            return Ok(());
        }
        // Not a cancel: the overlay comes back and the page says the save failed.
        Err(e) => {
            log::error!("{e}");
            on_main(&app, move |app| back(app, session, display));
            return Err(e);
        }
    };
    // Always written once a path is picked: the crop is already taken. The windows are hidden
    // meanwhile, so writing before closing costs nothing visible, and a failure can bring the
    // overlay back to say so.
    let written = spawn_blocking(move || write(&image, &path, &*saver))
        .await
        .unwrap_or_else(|e| Err(CaptureError::Write(e.to_string())));
    let failed = written.is_err();
    on_main(&app, move |app| {
        let Some(displays) = phase(app).end_save(session) else {
            return;
        };
        if failed {
            reshow(app, &displays, display);
        } else {
            close(app);
        }
    });
    written.inspect_err(|e| log::error!("{e}"))
}

/// Shows the Save dialog until the user picks a path we may write, or cancels. If `.png` had
/// to be added and that file exists, the dialog opens again with that name, so it asks about
/// overwriting it.
fn choose(
    saver: &impl Saver,
    now: NaiveDateTime,
    default_dir: Option<PathBuf>,
) -> Result<Option<PathBuf>, String> {
    let mut dir = output::save_dir(saver.last_dir(), default_dir, Path::is_dir);
    let mut name = output::file_name(now);
    loop {
        let Some(picked) = saver.ask(dir, &name)? else {
            return Ok(None);
        };
        match output::png_target(picked, Path::exists) {
            Target::Write(path) => return Ok(Some(path)),
            Target::Ask(path) => {
                dir = path.parent().map(Path::to_owned);
                // A path made by adding ".png" always has a file name.
                name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
            }
        }
    }
}

/// Encodes and writes the PNG, then remembers its folder.
fn write(image: &RgbaImage, path: &Path, saver: &impl Saver) -> Result<(), CaptureError> {
    let png = capture::encode_png(image)?;
    output::write_file(path, &png)
        .map_err(|e| CaptureError::Write(format!("{}: {e}", path.display())))?;
    log::info!("saved {}", path.display());
    if let Some(dir) = path.parent() {
        saver.remember_dir(dir);
    }
    Ok(())
}

/// The Save dialog was cancelled: show the windows again.
fn back<R: Runtime>(app: &AppHandle<R>, session: u64, display: u32) {
    let displays = phase(app).end_save(session);
    if let Some(displays) = displays {
        reshow(app, &displays, display);
    }
}

/// The OS side of a save. Faked in tests.
pub(crate) trait Saver: Send + Sync + 'static {
    /// Shows the Save dialog in `dir` with `name` filled in and waits for it (never on the main
    /// thread). The chosen path, `None` if cancelled, or the error if the dialog failed.
    fn ask(&self, dir: Option<PathBuf>, name: &str) -> Result<Option<PathBuf>, String>;
    /// The folder of the last save, from the prefs.
    fn last_dir(&self) -> Option<PathBuf>;
    fn remember_dir(&self, dir: &Path);
}

// ---- OS calls only below. ----

struct OsSaver<R: Runtime>(AppHandle<R>);

impl<R: Runtime> Saver for OsSaver<R> {
    fn ask(&self, dir: Option<PathBuf>, name: &str) -> Result<Option<PathBuf>, String> {
        use tauri_plugin_dialog::DialogExt;
        let mut dialog = self
            .0
            .dialog()
            .file()
            // Adds `.png` to a typed name where the platform supports it.
            .add_filter("PNG", &["png"])
            .set_file_name(name)
            .set_can_create_directories(true);
        if let Some(dir) = dir {
            dialog = dialog.set_directory(dir);
        }
        dialog
            .blocking_save_file()
            .map(|path| path.into_path().map_err(|e| e.to_string()))
            .transpose()
    }

    fn last_dir(&self) -> Option<PathBuf> {
        prefs::load(&self.0).last_save_dir()
    }

    /// Through `prefs::update`, so it can't write back settings changed meanwhile.
    fn remember_dir(&self, dir: &Path) {
        // `set_last_save_dir` logs a folder it can't remember; nothing to save then.
        const SKIPPED: &str = "not remembered";
        let remembered = prefs::update(&self.0, |p| {
            if p.set_last_save_dir(dir) {
                Ok(())
            } else {
                Err(SKIPPED)
            }
        });
        if let Err(e) = remembered
            && e != SKIPPED
        {
            log::warn!("could not remember the save folder: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::px;
    use super::super::window::shown as on_screen;
    use super::super::window::tests::{
        Handle, app, app_with_escape, app_with_escape_by, esc_held, is_idle, is_open, open, press,
        shown, wait_until, window,
    };
    use super::super::window::{close_session, overlay_escape, overlay_painted};
    use super::*;
    use crate::output::tests::assert_clean_png;
    use chrono::NaiveDate;
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A clipboard that records what it got, or fails.
    fn clipboard(fail: bool) -> (Clipboard, Arc<Mutex<Vec<RgbaImage>>>) {
        let images = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&images);
        let clipboard: Clipboard = Box::new(move |image: &RgbaImage| {
            if fail {
                return Err(CaptureError::Clipboard("busy".into()));
            }
            record.lock().unwrap().push(image.clone());
            Ok(())
        });
        (clipboard, images)
    }

    /// The crop `frame` gives: it colors pixel (x, y) as (x, y, 9).
    fn expected(x0: u32, y0: u32, width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x0 + x) as u8, (y0 + y) as u8, 9, 255])
        })
    }

    fn copy_in<R: Runtime>(
        w: &WebviewWindow<R>,
        h: &AppHandle<R>,
        session: u64,
        rect: PixelRect,
        c: Clipboard,
    ) -> Result<(), CaptureError> {
        let state = h.state::<Overlay>();
        let out = plain(session, rect);
        tauri::async_runtime::block_on(copy_with(w, &state, &out, c))
    }

    fn copy(h: &Handle, session: u64, rect: PixelRect, c: Clipboard) -> Result<(), CaptureError> {
        copy_in(&window(h, 1), h, session, rect, c)
    }

    #[test]
    fn copy_puts_exactly_the_pixel_rect_on_the_clipboard_and_closes() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        let (c, images) = clipboard(false);
        assert_eq!(copy(h, session, px(1, 0, 2, 2), c), Ok(()));
        assert_eq!(*images.lock().unwrap(), [expected(1, 0, 2, 2)]);
        assert!(is_idle(h));
    }

    #[test]
    fn copy_clamps_to_the_display_and_rejects_an_empty_rect() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        let (c, _) = clipboard(false);
        let empty = copy(h, session, px(0, 0, 0, 1), c);
        assert_eq!(empty, Err(CaptureError::EmptySelection));
        assert!(
            is_open(h, session),
            "an empty selection keeps the overlay open"
        );
        let (c, images) = clipboard(false);
        copy(h, session, px(3, 1, 10, 10), c).unwrap();
        assert_eq!(images.lock().unwrap()[0].dimensions(), (1, 1));
    }

    #[test]
    fn copy_ignores_other_sessions_and_windows() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        let (c, images) = clipboard(false);
        assert_eq!(copy(h, session + 1, px(0, 0, 1, 1), c), Ok(()));
        let settings =
            tauri::WebviewWindowBuilder::new(h, "settings", tauri::WebviewUrl::default())
                .build()
                .unwrap();
        let (c, _) = clipboard(false);
        assert_eq!(copy_in(&settings, h, session, px(0, 0, 1, 1), c), Ok(()));
        assert!(images.lock().unwrap().is_empty());
        assert!(is_open(h, session));
    }

    #[test]
    fn a_failed_copy_keeps_the_overlay_open() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        let (c, _) = clipboard(true);
        let r = copy(h, session, px(0, 0, 2, 2), c);
        assert_eq!(r, Err(CaptureError::Clipboard("busy".into())));
        assert!(is_open(h, session));
    }

    #[test]
    fn a_copy_does_not_close_after_a_save_began_meanwhile() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        // While the clipboard works, another display opens the Save dialog and cancels it.
        let handle = h.clone();
        let clipboard: Clipboard = Box::new(move |_: &RgbaImage| {
            let mut p = phase(&handle);
            assert!(p.begin_save(session));
            assert!(p.end_save(session).is_some());
            Ok(())
        });
        assert_eq!(copy(h, session, px(0, 0, 2, 2), clipboard), Ok(()));
        // The overlay stays: the user moved on from that copy.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(is_open(h, session));
    }

    /// Answers the dialog from `answers` (cancel when they run out) and records what it saw.
    #[derive(Default)]
    struct FakeSaver {
        answers: Mutex<VecDeque<Result<Option<PathBuf>, String>>>,
        last: Option<PathBuf>,
        asked: Mutex<Vec<(Option<PathBuf>, String)>>,
        /// What `during` returned each time the dialog was "open".
        during_results: Mutex<Vec<bool>>,
        remembered: Mutex<Vec<PathBuf>>,
        /// Runs while the dialog is "open".
        during: Option<Box<dyn Fn() -> bool + Send + Sync>>,
    }

    impl FakeSaver {
        fn answering(answers: &[Option<&Path>]) -> Self {
            Self {
                answers: Mutex::new(answers.iter().map(|a| Ok(a.map(Path::to_owned))).collect()),
                ..Default::default()
            }
        }
    }

    impl Saver for Arc<FakeSaver> {
        fn ask(&self, dir: Option<PathBuf>, name: &str) -> Result<Option<PathBuf>, String> {
            self.asked.lock().unwrap().push((dir, name.to_owned()));
            if let Some(during) = &self.during {
                self.during_results.lock().unwrap().push(during());
            }
            self.answers.lock().unwrap().pop_front().unwrap_or(Ok(None))
        }

        fn last_dir(&self) -> Option<PathBuf> {
            self.last.clone()
        }

        fn remember_dir(&self, dir: &Path) {
            self.remembered.lock().unwrap().push(dir.to_owned());
        }
    }

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 6)
            .unwrap()
            .and_hms_opt(13, 22, 5)
            .unwrap()
    }

    fn save(
        h: &Handle,
        session: u64,
        rect: PixelRect,
        saver: &Arc<FakeSaver>,
        default: &Path,
    ) -> Result<(), CaptureError> {
        save_out(h, &plain(session, rect), saver, default)
    }

    fn save_out(
        h: &Handle,
        out: &Output,
        saver: &Arc<FakeSaver>,
        default: &Path,
    ) -> Result<(), CaptureError> {
        let state = h.state::<Overlay>();
        let w = window(h, 1);
        let default = Some(default.to_owned());
        let saver = Arc::clone(saver);
        tauri::async_runtime::block_on(save_with(&w, &state, out, saver, now(), default))
    }

    /// A request with no annotations.
    fn plain(session: u64, rect: PixelRect) -> Output<'static> {
        Output {
            session,
            rect,
            marks: Marks::default(),
        }
    }

    const WHITE: [u8; 4] = [255; 4];

    /// A white dot at (0, 0) of the crop, and the rest of a 2x2 crop blurred.
    fn marks() -> Marks<'static> {
        Marks {
            layer: Some(Layer {
                x: 0,
                y: 0,
                pixels: ImageBuffer::from_raw(1, 1, &WHITE[..]).unwrap(),
            }),
            blurs: vec![px(0, 0, 2, 2)],
        }
    }

    /// `expected(1, 0, 2, 2)` with `marks()` on it.
    fn marked() -> RgbaImage {
        let mut image = expected(1, 0, 2, 2);
        compose::apply(&mut image, &marks());
        image
    }

    #[test]
    fn copy_puts_the_annotations_on_the_crop() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        let (c, images) = clipboard(false);
        let state = h.state::<Overlay>();
        let w = window(h, 1);
        let out = Output {
            marks: marks(),
            ..plain(session, px(1, 0, 2, 2))
        };
        let copying = copy_with(&w, &state, &out, c);
        assert_eq!(tauri::async_runtime::block_on(copying), Ok(()));
        let copied = images.lock().unwrap()[0].clone();
        assert_eq!(copied, marked());
        assert_eq!(*copied.get_pixel(0, 0), image::Rgba([255, 255, 255, 255]));
        assert_ne!(copied, expected(1, 0, 2, 2));
    }

    #[test]
    fn save_writes_a_clean_png_remembers_the_folder_and_closes() {
        let app = app_with_escape();
        let h = app.handle();
        let dir = tempfile::tempdir().unwrap();
        let session = shown(h, &[1]);
        assert!(esc_held(h));
        let path = dir.path().join("mine.png");
        let mut saver = FakeSaver::answering(&[Some(&path)]);
        let handle = h.clone();
        saver.during = Some(Box::new(move || !esc_held(&handle)));
        let saver = Arc::new(saver);
        assert_eq!(save(h, session, px(1, 0, 3, 2), &saver, dir.path()), Ok(()));
        assert_eq!(
            *saver.asked.lock().unwrap(),
            [(
                Some(dir.path().to_owned()),
                "xcreenshot-2026-11-06-13.22.05.png".to_owned()
            )]
        );
        assert_eq!(
            *saver.during_results.lock().unwrap(),
            [true],
            "Esc is the dialog's"
        );
        assert_clean_png(&fs::read(&path).unwrap(), &expected(1, 0, 3, 2));
        assert_eq!(*saver.remembered.lock().unwrap(), [dir.path().to_owned()]);
        wait_until("the overlay to close", || is_idle(h));
        assert!(!esc_held(h));
    }

    #[test]
    fn save_writes_the_annotations_with_the_blur_baked_in() {
        let app = app_with_escape();
        let h = app.handle();
        let dir = tempfile::tempdir().unwrap();
        let session = shown(h, &[1]);
        let path = dir.path().join("marked.png");
        let saver = Arc::new(FakeSaver::answering(&[Some(&path)]));
        let out = Output {
            marks: marks(),
            ..plain(session, px(1, 0, 2, 2))
        };
        let saved = save_out(h, &out, &saver, dir.path());
        assert_eq!(saved, Ok(()));
        assert_clean_png(&fs::read(&path).unwrap(), &marked());
        wait_until("the overlay to close", || is_idle(h));
    }

    #[test]
    fn save_opens_in_the_last_folder_and_adds_the_extension() {
        let app = app_with_escape();
        let h = app.handle();
        let last = tempfile::tempdir().unwrap();
        let session = shown(h, &[1]);
        let mut saver = FakeSaver::answering(&[Some(&last.path().join("name"))]);
        saver.last = Some(last.path().to_owned());
        let saver = Arc::new(saver);
        save(h, session, px(0, 0, 1, 1), &saver, Path::new("/default")).unwrap();
        assert_eq!(
            saver.asked.lock().unwrap()[0].0,
            Some(last.path().to_owned())
        );
        assert!(last.path().join("name.png").exists());
    }

    #[test]
    fn asks_again_rather_than_overwrite_a_file_the_user_did_not_see() {
        let app = app_with_escape();
        let h = app.handle();
        let dir = tempfile::tempdir().unwrap();
        let taken = dir.path().join("name.png");
        fs::write(&taken, b"keep me").unwrap();
        let session = shown(h, &[1]);
        // The user typed "name" and "name.png" exists: the dialog opens again with that name,
        // and cancelling it writes nothing.
        let saver = Arc::new(FakeSaver::answering(&[Some(&dir.path().join("name"))]));
        save(h, session, px(0, 0, 1, 1), &saver, dir.path()).unwrap();
        let asked = saver.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 2);
        assert_eq!(
            asked[1],
            (Some(dir.path().to_owned()), "name.png".to_owned())
        );
        assert_eq!(fs::read(&taken).unwrap(), b"keep me");
        assert!(is_open(h, session));
        // Confirming it there replaces the file.
        let saver = Arc::new(FakeSaver::answering(&[
            Some(&dir.path().join("name")),
            Some(&taken),
        ]));
        save(h, session, px(0, 0, 2, 2), &saver, dir.path()).unwrap();
        assert_clean_png(&fs::read(&taken).unwrap(), &expected(0, 0, 2, 2));
    }

    #[test]
    fn cancel_shows_the_overlay_again_with_the_selection_usable() {
        let app = app_with_escape();
        let h = app.handle();
        let session = shown(h, &[1, 2]);
        assert_eq!(on_screen(h), [1, 2]);
        let mut saver = FakeSaver::answering(&[None]);
        let handle = h.clone();
        saver.during = Some(Box::new(move || on_screen(&handle).is_empty()));
        let saver = Arc::new(saver);
        assert_eq!(
            save(h, session, px(0, 0, 2, 2), &saver, Path::new("/d")),
            Ok(())
        );
        assert!(saver.remembered.lock().unwrap().is_empty());
        assert!(is_open(h, session));
        assert!(!phase(h).saving());
        assert_eq!(
            *saver.during_results.lock().unwrap(),
            [true],
            "hidden during the dialog"
        );
        assert_eq!(on_screen(h), [1, 2], "shown again");
        assert!(esc_held(h), "Esc is the overlay's again");
        let (c, images) = clipboard(false);
        copy(h, session, px(0, 0, 1, 1), c).unwrap();
        assert_eq!(images.lock().unwrap().len(), 1);
        assert!(is_idle(h));
    }

    #[test]
    fn a_failed_write_shows_the_overlay_again_and_reports_it() {
        let app = app_with_escape();
        let h = app.handle();
        let dir = tempfile::tempdir().unwrap();
        let session = shown(h, &[1]);
        let path = dir.path().join("gone").join("x.png");
        let saver = Arc::new(FakeSaver::answering(&[Some(&path)]));
        let r = save(h, session, px(0, 0, 2, 2), &saver, dir.path());
        assert!(matches!(r, Err(CaptureError::Write(_))), "{r:?}");
        assert!(is_open(h, session));
        assert_eq!(on_screen(h), [1], "shown again to say so");
        assert!(esc_held(h));
        assert!(saver.remembered.lock().unwrap().is_empty());
    }

    #[test]
    fn nothing_closes_the_overlay_during_the_dialog_and_the_file_is_written() {
        let app = app_with_escape();
        let h = app.handle();
        let dir = tempfile::tempdir().unwrap();
        let session = shown(h, &[1]);
        let path = dir.path().join("kept.png");
        let mut saver = FakeSaver::answering(&[Some(&path)]);
        let handle = h.clone();
        saver.during = Some(Box::new(move || {
            // Esc forwarded by a page, a close request and a copy all do nothing.
            overlay_escape(handle.clone());
            close(&handle);
            close_session(&handle, session);
            let (c, images) = clipboard(false);
            copy(&handle, session, px(0, 0, 1, 1), c).unwrap();
            images.lock().unwrap().is_empty() && is_open(&handle, session)
        }));
        let saver = Arc::new(saver);
        save(h, session, px(0, 0, 1, 1), &saver, dir.path()).unwrap();
        assert_eq!(*saver.during_results.lock().unwrap(), [true]);
        assert!(path.exists());
        assert!(is_idle(h));
    }

    #[test]
    fn cancel_without_the_esc_shortcut_keeps_the_overlay() {
        // No Esc shortcut (another app held it), so the pages forward Esc: nothing to take
        // back, and the overlay stays.
        let app = app();
        let h = app.handle();
        let (session, release) = press(h, &[1]);
        release.send(true).unwrap();
        wait_until("the session to open", || is_open(h, session));
        overlay_painted(window(h, 1), h.state(), session);
        let saver = Arc::new(FakeSaver::default());
        save(h, session, px(0, 0, 2, 2), &saver, Path::new("/d")).unwrap();
        assert!(is_open(h, session));
    }

    #[test]
    fn closes_if_esc_cannot_be_taken_back() {
        // Esc registers at the start, then fails after the dialog: never stay open without a
        // way out.
        let calls = AtomicU32::new(0);
        let app = app_with_escape_by(move |on| !on || calls.fetch_add(1, Ordering::Relaxed) == 0);
        let h = app.handle();
        let session = shown(h, &[1]);
        let saver = Arc::new(FakeSaver::default());
        save(h, session, px(0, 0, 2, 2), &saver, Path::new("/d")).unwrap();
        assert!(is_idle(h));
    }

    #[test]
    fn a_window_painted_during_the_dialog_stays_hidden_until_cancel() {
        let app = app_with_escape();
        let h = app.handle();
        let session = open(h, &[1, 2]);
        overlay_painted(window(h, 1), h.state(), session);
        let mut saver = FakeSaver::default();
        let handle = h.clone();
        saver.during = Some(Box::new(move || {
            overlay_painted(window(&handle, 2), handle.state(), session);
            on_screen(&handle).is_empty()
        }));
        let saver = Arc::new(saver);
        save(h, session, px(0, 0, 2, 2), &saver, Path::new("/d")).unwrap();
        assert_eq!(
            *saver.during_results.lock().unwrap(),
            [true],
            "display 2 stayed hidden"
        );
        assert_eq!(on_screen(h), [1, 2], "both shown after the cancel");
        assert!(is_open(h, session));
        assert!(esc_held(h));
    }

    #[test]
    fn save_is_ignored_unless_shown_and_not_saving() {
        let app = app_with_escape();
        let h = app.handle();
        let session = open(h, &[1]);
        let saver = Arc::new(FakeSaver::default());
        let d = Path::new("/d");
        assert_eq!(
            save(h, session, px(0, 0, 1, 1), &saver, d),
            Ok(()),
            "not painted"
        );
        assert!(saver.asked.lock().unwrap().is_empty());
        overlay_painted(window(h, 1), h.state(), session);
        let empty = save(h, session, px(0, 0, 0, 0), &saver, d);
        assert_eq!(empty, Err(CaptureError::EmptySelection));
        assert!(is_open(h, session));
        assert!(phase(h).begin_save(session));
        assert_eq!(
            save(h, session, px(0, 0, 1, 1), &saver, d),
            Ok(()),
            "saving"
        );
        assert!(saver.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_dialog_is_reported_not_taken_for_a_cancel() {
        let app = app_with_escape();
        let h = app.handle();
        let session = shown(h, &[1]);
        let saver = FakeSaver::default();
        saver
            .answers
            .lock()
            .unwrap()
            .push_back(Err("no local path".into()));
        let saver = Arc::new(saver);
        let r = save(h, session, px(0, 0, 2, 2), &saver, Path::new("/d"));
        assert_eq!(r, Err(CaptureError::Dialog("no local path".into())));
        assert!(is_open(h, session));
        assert_eq!(on_screen(h), [1], "back to say so");
        assert!(esc_held(h));
    }

    #[test]
    fn the_dialog_opens_even_if_esc_cannot_be_released() {
        // Taking Esc works, releasing it fails: the dialog still opens, and Esc during it does
        // not close the overlay.
        let app = app_with_escape_by(|on| on);
        let h = app.handle();
        let session = shown(h, &[1]);
        let mut saver = FakeSaver::default();
        let handle = h.clone();
        saver.during = Some(Box::new(move || {
            overlay_escape(handle.clone());
            is_open(&handle, session) && phase(&handle).saving()
        }));
        let saver = Arc::new(saver);
        save(h, session, px(0, 0, 2, 2), &saver, Path::new("/d")).unwrap();
        assert_eq!(saver.asked.lock().unwrap().len(), 1, "the dialog opened");
        assert_eq!(*saver.during_results.lock().unwrap(), [true]);
        assert!(is_open(h, session));
        assert_eq!(on_screen(h), [1]);
    }

    #[test]
    fn a_cancel_for_a_session_that_is_not_saving_changes_nothing() {
        let app = app_with_escape();
        let h = app.handle();
        let session = shown(h, &[1]);
        back(h, session, 1);
        assert!(is_open(h, session));
    }

    fn headers(json: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(HEADER, tauri::http::HeaderValue::from_str(json).unwrap());
        h
    }

    #[test]
    fn parses_a_plain_request() {
        let h = headers(r#"{"session":3,"rect":{"x":1,"y":2,"width":3,"height":4}}"#);
        let body = InvokeBody::Raw(vec![]);
        assert_eq!(parse(&body, &h).unwrap(), plain(3, px(1, 2, 3, 4)));
        // An empty body may come as JSON.
        let json = InvokeBody::Json(serde_json::Value::Null);
        assert_eq!(parse(&json, &h).unwrap(), plain(3, px(1, 2, 3, 4)));
    }

    #[test]
    fn parses_the_layer_in_place_and_the_blurs() {
        let h = headers(
            r#"{"session":3,"rect":{"x":0,"y":0,"width":9,"height":9},"layer":{"x":1,"y":2,"width":2,"height":1},"blurs":[{"x":0,"y":0,"width":4,"height":4}]}"#,
        );
        let body = InvokeBody::Raw(vec![1, 2, 3, 4, 5, 6, 7, 8]);
        let out = parse(&body, &h).unwrap();
        let layer = out.marks.layer.unwrap();
        assert_eq!(
            (layer.x, layer.y, layer.pixels.dimensions()),
            (1, 2, (2, 1))
        );
        let InvokeBody::Raw(bytes) = &body else {
            unreachable!()
        };
        // The body itself, not a copy.
        assert_eq!(layer.pixels.as_raw().as_ptr(), bytes.as_ptr());
        assert_eq!(out.marks.blurs, [px(0, 0, 4, 4)]);
    }

    #[test]
    fn blurs_alone_have_no_layer() {
        let h = headers(
            r#"{"session":1,"rect":{"x":0,"y":0,"width":9,"height":9},"blurs":[{"x":0,"y":0,"width":4,"height":4}]}"#,
        );
        let body = InvokeBody::Raw(vec![]);
        let out = parse(&body, &h).unwrap();
        assert_eq!(out.marks.layer, None);
        assert_eq!(out.marks.blurs.len(), 1);
    }

    #[test]
    fn rejects_a_bad_request() {
        let body = InvokeBody::Raw(vec![1, 2, 3]);
        let missing = parse(&body, &HeaderMap::new()).unwrap_err();
        assert!(matches!(missing, CaptureError::Invalid(_)), "{missing}");
        assert!(parse(&body, &headers("{")).is_err());
        // The body must hold exactly the layer's pixels.
        let short = headers(
            r#"{"session":1,"rect":{"x":0,"y":0,"width":9,"height":9},"layer":{"x":0,"y":0,"width":2,"height":2}}"#,
        );
        let e = parse(&body, &short).unwrap_err();
        assert!(e.to_string().contains("3 bytes for a 2x2 layer"), "{e}");
    }

    #[test]
    fn caps_the_blurs_and_clips_them_to_the_selection() {
        let blur = r#"{"x":0,"y":0,"width":1,"height":1}"#;
        let many = vec![blur; MAX_BLURS + 1].join(",");
        let json = format!(
            r#"{{"session":1,"rect":{{"x":0,"y":0,"width":9,"height":9}},"blurs":[{many}]}}"#
        );
        let body = InvokeBody::Raw(vec![]);
        let e = parse(&body, &headers(&json)).unwrap_err();
        assert!(e.to_string().contains("at most 256"), "{e}");
        let h = headers(
            r#"{"session":1,"rect":{"x":0,"y":0,"width":9,"height":9},"blurs":[{"x":5,"y":5,"width":99,"height":99},{"x":50,"y":0,"width":4,"height":4}]}"#,
        );
        assert_eq!(parse(&body, &h).unwrap().marks.blurs, [px(5, 5, 4, 4)]);
    }
}
