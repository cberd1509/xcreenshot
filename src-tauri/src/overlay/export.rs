//! Copy and save: the overlay's way out with the selection.
//!
//! Both crop the selection (native pixels on the calling window's display, as the size badge
//! showed it) from the original frame Rust kept, so the result is exactly the captured pixels.
//!
//! - **Copy** (Enter, ⌘C / Ctrl+C) crops and puts the image on the clipboard off the main
//!   thread, then closes the overlay.
//! - **Save** (⌘S / Ctrl+S) hides the overlay windows, which sit above the Save dialog, and
//!   releases Esc to the dialog. The session stays, and nothing can close it until the save
//!   ends. The PNG is encoded on a worker while the dialog is open, so once a path is picked
//!   only the write is left; the file is always written. Then the overlay closes; if the
//!   write failed, or the dialog was cancelled, the windows come back with the selection
//!   intact, so the user can try again, copy, change the selection or press Esc.
//!
//! - **Share** (the Share button, macOS) encodes the PNG, releases Esc to the share sheet and
//!   opens it under the button. The windows stay. Picking a service closes the overlay; closing
//!   the sheet without one takes Esc back and keeps the selection. See `docs/share.md`.
//!
//! On failure the command returns the error and the page says so; the overlay is never left
//! without a way out. The OS side is behind [`Clipboard`], [`Saver`] and [`ShowSheet`], faked in
//! tests.
//!
//! A copy, save or share that worked remembers the selection (see `region`), after the
//! clipboard write, the file or the choice of a service, on another thread; a quick copy does
//! not. Then it shows a toast (see `toast`).

use super::display_id;
use super::window::{
    Overlay, close, close_for_share, close_if, hide, on_main, phase, reshow, sync_escape,
};
use crate::capture::{self, CaptureError, Density, PixelRect, clamp_rect};
use crate::output::compose::{self, Layer, Marks};
use crate::output::{self, Clipboard, Target};
use crate::prefs;
use crate::region::{self, Region};
use crate::share::{self, OnDone, ShareError, ShareOutcome};
use crate::toast::{self, Toast};
use chrono::NaiveDateTime;
use image::{ImageBuffer, RgbaImage};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::async_runtime::{JoinHandle, spawn_blocking};
use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};

/// The request header describing a copy or save. Must match `OUTPUT_HEADER` on the page.
pub const HEADER: &str = "x-xcreenshot-output";

/// Most blur rects one request may have. More is a broken or hostile page.
pub const MAX_BLURS: usize = 256;

/// The JSON in [`HEADER`]. `layer` is where the body's pixels go, relative to `rect`; it is
/// there only when the body has them. `anchor` is the Share button, for a share. `remember` is
/// `false` for an output whose region must not be remembered (a quick copy).
#[derive(Debug, Deserialize)]
struct Head {
    session: u64,
    rect: PixelRect,
    #[serde(default)]
    layer: Option<PixelRect>,
    #[serde(default)]
    blurs: Vec<PixelRect>,
    #[serde(default)]
    anchor: Option<share::Rect>,
    #[serde(default = "yes")]
    remember: bool,
}

fn yes() -> bool {
    true
}

/// A copy, save or share request: the session, the selection in native px, and its
/// annotations, whose pixels are read in place from the request body. A share also has the
/// button the sheet opens under, in logical px in the page.
#[derive(Debug, PartialEq)]
pub(crate) struct Output<'a> {
    pub session: u64,
    pub rect: PixelRect,
    pub marks: Marks<'a>,
    pub anchor: Option<share::Rect>,
    /// Whether the region may be remembered (not for a quick copy).
    pub remember: bool,
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
        anchor: head.anchor,
        remember: head.remember,
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
    let app = window.app_handle();
    let clipboard = output::os_clipboard(app);
    copy_with(&window, &state, &out, clipboard, remember_in(app)).await
}

/// Keeps the region of a copy or save that worked. Faked in tests.
pub(crate) type Remember = Box<dyn FnOnce(Region) + Send>;

/// Saves the region to the prefs on a blocking thread, so nothing waits for the file.
fn remember_in<R: Runtime>(app: &AppHandle<R>) -> Remember {
    let app = app.clone();
    Box::new(move |r| {
        // Numbered now, as the output succeeds, so saves that land late keep this order.
        let seq = region::ORDER.next();
        drop(spawn_blocking(move || region::save(&app, r, seq)));
    })
}

