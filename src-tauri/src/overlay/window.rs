//! Tauri glue for the overlay: prewarmed windows, the capture flow, commands and timing logs.
//!
//! Flow: hotkey -> capture every display -> keep the frames here -> place and show each
//! display's window -> emit `overlay:show` -> the window fetches its frame (`overlay_frame`),
//! draws it and reports `overlay_painted`. Esc calls `close_overlay`, which hides the windows
//! and frees the frames.

use super::{Guard, Placement, diff, display_id, label, pack_frame, placement};
use crate::capture::{self, CaptureError, DisplayFrame};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;
use tauri::ipc::Response;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window,
};

const SHOW_EVENT: &str = "overlay:show";
const HIDE_EVENT: &str = "overlay:hide";

/// Overlay state, managed by the app.
#[derive(Default)]
pub struct Overlay {
    guard: Guard,
    session: Mutex<Option<Session>>,
}

/// One open overlay: the frozen frames (kept for the selection, #9) and when the hotkey fired.
struct Session {
    frames: Vec<DisplayFrame>,
    started: Instant,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// Creates a hidden overlay window per display at startup, so the hotkey only has to show them.
pub fn prewarm<R: Runtime>(app: &AppHandle<R>) {
    prewarm_with(app, capture::display_ids());
}

fn prewarm_with<R: Runtime>(app: &AppHandle<R>, ids: Result<Vec<u32>, CaptureError>) {
    let result = ids
        .map_err(|e| e.to_string())
        .and_then(|ids| sync(app, &ids).map_err(|e| e.to_string()));
    if let Err(e) = result {
        log::error!("could not create overlay windows: {e}");
    }
}

/// Hotkey handler. Captures first, before any window changes, then shows the overlay.
pub fn start_capture<R: Runtime>(app: &AppHandle<R>) {
    capture_with(app, grab);
}

/// The OS part of a capture: permission check, then every display.
fn grab() -> Result<Vec<DisplayFrame>, CaptureError> {
    if !capture::has_permission() {
        // TODO(#23): show the permission screen instead of only the system prompt.
        let granted = capture::request_permission();
        log::warn!("Screen Recording permission requested (granted: {granted})");
        return Err(CaptureError::PermissionDenied);
    }
    capture::capture_all()
}

fn capture_with<R: Runtime>(
    app: &AppHandle<R>,
    grab: impl FnOnce() -> Result<Vec<DisplayFrame>, CaptureError>,
) {
    let started = Instant::now();
    let state = app.state::<Overlay>();
    if !state.guard.try_begin() {
        return log::info!("capture ignored: the overlay is already open");
    }
    let frames = match grab() {
        Ok(frames) => frames,
        Err(e) => {
            log::error!("capture failed: {e}");
            return state.guard.end();
        }
    };
    let capture_ms = ms(started);
    let shown = Instant::now();
    if let Err(e) = show(app, frames, started) {
        log::error!("could not show the overlay: {e}");
        return close(app);
    }
    log::info!(
        "overlay: capture {capture_ms:.1} ms, show {:.1} ms",
        ms(shown)
    );
}

/// Stores the frames, matches the windows to the displays, then places and shows them.
fn show<R: Runtime>(
    app: &AppHandle<R>,
    frames: Vec<DisplayFrame>,
    started: Instant,
) -> tauri::Result<()> {
    let ids: Vec<u32> = frames.iter().map(|f| f.id).collect();
    let places: Vec<_> = frames.iter().map(|f| (label(f.id), placement(f))).collect();
    // Stored before any window exists, so a window created just now finds its frame on load.
    *lock(&app.state::<Overlay>().session) = Some(Session { frames, started });
    sync(app, &ids)?;
    for (i, (label, place)) in places.into_iter().enumerate() {
        let Some(window) = app.get_webview_window(&label) else {
            continue;
        };
        position(&window, place)?;
        window.show()?;
        if i == 0 {
            window.set_focus()?;
        }
        app.emit_to(label.as_str(), SHOW_EVENT, ())?;
    }
    Ok(())
}

fn position<R: Runtime>(window: &WebviewWindow<R>, place: Placement) -> tauri::Result<()> {
    match place {
        // macOS resizes from the bottom-left corner, so size first, then move.
        Placement::Logical(b) => {
            window.set_size(LogicalSize::new(b.width, b.height))?;
            window.set_position(LogicalPosition::new(b.x, b.y))
        }
        // Windows rescales a window that moves to a monitor with another DPI, so move first.
        Placement::Physical(b) => {
            window.set_position(PhysicalPosition::new(b.x, b.y))?;
            window.set_size(PhysicalSize::new(b.width, b.height))
        }
    }
}

/// Hides every overlay, frees the frames and allows the next capture. Safe to call twice.
pub fn close<R: Runtime>(app: &AppHandle<R>) {
    let mut other_window_visible = false;
    for (label, window) in app.webview_windows() {
        if display_id(&label).is_none() {
            other_window_visible |= window.is_visible().unwrap_or(false);
            continue;
        }
        // Clears the canvas, so the next show does not flash this frame.
        let _ = app.emit_to(label.as_str(), HIDE_EVENT, ());
        if let Err(e) = window.hide() {
            log::warn!("could not hide {label}: {e}");
        }
    }
    let state = app.state::<Overlay>();
    lock(&state.session).take();
    state.guard.end();
    // Give focus back to the app the user was in.
    #[cfg(target_os = "macos")]
    if !other_window_visible {
        let _ = app.hide();
    }
    let _ = other_window_visible;
}

/// Closing an overlay window (Alt+F4, Cmd+W) closes the overlay instead of destroying it.
/// Returns whether the close was handled.
pub fn close_requested<R: Runtime>(window: &Window<R>) -> bool {
    if display_id(window.label()).is_none() {
        return false;
    }
    close(window.app_handle());
    true
}

/// Creates and destroys overlay windows so there is one per display.
fn sync<R: Runtime>(app: &AppHandle<R>, displays: &[u32]) -> tauri::Result<()> {
    let windows = app.webview_windows();
    let d = diff(windows.keys().map(String::as_str), displays);
    if d.create.is_empty() && d.destroy.is_empty() {
        return Ok(());
    }
    log::info!(
        "overlay windows: create {:?}, destroy {:?}",
        d.create,
        d.destroy
    );
    for label in &d.destroy {
        if let Some(w) = windows.get(label) {
            w.destroy()?;
        }
    }
    for id in d.create {
        build(app, id)?;
    }
    Ok(())
}

fn build<R: Runtime>(app: &AppHandle<R>, display: u32) -> tauri::Result<WebviewWindow<R>> {
    let url = WebviewUrl::App("overlay.html".into());
    let builder = WebviewWindowBuilder::new(app, label(display), url)
        .title("xcreenshot")
        .visible(false)
        .focused(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true);
    // The first click on the overlay starts the selection instead of only focusing it.
    #[cfg(target_os = "macos")]
    let builder = builder.accept_first_mouse(true);
    let window = builder.build()?;
    // The mock runtime used in tests has no real NSWindow.
    #[cfg(all(target_os = "macos", not(test)))]
    macos::raise(&window);
    Ok(window)
}

/// Window level and Spaces: `always_on_top` alone stays under the menu bar and the Dock.
#[cfg(all(target_os = "macos", not(test)))]
mod macos {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreenSaverWindowLevel, NSWindow, NSWindowCollectionBehavior};
    use tauri::{Runtime, WebviewWindow};

