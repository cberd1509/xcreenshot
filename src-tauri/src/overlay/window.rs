//! Tauri glue for the overlay.
//!
//! Hotkey (main thread): claim the [`Phase`], register Esc, start the capture on a worker
//! thread and place the hidden windows while it runs. Capture done (worker): open the session
//! and send `overlay:show` with each frame's size. Each window fetches its frame
//! (`overlay_frame`: the raw RGBA buffer, no copy), draws it and calls `overlay_painted`, which
//! shows that window. Esc, `close_overlay`, closing a window or the watchdog hide everything.
//! Window and shortcut calls stay on the main thread.

use super::{FrameInfo, Phase, diff, display_id, label};
use crate::capture::{self, Bounds, CaptureError, Display, DisplayFrame};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tauri::ipc::Response;
use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcut, ShortcutState};

const SHOW_EVENT: &str = "overlay:show";
const ESCAPE: &str = "Escape";
/// If no window paints within this time the overlay page is broken: close everything, so a
/// hung page never locks the user out.
const WATCHDOG: Duration = Duration::from_secs(2);

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

/// Overlay state, managed by the app.
#[derive(Default)]
pub struct Overlay {
    phase: Mutex<Phase>,
}

impl Overlay {
    fn phase(&self) -> MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn phase<R: Runtime>(app: &AppHandle<R>) -> MutexGuard<'_, Phase> {
    app.state::<Overlay>().inner().phase()
}

/// The OS side of a capture: permission, listing displays and grabbing them. Faked in tests.
pub(crate) trait Screen: Send + 'static {
    /// Whether capture is allowed. May show the system prompt.
    fn permitted(&self) -> bool;
    fn displays(&self) -> Result<Vec<Display>, CaptureError>;
    fn capture(&self, ids: &[u32]) -> Result<Vec<DisplayFrame>, CaptureError>;
    /// The cursor in the displays' OS units (points on macOS, pixels on Windows).
    fn cursor<R: Runtime>(&self, app: &AppHandle<R>) -> Option<(f64, f64)>;
}

/// The real screen, through `capture`.
struct Os;

impl Screen for Os {
    fn permitted(&self) -> bool {
        if capture::has_permission() {
            return true;
        }
        // TODO(#23): show the permission screen instead of only the system prompt.
        let granted = capture::request_permission();
        log::warn!("Screen Recording permission requested (granted: {granted})");
        false
    }

    fn displays(&self) -> Result<Vec<Display>, CaptureError> {
        capture::displays()
    }

    fn capture(&self, ids: &[u32]) -> Result<Vec<DisplayFrame>, CaptureError> {
        capture::capture_displays(ids)
    }

    fn cursor<R: Runtime>(&self, app: &AppHandle<R>) -> Option<(f64, f64)> {
        let p = app.cursor_position().ok()?;
        // macOS reports points times the primary display's scale.
        let scale = if cfg!(target_os = "macos") {
            app.primary_monitor().ok()??.scale_factor()
        } else {
            1.0
        };
        Some((p.x / scale, p.y / scale))
    }
}

/// Creates a hidden overlay window per display at startup, so the hotkey only has to show them.
pub fn prewarm<R: Runtime>(app: &AppHandle<R>) {
    prewarm_with(app, &Os);
}

fn prewarm_with<R: Runtime>(app: &AppHandle<R>, screen: &impl Screen) {
    let result = screen.displays().map(|d| sync(app, &ids(&d)));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::error!("could not create overlay windows: {e}"),
        Err(e) => log::error!("could not list displays for the overlay: {e}"),
    }
}

/// Hotkey handler (main thread). Returns the session id if a capture started.
pub fn start_capture<R: Runtime>(app: &AppHandle<R>) -> Option<u64> {
    start_with(app, Os)
}

/// Claims the phase, starts the capture on a worker thread, then registers Esc and places the
/// hidden windows while it runs.
fn start_with<R: Runtime>(app: &AppHandle<R>, screen: impl Screen) -> Option<u64> {
    let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    if !phase(app).begin(session, Instant::now()) {
        log::info!("capture ignored: the overlay is already open");
        return None;
    }
    let displays = match screen.permitted().then(|| screen.displays()) {
        Some(Ok(d)) => d,
        Some(Err(e)) => {
            log::error!("could not list displays: {e}");
            *phase(app) = Phase::Idle;
            return None;
        }
        None => {
            *phase(app) = Phase::Idle;
            return None;
        }
    };
    let worker = app.clone();
    let ids = ids(&displays);
    std::thread::spawn(move || {
        let frames = screen.capture(&ids);
        let cursor = screen.cursor(&worker);
        finish(&worker, session, frames, cursor, WATCHDOG);
    });
    set_escape(app, true);
    if let Err(e) = place(app, &displays) {
        log::error!("could not place the overlay windows: {e}");
        close(app);
        return None;
    }
    Some(session)
}