async fn copy_with<R: Runtime>(
    window: &WebviewWindow<R>,
    state: &Overlay,
    out: &Output<'_>,
    clipboard: Clipboard,
    remember: Remember,
) -> Result<(), CaptureError> {
    let started = Instant::now();
    let session = out.session;
    let Some(Selected {
        frame,
        density,
        saves,
        region,
        ..
    }) = selected_frame(window, state, out)
    else {
        return Ok(());
    };
    // Cropped and annotated here: the annotations are read in place from the request.
    let image = annotated(&frame, out)?;
    drop(frame);
    let copied = spawn_blocking(move || {
        clipboard(&image, density)?;
        Ok(image.dimensions())
    })
    .await
    .unwrap_or_else(|e| Err(CaptureError::Clipboard(e.to_string())));
    let (width, height) = copied.inspect_err(|e| log::warn!("copy failed: {e}"))?;
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    log::info!("copied {width}x{height} px in {ms:.0} ms");
    if let Some(region) = region {
        remember(region);
    }
    // Only if nothing happened meanwhile: a save started (even cancelled) keeps the overlay.
    // The toast only with the close: not over a Save dialog, nor under a newer capture.
    let anchor = window.clone();
    on_main(window.app_handle(), move |app| {
        let mut closing = false;
        close_if(app, |phase| {
            closing = phase.copy_may_close(session, saves);
            closing
        });
        if closing {
            toast::show(&anchor, Toast::Copied { width, height });
        }
    });
    Ok(())
}

/// What a copy or save works from.
struct Selected {
    /// The calling window's display.
    display: u32,
    /// Its original frame.
    frame: Arc<RgbaImage>,
    /// The frame's pixel density, for the PNG.
    density: Density,
    /// The session's save count.
    saves: u32,
    /// The selection to remember, unless a display was not captured.
    region: Option<Region>,
}

/// What a copy or save works from, or `None` if the session or window is not an open overlay,
/// or the Save dialog is open.
fn selected_frame<R: Runtime>(
    window: &WebviewWindow<R>,
    state: &Overlay,
    out: &Output,
) -> Option<Selected> {
    let session = out.session;
    let display = display_id(window.label())?;
    let mut phase = state.phase();
    let saves = phase.saves(session)?;
    let (frame, density) = phase.image(session, display)?;
    let region = if out.remember {
        phase.region(session, display, out.rect)
    } else {
        None
    };
    Some(Selected {
        display,
        saves,
        frame,
        density,
        region,
    })
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
    let remember = remember_in(&app);
    let saver = OsSaver(app);
    save_with(&window, &state, &out, saver, now, default_dir, remember).await
}