    pub fn raise<R: Runtime>(window: &WebviewWindow<R>) {
        if MainThreadMarker::new().is_none() {
            return log::error!("overlay window level must be set on the main thread");
        }
        let ptr = match window.ns_window() {
            Ok(ptr) => ptr,
            Err(e) => return log::error!("no NSWindow for {}: {e}", window.label()),
        };
        // SAFETY: Tauri returns the live NSWindow of this window, and we are on the main thread.
        let ns_window = unsafe { &*ptr.cast::<NSWindow>() };
        // Above the menu bar (24), the Dock (20) and pop-up menus (101).
        ns_window.setLevel(NSScreenSaverWindowLevel);
        // On every Space, and over full-screen apps.
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
    }
}

/// The calling window's frame, packed by [`pack_frame`]; empty if there is none.
fn frame_bytes(state: &Overlay, window_label: &str) -> Vec<u8> {
    let Some(id) = display_id(window_label) else {
        return Vec::new();
    };
    let session = lock(&state.session);
    let Some(session) = session.as_ref() else {
        return Vec::new();
    };
    let Some(frame) = session.frames.iter().find(|f| f.id == id) else {
        return Vec::new();
    };
    let bytes = pack_frame(frame);
    log::info!(
        "{window_label}: frame ready {:.1} ms after the hotkey",
        ms(session.started)
    );
    bytes
}

/// Raw frame bytes for the calling overlay window. Async, so it runs off the main thread and
/// several displays fetch at once.
#[tauri::command]
pub(crate) async fn overlay_frame<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Overlay>,
) -> Result<Response, ()> {
    Ok(Response::new(frame_bytes(&state, window.label())))
}

/// The overlay drew its frame. Logs hotkey-to-painted time.
#[tauri::command]
pub(crate) fn overlay_painted<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Overlay>,
    fetch_ms: f64,
    draw_ms: f64,
) {
    if let Some(session) = lock(&state.session).as_ref() {
        log::info!(
            "{}: painted {:.1} ms after the hotkey (fetch {fetch_ms:.1} ms, draw {draw_ms:.1} ms)",
            window.label(),
            ms(session.started)
        );
    }
}

#[tauri::command]
pub(crate) fn close_overlay<R: Runtime>(app: AppHandle<R>) {
    close(&app);
}