/// Capture finished (worker thread). Opens the session and tells each window to draw.
fn finish<R: Runtime>(
    app: &AppHandle<R>,
    session: u64,
    frames: Result<Vec<DisplayFrame>, CaptureError>,
    cursor: Option<(f64, f64)>,
    watchdog: Duration,
) {
    let frames = match frames {
        Ok(frames) => frames,
        Err(e) => {
            log::error!("capture failed: {e}");
            return on_main(app, move |app| close_session(app, session));
        }
    };
    // `None`: Esc closed the overlay during the capture.
    let Some(shows) = phase(app).open(session, frames, cursor) else {
        return;
    };
    for (display, info) in shows {
        if let Err(e) = app.emit_to(label(display).as_str(), SHOW_EVENT, info) {
            log::warn!("could not tell overlay {display} to draw: {e}");
        }
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(watchdog);
        on_main(&app, move |app| rescue(app, session));
    });
}

fn on_main<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(&AppHandle<R>) + Send + 'static) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || f(&handle)) {
        log::error!("could not run on the main thread: {e}");
    }
}

/// Watchdog: nothing painted, so the overlay page is broken. Close instead of hanging.
fn rescue<R: Runtime>(app: &AppHandle<R>, session: u64) {
    if phase(app).stalled(session) {
        log::warn!("overlay did not paint within {WATCHDOG:?}; closing it");
        close(app);
    }
}

/// Closes the overlay only if `session` is still the current one.
pub fn close_session<R: Runtime>(app: &AppHandle<R>, session: u64) {
    let current = phase(app).session();
    if current == Some(session) {
        close(app);
    }
}

/// Hides every overlay, frees the frames, releases Esc and allows the next capture.
/// Main thread only. Safe to call twice.
pub fn close<R: Runtime>(app: &AppHandle<R>) {
    *phase(app) = Phase::Idle;
    set_escape(app, false);
    let windows = app.webview_windows();
    for (label, window) in windows.iter().filter(|(l, _)| display_id(l).is_some()) {
        if let Err(e) = window.hide() {
            // A window left on screen would block input: remove it; `sync` rebuilds it.
            log::warn!("could not hide {label}, destroying it: {e}");
            let _ = window.destroy();
        }
    }
    // Give focus back to the app the user was in, unless Settings is open.
    #[cfg(target_os = "macos")]
    if !windows
        .iter()
        .any(|(l, w)| display_id(l).is_none() && w.is_visible().unwrap_or(false))
    {
        let _ = app.hide();
    }
}

/// Esc belongs to the app while the overlay is open, so it works even if the page is broken.
fn set_escape<R: Runtime>(app: &AppHandle<R>, on: bool) {
    // Missing only in tests, where the plugin is not installed.
    let Some(shortcuts) = app.try_state::<GlobalShortcut<R>>() else {
        return;
    };
    let result = if on {
        shortcuts.on_shortcut(ESCAPE, |app, _, event| {
            if event.state() == ShortcutState::Pressed {
                close(app);
            }
        })
    } else if shortcuts.is_registered(ESCAPE) {
        shortcuts.unregister(ESCAPE)
    } else {
        Ok(())
    };
    if let Err(e) = result {
        log::warn!("could not update the Esc shortcut: {e}");
    }
}

fn ids(displays: &[Display]) -> Vec<u32> {
    displays.iter().map(|d| d.id).collect()
}

/// Matches the windows to the displays and moves each (still hidden) over its display.
fn place<R: Runtime>(app: &AppHandle<R>, displays: &[Display]) -> tauri::Result<()> {
    sync(app, &ids(displays))?;
    for d in displays {
        if let Some(window) = app.get_webview_window(&label(d.id)) {
            position(&window, d.bounds)?;
        }
    }
    Ok(())
}