async fn save_with<R: Runtime>(
    window: &WebviewWindow<R>,
    state: &Overlay,
    out: &Output<'_>,
    saver: impl Saver,
    now: NaiveDateTime,
    default_dir: Option<PathBuf>,
    remember: Remember,
) -> Result<(), CaptureError> {
    let session = out.session;
    let Some(Selected {
        display,
        frame,
        density,
        region,
        ..
    }) = selected_frame(window, state, out)
    else {
        return Ok(());
    };
    // Checked before anything hides, so an empty selection just stays.
    let started = Instant::now();
    let image = annotated(&frame, out)?;
    drop(frame);
    let mut steps = Steps::default();
    steps.done("crop", started);
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
    // Encoded while the dialog is open, so a picked path waits only for the write. A cancel
    // lets the encode finish and drops the PNG: one wasted encode, on a worker.
    let size = image.dimensions();
    let encoding = spawn_blocking(move || {
        let started = Instant::now();
        capture::encode_png(&image, density).map(|png| (png, started.elapsed()))
    });
    let saver = Arc::new(saver);
    let asker = Arc::clone(&saver);
    let asked = Instant::now();
    let chosen = spawn_blocking(move || choose(&*asker, now, default_dir))
        .await
        .unwrap_or_else(|e| Err(e.to_string()))
        .map_err(CaptureError::Dialog);
    steps.done("dialog", asked);
    let path = match chosen {
        Ok(Some(path)) => path,
        Ok(None) => {
            log::info!("save cancelled ({steps})");
            on_main(&app, move |app| back(app, session, display));
            return Ok(());
        }
        // Not a cancel: the overlay comes back and the page says the save failed.
        Err(e) => {
            log::error!("{e} ({steps})");
            on_main(&app, move |app| back(app, session, display));
            return Err(e);
        }
    };
    // Always written once a path is picked: the crop is already taken. The windows are hidden
    // meanwhile, so writing before closing costs nothing visible, and a failure can bring the
    // overlay back to say so.
    let saved = Toast::saved(&path);
    let written = write_when_encoded(encoding, path.clone(), saver, &mut steps).await;
    let failed = written.is_err();
    // Before the close, which lets a waiting update restart the app.
    if let Some(region) = region.filter(|_| !failed) {
        remember(region);
    }
    let anchor = window.clone();
    on_main(&app, move |app| {
        let Some(displays) = phase(app).end_save(session) else {
            return;
        };
        if failed {
            reshow(app, &displays, display);
        } else {
            close(app);
            toast::show(&anchor, saved);
        }
    });
    written.inspect_err(|e| log::error!("{e} ({steps})"))?;
    log::info!(
        "saved {}x{} px to {} ({steps})",
        size.0,
        size.1,
        path.display()
    );
    Ok(())
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

/// Waits for the PNG being encoded on a worker, then writes it and remembers its folder on
/// workers, adding each step reached to `steps`, a failed one too.
async fn write_when_encoded(
    encoding: JoinHandle<Result<(Vec<u8>, Duration), CaptureError>>,
    path: PathBuf,
    saver: Arc<impl Saver>,
    steps: &mut Steps,
) -> Result<(), CaptureError> {
    let started = Instant::now();
    let encoded = encoding
        .await
        .unwrap_or_else(|e| Err(CaptureError::Encode(e.to_string())));
    let waited = steps.done("wait for the PNG", started);
    let (png, encode) = encoded?;
    steps.add("encode", encode);
    let target = path.clone();
    let wrote = spawn_blocking(move || {
        output::write_file(&target, &png)
            .map_err(|e| CaptureError::Write(format!("{}: {e}", target.display())))
    })
    .await
    .unwrap_or_else(|e| Err(CaptureError::Write(e.to_string())));
    let written = steps.done("write", waited);
    wrote?;
    if let Some(dir) = path.parent().map(Path::to_owned) {
        let _ = spawn_blocking(move || saver.remember_dir(&dir)).await;
        steps.done("remember folder", written);
    }
    Ok(())
}

/// The steps of a save so far, in order, with how long each took, for the log.
#[derive(Default)]
struct Steps(Vec<(&'static str, Duration)>);

impl Steps {
    fn add(&mut self, step: &'static str, took: Duration) {
        self.0.push((step, took));
    }

    /// Adds `step` as taking from `since` until now, and returns now.
    fn done(&mut self, step: &'static str, since: Instant) -> Instant {
        let now = Instant::now();
        self.add(step, now - since);
        now
    }
}

impl std::fmt::Display for Steps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, (step, took)) in self.0.iter().enumerate() {
            let sep = if i == 0 { "" } else { ", " };
            write!(f, "{sep}{step} {:.0} ms", took.as_secs_f64() * 1000.0)?;
        }
        Ok(())
    }
}

/// The Save dialog was cancelled: show the windows again.
fn back<R: Runtime>(app: &AppHandle<R>, session: u64, display: u32) {
    let displays = phase(app).end_save(session);
    if let Some(displays) = displays {
        reshow(app, &displays, display);
    }
}

/// Shares the selection, with its annotations, in the share sheet under the Share button.
/// Returns once the sheet is open: the overlay closes when a service is picked and stays when
/// the sheet is dismissed. The request is raw: see [`compose`].
#[tauri::command]
pub(crate) async fn overlay_share<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, Overlay>,
    request: Request<'_>,
) -> Result<(), CaptureError> {
    let out = parse(request.body(), request.headers())?;
    let remember = remember_in(window.app_handle());
    share_with(&window, &state, &out, os_sheet(), remember).await
}

/// The real share sheet.
fn os_sheet<R: Runtime>() -> ShowSheet<R> {
    Box::new(|w, png, anchor, done| share::share_png(w, png, anchor, done))
}

/// Shows the share sheet for a PNG under an anchor and returns once it is open
/// (`share::share_png`). Faked in tests.
pub(crate) type ShowSheet<R> =
    Box<dyn FnOnce(&WebviewWindow<R>, &[u8], share::Rect, OnDone) -> Result<(), ShareError> + Send>;