#[cfg(test)]
mod tests {
    use super::super::tests::frame;
    use super::*;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    fn app() -> tauri::App<MockRuntime> {
        mock_builder()
            .manage(Overlay::default())
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    fn overlay_labels(app: &AppHandle<MockRuntime>) -> Vec<String> {
        let mut labels: Vec<_> = app
            .webview_windows()
            .into_keys()
            .filter(|l| display_id(l).is_some())
            .collect();
        labels.sort();
        labels
    }

    /// Presses the hotkey with fake displays.
    fn open(app: &AppHandle<MockRuntime>, ids: &[u32]) {
        capture_with(app, || Ok(ids.iter().map(|&id| frame(id, 4, 2)).collect()));
        assert!(app.state::<Overlay>().guard.is_open());
    }

    #[test]
    fn show_makes_one_window_per_display_and_keeps_the_frames() {
        let app = app();
        let h = app.handle();
        open(h, &[1, 2]);
        assert_eq!(overlay_labels(h), ["overlay-1", "overlay-2"]);
        let state = h.state::<Overlay>();
        assert_eq!(
            frame_bytes(&state, "overlay-2"),
            pack_frame(&frame(2, 4, 2))
        );
    }

    #[test]
    fn hotkey_is_ignored_while_the_overlay_is_open() {
        let app = app();
        let h = app.handle();
        open(h, &[1]);
        // The second press returns before capturing; the first press's frames stay.
        capture_with(h, || panic!("captured while the overlay was open"));
        start_capture(h);
        let state = h.state::<Overlay>();
        assert!(state.guard.is_open());
        assert_eq!(
            frame_bytes(&state, "overlay-1"),
            pack_frame(&frame(1, 4, 2))
        );
    }

    #[test]
    fn failed_capture_shows_nothing_and_allows_a_retry() {
        let app = app();
        let h = app.handle();
        capture_with(h, || Err(CaptureError::PermissionDenied));
        let state = h.state::<Overlay>();
        assert!(!state.guard.is_open());
        assert!(overlay_labels(h).is_empty());
        open(h, &[1]);
    }

    #[test]
    fn prewarm_creates_a_hidden_window_per_display() {
        let app = app();
        let h = app.handle();
        prewarm_with(h, Err(CaptureError::NoDisplays));
        assert!(overlay_labels(h).is_empty());
        prewarm_with(h, Ok(vec![4, 5]));
        assert_eq!(overlay_labels(h), ["overlay-4", "overlay-5"]);
        assert!(!h.state::<Overlay>().guard.is_open());
    }

    #[test]
    fn close_frees_the_frames_and_allows_the_next_capture() {
        let app = app();
        let h = app.handle();
        open(h, &[1]);
        close(h);
        let state = h.state::<Overlay>();
        assert!(!state.guard.is_open());
        assert!(frame_bytes(&state, "overlay-1").is_empty());
        // The windows stay, hidden, for the next capture.
        assert_eq!(overlay_labels(h), ["overlay-1"]);
        close(h);
        assert!(!state.guard.is_open());
    }

    #[test]
    fn display_changes_rebuild_the_windows() {
        let app = app();
        let h = app.handle();
        WebviewWindowBuilder::new(h, "settings", WebviewUrl::default())
            .build()
            .unwrap();
        sync(h, &[1, 2]).unwrap();
        assert_eq!(overlay_labels(h), ["overlay-1", "overlay-2"]);
        open(h, &[2, 3]);
        // The mock runtime never reports a window destroyed, so overlay-1 stays listed here;
        // which windows get destroyed is covered by the `diff` tests.
        assert!(overlay_labels(h).contains(&"overlay-3".to_owned()));
        assert!(h.get_webview_window("settings").is_some());
        close(h);
    }

    #[test]
    fn frame_bytes_is_empty_without_a_matching_frame() {
        let app = app();
        let h = app.handle();
        let state = h.state::<Overlay>();
        assert!(frame_bytes(&state, "overlay-1").is_empty());
        open(h, &[1]);
        assert!(frame_bytes(&state, "overlay-7").is_empty());
        assert!(frame_bytes(&state, "settings").is_empty());
    }

    #[test]
    fn commands_serve_the_calling_window() {
        let app = app();
        let h = app.handle();
        open(h, &[1]);
        let window = h.get_webview_window("overlay-1").unwrap();
        let response =
            tauri::async_runtime::block_on(overlay_frame(window.clone(), h.state::<Overlay>()));
        assert!(response.is_ok());
        overlay_painted(window, h.state::<Overlay>(), 12.0, 3.0);
        close_overlay(h.clone());
        assert!(!h.state::<Overlay>().guard.is_open());
    }

    #[test]
    fn closing_an_overlay_window_closes_the_overlay() {
        let app = app();
        let h = app.handle();
        open(h, &[1]);
        let overlay = h.get_webview_window("overlay-1").unwrap().as_ref().window();
        assert!(close_requested(&overlay));
        assert!(!h.state::<Overlay>().guard.is_open());
        let settings = WebviewWindowBuilder::new(h, "settings", WebviewUrl::default())
            .build()
            .unwrap();
        assert!(!close_requested(&settings.as_ref().window()));
    }
}