fn position<R: Runtime>(window: &WebviewWindow<R>, b: Bounds) -> tauri::Result<()> {
    if cfg!(target_os = "windows") {
        // Pixels. Move first: Windows rescales a window that moves to a monitor with another DPI.
        window.set_position(PhysicalPosition::new(b.x, b.y))?;
        window.set_size(PhysicalSize::new(b.width, b.height))
    } else {
        // Points. macOS resizes from the bottom-left corner, so size first, then move.
        window.set_size(LogicalSize::new(b.width, b.height))?;
        window.set_position(LogicalPosition::new(b.x, b.y))
    }
}

/// Creates and destroys overlay windows so there is one per display. Main thread only, so
/// startup and the hotkey never build the same window twice.
fn sync<R: Runtime>(app: &AppHandle<R>, displays: &[u32]) -> tauri::Result<()> {
    let windows = app.webview_windows();
    let d = diff(windows.keys().map(String::as_str), displays);
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
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .background_color(tauri::window::Color(0, 0, 0, 255))
        // The page draws while its window is hidden; WebKit would otherwise throttle it.
        .background_throttling(BackgroundThrottlingPolicy::Disabled);
    // The first click on the overlay starts the selection instead of only focusing it.
    #[cfg(target_os = "macos")]
    let builder = builder.accept_first_mouse(true);
    let window = builder.build()?;
    // A user close (Alt+F4) closes the overlay and keeps the window for the next capture.
    // Quitting the app does not send CloseRequested, so it is never blocked.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            close(&handle);
        }
    });
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
        // Over full-screen apps too. `visible_on_all_workspaces` already set CanJoinAllSpaces.
        ns_window.setCollectionBehavior(
            ns_window.collectionBehavior() | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
    }
}

/// The calling window's frame info, if its frame is waiting. Called by a window that loads
/// after `overlay:show` was sent (a display plugged in during the capture).
#[tauri::command]
pub(crate) fn overlay_ready<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Overlay>,
) -> Option<FrameInfo> {
    state.phase().pending_frame(display_id(window.label())?)
}

/// The calling window's frame as raw RGBA, moved out of the session (no copy); empty if it
/// was already taken or the session is gone. Async, so it runs off the main thread.
#[tauri::command]
pub(crate) async fn overlay_frame<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Overlay>,
    session: u64,
) -> Result<Response, ()> {
    let image = display_id(window.label()).and_then(|d| state.phase().take_frame(session, d));
    Ok(Response::new(
        image.map(|i| i.into_raw()).unwrap_or_default(),
    ))
}

/// The window drew its frame: show it (only if its session is still open), focus the
/// display under the cursor, and log the time on the first paint.
#[tauri::command]
pub(crate) fn overlay_painted<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Overlay>,
    session: u64,
) {
    let Some(display) = display_id(window.label()) else {
        return;
    };
    let Some(paint) = state.phase().painted(session, display) else {
        return;
    };
    let app = window.app_handle();
    if let Err(e) = window.show() {
        log::error!("could not show the overlay: {e}");
        return close(app);
    }
    if let Some(focus) = paint.focus.and_then(|f| app.get_webview_window(&label(f))) {
        let _ = focus.set_focus();
    }
    if let Some((total, capture)) = paint.first {
        log::info!("overlay painted {total:.0} ms after the hotkey (capture {capture:.0} ms)");
    }
}

#[tauri::command]
pub(crate) fn close_overlay<R: Runtime>(app: AppHandle<R>) {
    close(&app);
}

#[cfg(test)]
mod tests {
    use super::super::os_bounds;
    use super::super::tests::frame;
    use super::*;
    use std::sync::mpsc;
    use tauri::ipc::{InvokeResponseBody, IpcResponse};
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    type Handle = AppHandle<MockRuntime>;

    /// Displays `ids`. Its capture waits for the test: `true` captures, `false` fails.
    struct Fake {
        permitted: bool,
        ids: Vec<u32>,
        release: mpsc::Receiver<bool>,
    }

    impl Screen for Fake {
        fn permitted(&self) -> bool {
            self.permitted
        }

        fn displays(&self) -> Result<Vec<Display>, CaptureError> {
            if self.ids.is_empty() {
                return Err(CaptureError::NoDisplays);
            }
            Ok(self
                .ids
                .iter()
                .map(|&id| Display {
                    id,
                    bounds: os_bounds(&frame(id, 4, 2)),
                })
                .collect())
        }