async fn share_with<R: Runtime>(
    window: &WebviewWindow<R>,
    state: &Overlay,
    out: &Output<'_>,
    show: ShowSheet<R>,
    remember: Remember,
) -> Result<(), CaptureError> {
    let anchor = out
        .anchor
        .ok_or_else(|| invalid("a share needs the button's rect"))?;
    let session = out.session;
    let Some(Selected {
        frame,
        density,
        region,
        ..
    }) = selected_frame(window, state, out)
    else {
        return Ok(());
    };
    let image = annotated(&frame, out)?;
    drop(frame);
    // The same PNG a save writes: the services read it from the temp file.
    let png = spawn_blocking(move || capture::encode_png(&image, density))
        .await
        .unwrap_or_else(|e| Err(CaptureError::Encode(e.to_string())))?;
    if !state.phase().begin_share(session) {
        return Ok(());
    }
    let app = window.app_handle().clone();
    // Released so Esc closes the sheet, not the overlay. Queued before the sheet, which opens
    // from the main thread too. If it cannot be released the sheet still opens; `escape`
    // ignores Esc while sharing.
    on_main(&app, |app| {
        if !sync_escape(app) {
            log::warn!("could not release Esc for the share sheet");
        }
    });
    let target = window.clone();
    let done: OnDone = Box::new(move |outcome| shared(&target, session, outcome, region, remember));
    let sheet = window.clone();
    let shown = spawn_blocking(move || show(&sheet, &png, anchor, done))
        .await
        .unwrap_or_else(|e| Err(ShareError::Window(e.to_string())));
    shown.map_err(|e| {
        log::error!("{e}");
        // Ended now, so a retry right after the error is not turned away; Esc follows.
        if state.phase().end_share(session) {
            on_main(&app, take_escape_back);
        }
        CaptureError::Share(e.to_string())
    })
}

/// The share sheet closed (main thread). A chosen service closes the overlay, leaving the app
/// active for its own UI, and remembers the region; a dismissal keeps it open.
fn shared<R: Runtime>(
    window: &WebviewWindow<R>,
    session: u64,
    outcome: ShareOutcome,
    region: Option<Region>,
    remember: Remember,
) {
    let app = window.app_handle();
    match outcome {
        ShareOutcome::Chosen(service) => {
            log::info!("shared with {service}");
            // Only for the session still sharing: a stale answer changes nothing, not even
            // the level of a window a newer sheet lowered.
            if close_for_share(app, session) {
                if let Some(region) = region {
                    remember(region);
                }
                // The sheet left the window lowered, so the service's UI shows over it.
                share::restore_level(window);
                let service = Some(service).filter(|s| !s.is_empty());
                toast::show(window, Toast::Shared { service });
            }
        }
        ShareOutcome::Cancelled => {
            log::info!("share cancelled");
            if phase(app).end_share(session) {
                // Usually back already; not for a picker that expired with its menu open.
                share::restore_level(window);
                take_escape_back(app);
            }
        }
    }
}