        fn capture(&self, ids: &[u32]) -> Result<Vec<DisplayFrame>, CaptureError> {
            match self.release.recv() {
                Ok(true) => Ok(ids.iter().map(|&id| frame(id, 4, 2)).collect()),
                _ => Err(CaptureError::EmptyImage),
            }
        }

        fn cursor<R: Runtime>(&self, _: &AppHandle<R>) -> Option<(f64, f64)> {
            None
        }
    }

    fn fake(permitted: bool, ids: &[u32]) -> (Fake, mpsc::Sender<bool>) {
        let (tx, release) = mpsc::channel();
        let ids = ids.to_vec();
        (
            Fake {
                permitted,
                ids,
                release,
            },
            tx,
        )
    }

    fn app() -> tauri::App<MockRuntime> {
        mock_builder()
            .manage(Overlay::default())
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    fn overlay_labels(app: &Handle) -> Vec<String> {
        let mut labels: Vec<_> = app
            .webview_windows()
            .into_keys()
            .filter(|l| display_id(l).is_some())
            .collect();
        labels.sort();
        labels
    }

    fn wait_until(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn is_open(app: &Handle, session: u64) -> bool {
        matches!(&*phase(app), Phase::Open(_)) && phase(app).session() == Some(session)
    }

    fn is_idle(app: &Handle) -> bool {
        matches!(*phase(app), Phase::Idle)
    }

    /// Presses the hotkey on fake displays; the capture finishes when `release` is sent.
    fn press(app: &Handle, ids: &[u32]) -> (u64, mpsc::Sender<bool>) {
        let (screen, release) = fake(true, ids);
        (start_with(app, screen).unwrap(), release)
    }

    /// Presses the hotkey and lets the capture finish.
    fn open(app: &Handle, ids: &[u32]) -> u64 {
        let (session, release) = press(app, ids);
        release.send(true).unwrap();
        wait_until("the session to open", || is_open(app, session));
        session
    }

    fn window(app: &Handle, display: u32) -> WebviewWindow<MockRuntime> {
        app.get_webview_window(&label(display)).unwrap()
    }

    fn fetch(app: &Handle, display: u32, session: u64) -> Vec<u8> {
        let state = app.state::<Overlay>();
        let response =
            tauri::async_runtime::block_on(overlay_frame(window(app, display), state, session));
        match response.unwrap().body().unwrap() {
            InvokeResponseBody::Raw(bytes) => bytes,
            InvokeResponseBody::Json(json) => panic!("expected raw bytes, got {json}"),
        }
    }

    #[test]
    fn prewarm_creates_a_window_per_display() {
        let app = app();
        let h = app.handle();
        prewarm_with(h, &fake(true, &[]).0);
        assert!(overlay_labels(h).is_empty());
        prewarm_with(h, &fake(true, &[4, 5]).0);
        assert_eq!(overlay_labels(h), ["overlay-4", "overlay-5"]);
        assert!(is_idle(h));
    }

    #[test]
    fn hotkey_places_a_window_per_display_then_sends_frames() {
        let app = app();
        let h = app.handle();
        let (session, release) = press(h, &[1, 2]);
        // Windows are placed while the capture runs; nothing to draw yet.
        assert_eq!(overlay_labels(h), ["overlay-1", "overlay-2"]);
        assert_eq!(overlay_ready(window(h, 1), h.state()), None);
        release.send(true).unwrap();
        wait_until("the session to open", || is_open(h, session));
        let info = overlay_ready(window(h, 2), h.state()).unwrap();
        assert_eq!((info.session, info.width, info.height), (session, 4, 2));
        assert_eq!(fetch(h, 2, session), frame(2, 4, 2).image.into_raw());
        // Handed over once; a second fetch (load check and show both firing) gets nothing.
        assert!(fetch(h, 2, session).is_empty());
        assert_eq!(overlay_ready(window(h, 2), h.state()), None);
        assert!(fetch(h, 1, session + 1).is_empty(), "other session");
    }

    #[test]
    fn hotkey_is_ignored_while_the_overlay_is_open() {
        let app = app();
        let h = app.handle();
        let (session, release) = press(h, &[1]);
        assert_eq!(start_with(h, fake(true, &[1]).0), None, "while capturing");
        release.send(true).unwrap();
        wait_until("the session to open", || is_open(h, session));
        assert_eq!(start_with(h, fake(true, &[1]).0), None, "while open");
        assert_eq!(phase(h).session(), Some(session));
    }

    #[test]
    fn no_permission_or_no_displays_captures_nothing() {
        let app = app();
        let h = app.handle();
        assert_eq!(start_with(h, fake(false, &[1]).0), None);
        assert!(is_idle(h));
        assert!(overlay_labels(h).is_empty(), "no window before permission");
        assert_eq!(start_with(h, fake(true, &[]).0), None);
        assert!(is_idle(h));
    }

    #[test]
    fn a_failed_capture_closes_the_overlay() {
        let app = app();
        let h = app.handle();
        let (_, release) = press(h, &[1]);
        release.send(false).unwrap();
        wait_until("the overlay to close", || is_idle(h));
        open(h, &[1]);
    }

    #[test]
    fn a_failure_from_an_older_capture_is_ignored() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1]);
        let failed = Err(CaptureError::NoDisplays);
        finish(h, session + 100, failed, None, WATCHDOG);
        assert!(is_open(h, session));
    }

    #[test]
    fn painting_shows_only_the_current_session() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1, 2]);
        overlay_painted(window(h, 1), h.state(), session);
        assert!(!phase(h).stalled(session));
        close_overlay(h.clone());
        // A late paint from the closed session must not reopen anything.
        overlay_painted(window(h, 2), h.state(), session);
        assert!(is_idle(h));
    }

    #[test]
    fn closing_frees_the_frames_and_allows_the_next_capture() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1]);
        close(h);
        assert!(fetch(h, 1, session).is_empty());
        // The windows stay, hidden, for the next capture.
        assert_eq!(overlay_labels(h), ["overlay-1"]);
        open(h, &[1]);
    }

    #[test]
    fn a_capture_finished_after_esc_shows_nothing() {
        let app = app();
        let h = app.handle();
        let (session, release) = press(h, &[1]);
        close(h);
        release.send(true).unwrap();
        // The worker finishes on its own; give it time, then check nothing opened.
        std::thread::sleep(Duration::from_millis(50));
        assert!(is_idle(h));
        assert_eq!(overlay_ready(window(h, 1), h.state()), None);
        close_session(h, session);
        assert!(is_idle(h));
    }

    #[test]
    fn close_session_leaves_a_newer_session_alone() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1]);
        close_session(h, session + 1);
        assert!(is_open(h, session));
        close_session(h, session);
        assert!(is_idle(h));
    }

    #[test]
    fn watchdog_closes_an_overlay_that_never_painted() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1, 2]);
        rescue(h, session + 1);
        assert!(is_open(h, session), "other session");
        rescue(h, session);
        assert!(is_idle(h));

        let session = open(h, &[1]);
        overlay_painted(window(h, 1), h.state(), session);
        rescue(h, session);
        assert!(is_open(h, session), "painted in time");
    }

    #[test]
    fn display_changes_rebuild_the_windows() {
        let app = app();
        let h = app.handle();
        WebviewWindowBuilder::new(h, "settings", WebviewUrl::default())
            .build()
            .unwrap();
        sync(h, &[1, 2]).unwrap();
        sync(h, &[2, 1]).unwrap();
        assert_eq!(overlay_labels(h), ["overlay-1", "overlay-2"]);
        open(h, &[2, 3]);
        // The mock runtime never reports a window destroyed, so overlay-1 stays listed here;
        // which windows get destroyed is covered by the `diff` tests.
        assert!(overlay_labels(h).contains(&"overlay-3".to_owned()));
        assert!(h.get_webview_window("settings").is_some());
    }

    #[test]
    fn esc_is_held_only_while_the_overlay_is_open() {
        let app = mock_builder()
            .plugin(tauri_plugin_global_shortcut::Builder::new().build())
            .manage(Overlay::default())
            .build(mock_context(noop_assets()))
            .unwrap();
        let h = app.handle();
        let shortcuts = h.state::<GlobalShortcut<MockRuntime>>();
        let session = open(h, &[1]);
        assert!(shortcuts.is_registered(ESCAPE));
        close_session(h, session);
        assert!(!shortcuts.is_registered(ESCAPE));
    }

    #[test]
    fn calls_from_other_windows_are_ignored() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1]);
        let settings = WebviewWindowBuilder::new(h, "settings", WebviewUrl::default())
            .build()
            .unwrap();
        assert_eq!(overlay_ready(settings.clone(), h.state()), None);
        overlay_painted(settings, h.state(), session);
        assert!(phase(h).stalled(session));
    }
}