/// The share sheet is gone without a choice: Esc is the overlay's again. Closes the overlay
/// instead if Esc cannot be taken back, so it never stays open without a way out.
fn take_escape_back<R: Runtime>(app: &AppHandle<R>) {
    if !sync_escape(app) {
        log::error!("could not take Esc back; closing the overlay");
        close(app);
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
    use tauri::test::MockRuntime;

    /// The test frames have twice as many pixels as points.
    const RETINA: Density = Density::new(2.0, 2.0);

    /// A clipboard that records what it got, or fails. Every image comes at the frame's density.
    fn clipboard(fail: bool) -> (Clipboard, Arc<Mutex<Vec<RgbaImage>>>) {
        let images = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&images);
        let clipboard: Clipboard = Box::new(move |image: &RgbaImage, density| {
            if fail {
                return Err(CaptureError::Clipboard("busy".into()));
            }
            assert_eq!(density, RETINA);
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

    /// Records the regions it is given.
    fn remembering() -> (Remember, Arc<Mutex<Vec<Region>>>) {
        let regions = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&regions);
        let remember: Remember = Box::new(move |r| record.lock().unwrap().push(r));
        (remember, regions)
    }

    /// Ignores the region.
    fn forget() -> Remember {
        Box::new(drop)
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
        tauri::async_runtime::block_on(copy_with(w, &state, &out, c, forget()))
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
        let copied = Toast::Copied {
            width: 2,
            height: 2,
        };
        assert_eq!(toasts(h), [copied]);
    }

    /// The toasts shown so far, as the toast page gets them when it loads.
    fn toasts(h: &Handle) -> Vec<Toast> {
        let Some(window) = h.get_webview_window(toast::LABEL) else {
            return Vec::new();
        };
        let shown = toast::window::toast_ready(window, h.state());
        shown.into_iter().map(|s| s.toast).collect()
    }

    /// The macOS clipboard as `os_clipboard` builds it, on a private pasteboard: the copied
    /// PNG has the session's density.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_copy_puts_the_frames_density_on_the_pasteboard() {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        let pasted = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&pasted);
        let c = output::png_clipboard(move |png| {
            let pasteboard = NSPasteboard::pasteboardWithUniqueName();
            output::macos::write(&pasteboard, png)?;
            // SAFETY: an AppKit constant, valid for the life of the process.
            let data = pasteboard.dataForType(unsafe { NSPasteboardTypePNG });
            *record.lock().unwrap() = data.unwrap().to_vec();
            Ok(())
        });
        assert_eq!(copy(h, session, px(1, 0, 2, 2), c), Ok(()));
        // The test frames are 2× (pixels over points): 144 DPI.
        assert_clean_png(&pasted.lock().unwrap(), &expected(1, 0, 2, 2), RETINA);
        assert_eq!(RETINA.pixels_per_metre(), (5669, 5669));
    }

    #[test]
    fn a_copy_remembers_its_region_only_once_on_the_clipboard() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        let state = h.state::<Overlay>();
        let w = window(h, 1);
        let out = plain(session, px(1, 0, 2, 2));
        let (c, _) = clipboard(true);
        let (remember, regions) = remembering();
        let failed = tauri::async_runtime::block_on(copy_with(&w, &state, &out, c, remember));
        assert!(failed.is_err());
        assert!(regions.lock().unwrap().is_empty(), "the copy failed");
        let (c, _) = clipboard(false);
        let (remember, regions) = remembering();
        let copied = tauri::async_runtime::block_on(copy_with(&w, &state, &out, c, remember));
        assert_eq!(copied, Ok(()));
        let regions = regions.lock().unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].display.id, 1);
        assert_eq!(regions[0].rect, region::tests::rect(0.5, 0.0, 1.0, 1.0));
    }

    #[test]
    fn a_quick_copy_does_not_remember_its_region() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        let state = h.state::<Overlay>();
        let out = Output {
            remember: false,
            ..plain(session, px(1, 0, 2, 2))
        };
        let (c, images) = clipboard(false);
        let (remember, regions) = remembering();
        let w = window(h, 1);
        let copied = copy_with(&w, &state, &out, c, remember);
        assert_eq!(tauri::async_runtime::block_on(copied), Ok(()));
        assert_eq!(*images.lock().unwrap(), [expected(1, 0, 2, 2)]);
        assert!(regions.lock().unwrap().is_empty());
        wait_until("the overlay to close", || is_idle(h));
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
        assert!(toasts(h).is_empty());
    }

    #[test]
    fn a_copy_does_not_close_after_a_save_began_meanwhile() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        // While the clipboard works, another display opens the Save dialog and cancels it.
        let handle = h.clone();
        let clipboard: Clipboard = Box::new(move |_: &RgbaImage, _| {
            let mut p = phase(&handle);
            assert!(p.begin_save(session));
            assert!(p.end_save(session).is_some());
            Ok(())
        });
        assert_eq!(copy(h, session, px(0, 0, 2, 2), clipboard), Ok(()));
        // The overlay stays: the user moved on from that copy. No toast either.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(is_open(h, session));
        assert!(toasts(h).is_empty());
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
        /// The regions the save remembered.
        regions: Arc<Mutex<Vec<Region>>>,
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
        let regions = Arc::clone(&saver.regions);
        let remember: Remember = Box::new(move |r| regions.lock().unwrap().push(r));
        let saver = Arc::clone(saver);
        let saving = save_with(&w, &state, out, saver, now(), default, remember);
        tauri::async_runtime::block_on(saving)
    }

    /// A request with no annotations.
    fn plain(session: u64, rect: PixelRect) -> Output<'static> {
        Output {
            session,
            rect,
            marks: Marks::default(),
            anchor: None,
            remember: true,
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
        let copying = copy_with(&w, &state, &out, c, forget());
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
        assert_clean_png(&fs::read(&path).unwrap(), &expected(1, 0, 3, 2), RETINA);
        assert_eq!(*saver.remembered.lock().unwrap(), [dir.path().to_owned()]);
        let regions = saver.regions.lock().unwrap();
        assert_eq!(regions.len(), 1, "the region is remembered");
        assert_eq!(regions[0].rect, region::tests::rect(0.5, 0.0, 1.5, 1.0));
        drop(regions);
        wait_until("the overlay to close", || is_idle(h));
        assert!(!esc_held(h));
        assert_eq!(toasts(h), [Toast::saved(&path)]);
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
        assert_clean_png(&fs::read(&path).unwrap(), &marked(), RETINA);
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
        assert_clean_png(&fs::read(&taken).unwrap(), &expected(0, 0, 2, 2), RETINA);
    }

    #[test]
    fn save_steps_are_logged_in_order_in_milliseconds() {
        let mut steps = Steps::default();
        assert_eq!(steps.to_string(), "");
        steps.add("crop", Duration::from_micros(9_400));
        steps.add("dialog", Duration::from_micros(2_345_600));
        assert_eq!(steps.to_string(), "crop 9 ms, dialog 2346 ms");
    }

    #[test]
    fn a_done_step_ends_where_the_next_one_starts() {
        let mut steps = Steps::default();
        let start = Instant::now();
        let first = steps.done("a", start);
        let second = steps.done("b", first);
        assert_eq!(steps.0, [("a", first - start), ("b", second - first)]);
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
        assert!(saver.regions.lock().unwrap().is_empty(), "cancelled");
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
        assert!(saver.regions.lock().unwrap().is_empty(), "not written");
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

    const ANCHOR: share::Rect = share::Rect {
        x: 10.0,
        y: 20.0,
        width: 80.0,
        height: 36.0,
    };

    /// What the fake share sheet was shown with (the PNG, the anchor and whether Esc was held
    /// then), and its `on_done`, to answer with.
    #[derive(Default)]
    struct Sheet {
        shown: Vec<(Vec<u8>, share::Rect, bool)>,
        done: Option<OnDone>,
    }

    /// A share sheet that records what it is shown with, or fails to open.
    fn sheet(fail: bool) -> (ShowSheet<MockRuntime>, Arc<Mutex<Sheet>>) {
        let seen = Arc::new(Mutex::new(Sheet::default()));
        let record = Arc::clone(&seen);
        let show: ShowSheet<MockRuntime> = Box::new(move |w, png, anchor, done| {
            let mut s = record.lock().unwrap();
            s.shown
                .push((png.to_vec(), anchor, esc_held(w.app_handle())));
            if fail {
                return Err(ShareError::Window("no view".into()));
            }
            s.done = Some(done);
            Ok(())
        });
        (show, seen)
    }

    /// The user picks a service in the sheet, or dismisses it.
    fn answer(seen: &Mutex<Sheet>, outcome: ShareOutcome) {
        let done = seen.lock().unwrap().done.take().expect("the sheet is open");
        done(outcome);
    }

    fn shown_sheets(seen: &Mutex<Sheet>) -> usize {
        seen.lock().unwrap().shown.len()
    }

    /// A share request with no annotations.
    fn anchored(session: u64, rect: PixelRect) -> Output<'static> {
        Output {
            anchor: Some(ANCHOR),
            ..plain(session, rect)
        }
    }

    fn share_out(
        h: &Handle,
        out: &Output,
        show: ShowSheet<MockRuntime>,
        remember: Remember,
    ) -> Result<(), CaptureError> {
        let state = h.state::<Overlay>();
        let w = window(h, 1);
        tauri::async_runtime::block_on(share_with(&w, &state, out, show, remember))
    }

    #[test]
    fn share_shows_the_annotated_png_and_closes_once_a_service_is_picked() {
        let app = app_with_escape();
        let h = app.handle();
        let session = shown(h, &[1]);
        let (show, seen) = sheet(false);
        let (remember, regions) = remembering();
        let out = Output {
            marks: marks(),
            ..anchored(session, px(1, 0, 2, 2))
        };
        assert_eq!(share_out(h, &out, show, remember), Ok(()));
        {
            let s = seen.lock().unwrap();
            let (png, anchor, held) = &s.shown[0];
            // The same PNG a save writes: blur baked in, at the frame's density.
            assert_clean_png(png, &marked(), RETINA);
            assert_eq!(*anchor, ANCHOR);
            assert!(!held, "Esc is the sheet's");
        }
        assert!(is_open(h, session), "open until a service is picked");
        assert_eq!(on_screen(h), [1], "the windows stay under the sheet");
        assert!(regions.lock().unwrap().is_empty());
        answer(&seen, ShareOutcome::Chosen("AirDrop".into()));
        assert!(is_idle(h));
        assert!(on_screen(h).is_empty());
        let service = Some("AirDrop".to_owned());
        assert_eq!(toasts(h), [Toast::Shared { service }]);
        assert!(!esc_held(h));
        let regions = regions.lock().unwrap();
        assert_eq!(regions.len(), 1, "the region is remembered");
        assert_eq!(regions[0].rect, region::tests::rect(0.5, 0.0, 1.0, 1.0));
    }

    #[test]
    fn dismissing_the_sheet_keeps_the_overlay_and_takes_esc_back() {
        let app = app_with_escape();
        let h = app.handle();
        let session = shown(h, &[1, 2]);
        let (show, seen) = sheet(false);
        let (remember, regions) = remembering();
        share_out(h, &anchored(session, px(0, 0, 2, 2)), show, remember).unwrap();
        assert!(!esc_held(h));
        answer(&seen, ShareOutcome::Cancelled);
        assert!(is_open(h, session));
        assert!(esc_held(h), "Esc is the overlay's again");
        assert_eq!(on_screen(h), [1, 2]);
        assert!(regions.lock().unwrap().is_empty(), "nothing was shared");
        assert!(toasts(h).is_empty());
        // The selection is still usable.
        let (c, images) = clipboard(false);
        copy(h, session, px(0, 0, 1, 1), c).unwrap();
        assert_eq!(images.lock().unwrap().len(), 1);
        assert!(is_idle(h));
    }

    #[test]
    fn nothing_else_runs_while_the_sheet_is_open() {
        let app = app_with_escape();
        let h = app.handle();
        let dir = tempfile::tempdir().unwrap();
        let session = shown(h, &[1]);
        let (show, seen) = sheet(false);
        share_out(h, &anchored(session, px(0, 0, 2, 2)), show, forget()).unwrap();
        // Esc forwarded by a page, a copy, a save and a second share all do nothing.
        overlay_escape(h.clone());
        let (c, images) = clipboard(false);
        copy(h, session, px(0, 0, 1, 1), c).unwrap();
        assert!(images.lock().unwrap().is_empty());
        let saver = Arc::new(FakeSaver::default());
        save(h, session, px(0, 0, 1, 1), &saver, dir.path()).unwrap();
        assert!(saver.asked.lock().unwrap().is_empty());
        let (again, other) = sheet(false);
        let second = share_out(h, &anchored(session, px(0, 0, 1, 1)), again, forget());
        assert_eq!(second, Ok(()));
        assert_eq!(shown_sheets(&other), 0);
        assert!(is_open(h, session));
        answer(&seen, ShareOutcome::Chosen("Messages".into()));
        assert!(is_idle(h));
    }

    #[test]
    fn a_sheet_that_cannot_open_keeps_the_overlay_and_reports_it() {
        let app = app_with_escape();
        let h = app.handle();
        let session = shown(h, &[1]);
        let (show, seen) = sheet(true);
        let r = share_out(h, &anchored(session, px(0, 0, 2, 2)), show, forget());
        assert!(
            matches!(&r, Err(CaptureError::Share(e)) if e.contains("no view")),
            "{r:?}"
        );
        assert_eq!(shown_sheets(&seen), 1);
        // Ended before the error returns, so a retry right away is not turned away.
        assert!(!phase(h).in_dialog());
        assert!(is_open(h, session));
        assert!(esc_held(h), "Esc is the overlay's again");
        // Not left sharing: the next share opens.
        let (show, seen) = sheet(false);
        share_out(h, &anchored(session, px(0, 0, 2, 2)), show, forget()).unwrap();
        assert_eq!(shown_sheets(&seen), 1);
    }

    #[test]
    fn there_is_no_sheet_off_macos_or_in_tests() {
        let app = app_with_escape();
        let h = app.handle();
        let session = shown(h, &[1]);
        let r = share_out(h, &anchored(session, px(0, 0, 2, 2)), os_sheet(), forget());
        assert!(
            matches!(&r, Err(CaptureError::Share(e)) if e.contains("not supported")),
            "{r:?}"
        );
        assert!(is_open(h, session));
        assert!(esc_held(h));
    }

    #[test]
    fn share_is_ignored_unless_shown_and_needs_a_selection_and_an_anchor() {
        let app = app_with_escape();
        let h = app.handle();
        let session = open(h, &[1]);
        let (show, seen) = sheet(false);
        let not_shown = share_out(h, &anchored(session, px(0, 0, 1, 1)), show, forget());
        assert_eq!(not_shown, Ok(()), "not painted");
        assert_eq!(shown_sheets(&seen), 0);
        overlay_painted(window(h, 1), h.state(), session);
        let (show, seen) = sheet(false);
        let other = share_out(h, &anchored(session + 1, px(0, 0, 1, 1)), show, forget());
        assert_eq!(other, Ok(()), "other session");
        assert_eq!(shown_sheets(&seen), 0);
        let (show, seen) = sheet(false);
        let empty = share_out(h, &anchored(session, px(0, 0, 0, 0)), show, forget());
        assert_eq!(empty, Err(CaptureError::EmptySelection));
        assert_eq!(shown_sheets(&seen), 0);
        let (show, seen) = sheet(false);
        let no_anchor = share_out(h, &plain(session, px(0, 0, 1, 1)), show, forget());
        assert!(matches!(no_anchor, Err(CaptureError::Invalid(_))));
        assert_eq!(shown_sheets(&seen), 0);
        assert!(is_open(h, session));
        assert!(esc_held(h));
    }

    #[test]
    fn closes_if_esc_cannot_be_taken_back_after_the_sheet() {
        let calls = AtomicU32::new(0);
        let app = app_with_escape_by(move |on| !on || calls.fetch_add(1, Ordering::Relaxed) == 0);
        let h = app.handle();
        let session = shown(h, &[1]);
        let (show, seen) = sheet(false);
        share_out(h, &anchored(session, px(0, 0, 2, 2)), show, forget()).unwrap();
        answer(&seen, ShareOutcome::Cancelled);
        assert!(is_idle(h));
        assert!(on_screen(h).is_empty(), "the windows are gone too");
    }

    #[test]
    fn nothing_closes_the_overlay_under_the_sheet() {
        let app = app_with_escape();
        let h = app.handle();
        let session = shown(h, &[1]);
        let (show, seen) = sheet(false);
        share_out(h, &anchored(session, px(0, 0, 2, 2)), show, forget()).unwrap();
        close(h);
        close_session(h, session);
        assert!(is_open(h, session));
        assert_eq!(on_screen(h), [1]);
        answer(&seen, ShareOutcome::Cancelled);
        assert!(is_open(h, session));
        assert!(esc_held(h));
    }

    #[test]
    fn a_stale_choice_neither_remembers_nor_closes() {
        let app = app_with_escape();
        let h = app.handle();
        let session = shown(h, &[1]);
        let (remember, regions) = remembering();
        let region = phase(h).region(session, 1, px(0, 0, 2, 2));
        // This session is not sharing: the answer belongs to an older sheet.
        let chosen = ShareOutcome::Chosen("Mail".into());
        shared(&window(h, 1), session, chosen, region, remember);
        assert!(regions.lock().unwrap().is_empty());
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
        let kept = r#"{"session":3,"rect":{"x":1,"y":2,"width":3,"height":4},"remember":false}"#;
        assert!(!parse(&body, &headers(kept)).unwrap().remember);
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
    fn parses_the_share_buttons_rect() {
        let h = headers(
            r#"{"session":3,"rect":{"x":1,"y":2,"width":3,"height":4},"anchor":{"x":10,"y":20,"width":80,"height":36}}"#,
        );
        let body = InvokeBody::Raw(vec![]);
        assert_eq!(parse(&body, &h).unwrap(), anchored(3, px(1, 2, 3, 4)));
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
