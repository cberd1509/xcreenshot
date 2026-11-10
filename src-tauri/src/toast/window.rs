//! Tauri glue for the toasts: the window, the sound and the page's two commands.
//!
//! [`show`] builds the window if needed, moves it to the bottom of the display the capture was
//! on, plays the sound and sends the toast (`toast:show`), or keeps it until the page asks for
//! it (`toast_ready`). The page calls `toast_idle` when its last toast has faded; the window
//! then hides. A capture hides it at once ([`hide_for_capture`]), so no toast is in the frame.

use super::{LABEL, Queue, SIZE, Shown, Toast, place, sound};
use crate::capture::Bounds;
use std::sync::{Mutex, MutexGuard, PoisonError};
use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{
    AppHandle, Emitter, Manager, Runtime, State, Webview, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const SHOW_EVENT: &str = "toast:show";
/// A capture started: the page drops its toasts.
const CLEAR_EVENT: &str = "toast:clear";

/// The toasts' state, managed by the app.
#[derive(Default)]
pub struct Toasts(Mutex<Queue>);

impl Toasts {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Shows `toast` at the bottom of the display `anchor` covers (an overlay window), with the
/// screenshot sound. Does nothing unless the app manages [`Toasts`]. Main thread only.
pub fn show<R: Runtime>(anchor: &WebviewWindow<R>, toast: Toast) {
    let app = anchor.app_handle();
    let Some(toasts) = app.try_state::<Toasts>() else {
        return;
    };
    let window = match window(app) {
        Ok(window) => window,
        Err(e) => return log::error!("could not open the toast window: {e}"),
    };
    if let Some((display, scale)) = display_of(anchor)
        && let Err(e) = crate::overlay::window::position(&window, place(display, scale))
    {
        log::warn!("could not place the toasts: {e}");
    }
    // `None`: the page is still loading; it asks for the toast when ready.
    let sent = toasts.queue().push(toast);
    // Not if the window failed: no toast, no sound.
    sound::play();
    let Some(shown) = sent else {
        return;
    };
    present(&window);
    if let Err(e) = app.emit_to(LABEL, SHOW_EVENT, &shown) {
        log::warn!("could not show the toast: {e}");
        toasts.queue().undelivered(shown);
        conceal(&window);
        // The page only asks for missed toasts when it loads: reload it so it asks again.
        if let Err(e) = window.reload() {
            log::warn!("could not reload the toasts: {e}");
        }
    }
}

/// Wired to every page load: when the toast page starts loading (again), toasts wait until it
/// listens, so none is sent to a page that can't get it.
pub fn on_page_load<R: Runtime>(webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
    if webview.label() == LABEL && payload.event() == PageLoadEvent::Started {
        reloading(webview.app_handle());
    }
}

fn reloading<R: Runtime>(app: &AppHandle<R>) {
    let Some(toasts) = app.try_state::<Toasts>() else {
        return;
    };
    if toasts.queue().reloading()
        && let Some(window) = app.get_webview_window(LABEL)
    {
        conceal(&window);
    }
}

/// A capture is starting: hides the toasts, so none is in the frame. Costs nothing when none is
/// on screen. Main thread only.
pub fn hide_for_capture<R: Runtime>(app: &AppHandle<R>) {
    let Some(toasts) = app.try_state::<Toasts>() else {
        return;
    };
    if !toasts.queue().clear() {
        return;
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        conceal(&window);
    }
    if let Err(e) = app.emit_to(LABEL, CLEAR_EVENT, ()) {
        log::warn!("could not clear the toasts: {e}");
    }
}

/// The display `window` covers, in its OS units (points on macOS, pixels on Windows), and how
/// many of them make a logical px.
fn display_of<R: Runtime>(window: &WebviewWindow<R>) -> Option<(Bounds, f64)> {
    let scale = window.scale_factor().ok()?;
    let at = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    if cfg!(target_os = "windows") {
        let bounds = Bounds {
            x: at.x,
            y: at.y,
            width: size.width,
            height: size.height,
        };
        return Some((bounds, scale));
    }
    let at = at.to_logical::<f64>(scale);
    let size = size.to_logical::<f64>(scale);
    let bounds = Bounds {
        x: at.x.round() as i32,
        y: at.y.round() as i32,
        width: size.width.round() as u32,
        height: size.height.round() as u32,
    };
    Some((bounds, 1.0))
}

/// Builds the hidden toast window at startup, so the first toast never builds a webview on the
/// main thread between two captures.
pub fn prewarm<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = window(app) {
        log::error!("could not open the toast window: {e}");
    }
}

/// The toast window, built hidden if it isn't yet. It can't take focus or clicks.
fn window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let url = WebviewUrl::App("toast.html".into());
    let window = WebviewWindowBuilder::new(app, LABEL, url)
        .title("xcreenshot")
        .inner_size(f64::from(SIZE.0), f64::from(SIZE.1))
        .visible(false)
        .focused(false)
        .focusable(false)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        // It gets toasts while hidden; WebKit would otherwise throttle it.
        .background_throttling(BackgroundThrottlingPolicy::Disabled)
        .build()?;
    window.set_ignore_cursor_events(true)?;
    #[cfg(all(target_os = "macos", not(test)))]
    native::prepare(&window);
    Ok(window)
}

/// Puts the window on screen without activating the app or focusing it.
#[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
fn present<R: Runtime>(window: &WebviewWindow<R>) {
    native::present(window);
}

#[cfg(not(all(any(target_os = "macos", target_os = "windows"), not(test))))]
fn present<R: Runtime>(window: &WebviewWindow<R>) {
    if let Err(e) = window.show() {
        log::warn!("could not show the toasts: {e}");
    }
}

/// Takes the window off screen.
#[cfg(all(target_os = "windows", not(test)))]
fn conceal<R: Runtime>(window: &WebviewWindow<R>) {
    native::conceal(window);
}

#[cfg(not(all(target_os = "windows", not(test))))]
fn conceal<R: Runtime>(window: &WebviewWindow<R>) {
    if let Err(e) = window.hide() {
        log::warn!("could not hide the toasts: {e}");
    }
}

/// The toasts sent before the page listened. Shows the window if there are any.
#[tauri::command]
pub(crate) fn toast_ready<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, Toasts>,
) -> Vec<Shown> {
    if window.label() != LABEL {
        return Vec::new();
    }
    let missed = state.queue().ready();
    if !missed.is_empty() {
        present(&window);
    }
    missed
}

/// The page's last toast faded, the newest it got being `seq`: hides the window, unless a
/// newer toast is on its way.
#[tauri::command]
pub(crate) fn toast_idle<R: Runtime>(window: WebviewWindow<R>, state: State<'_, Toasts>, seq: u64) {
    if window.label() == LABEL && state.queue().idle(seq) {
        conceal(&window);
    }
}

/// Window level, Spaces and hiding: what Tauri's builder can't set.
#[cfg(all(target_os = "macos", not(test)))]
mod native {
    use crate::macos::main_ns_window;
    use objc2_app_kit::{NSStatusWindowLevel, NSWindowCollectionBehavior};
    use tauri::{Runtime, WebviewWindow};

    /// Above the Dock and the menu bar, on every Space and over full-screen apps, out of the
    /// window cycle, and still on screen while the app is hidden (closing the overlay hides the
    /// app to give focus back).
    pub fn prepare<R: Runtime>(window: &WebviewWindow<R>) {
        let Some(ns_window) = main_ns_window(window) else {
            return;
        };
        ns_window.setLevel(NSStatusWindowLevel);
        ns_window.setCollectionBehavior(
            ns_window.collectionBehavior()
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        ns_window.setCanHide(false);
    }

    /// Ordered front without becoming key or activating the app (Tauri's `show` makes it key).
    pub fn present<R: Runtime>(window: &WebviewWindow<R>) {
        if let Some(ns_window) = main_ns_window(window) {
            ns_window.orderFrontRegardless();
        }
    }
}

/// Showing without activating: Tauri's `show` activates the window.
#[cfg(all(target_os = "windows", not(test)))]
mod native {
    use tauri::{Runtime, WebviewWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        SHOW_WINDOW_CMD, SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow,
    };

    fn set<R: Runtime>(window: &WebviewWindow<R>, how: SHOW_WINDOW_CMD) {
        match window.hwnd() {
            // SAFETY: a live window handle from Tauri; ShowWindow only changes its visibility.
            Ok(hwnd) => unsafe {
                let _ = ShowWindow(hwnd, how);
            },
            Err(e) => log::warn!("no window handle for the toasts: {e}"),
        }
    }

    pub fn present<R: Runtime>(window: &WebviewWindow<R>) {
        set(window, SW_SHOWNOACTIVATE);
    }

    /// Also by hand, as Tauri doesn't know the window is shown.
    pub fn conceal<R: Runtime>(window: &WebviewWindow<R>) {
        set(window, SW_HIDE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Listener;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    fn app() -> tauri::App<MockRuntime> {
        mock_builder()
            .manage(Toasts::default())
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    fn anchor(app: &AppHandle<MockRuntime>) -> WebviewWindow<MockRuntime> {
        WebviewWindowBuilder::new(app, "overlay-1", WebviewUrl::default())
            .build()
            .unwrap()
    }

    /// The `toast:show` and `toast:clear` events, as JSON.
    fn record(app: &AppHandle<MockRuntime>) -> std::sync::Arc<Mutex<Vec<String>>> {
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        for event in [SHOW_EVENT, CLEAR_EVENT] {
            let seen = std::sync::Arc::clone(&seen);
            app.listen_any(event, move |e| {
                seen.lock()
                    .unwrap()
                    .push(format!("{event} {}", e.payload()));
            });
        }
        seen
    }

    fn copied(width: u32) -> Toast {
        Toast::Copied { width, height: 2 }
    }

    #[test]
    fn the_first_toast_waits_for_the_page_then_the_next_go_straight_to_it() {
        let app = app();
        let h = app.handle();
        let seen = record(h);
        let overlay = anchor(h);
        show(&overlay, copied(1));
        let window = h
            .get_webview_window(LABEL)
            .expect("built on the first toast");
        assert!(seen.lock().unwrap().is_empty(), "the page is loading");
        let missed = toast_ready(window.clone(), h.state());
        assert_eq!(
            missed,
            [Shown {
                seq: 1,
                toast: copied(1)
            }]
        );
        show(&overlay, copied(3));
        assert_eq!(
            *seen.lock().unwrap(),
            [r#"toast:show {"seq":2,"kind":"copied","width":3,"height":2}"#]
        );
        assert_eq!(h.webview_windows().len(), 2, "one toast window, reused");
        toast_idle(window, h.state(), 1);
        assert!(h.state::<Toasts>().queue().clear(), "toast 2 is still up");
    }

    #[test]
    fn prewarm_builds_the_window_once() {
        let app = app();
        let h = app.handle();
        prewarm(h);
        prewarm(h);
        assert!(h.get_webview_window(LABEL).is_some());
        assert_eq!(h.webview_windows().len(), 1);
    }

    #[test]
    fn idle_hides_the_window_once_nothing_newer_is_coming() {
        let app = app();
        let h = app.handle();
        let overlay = anchor(h);
        show(&overlay, copied(1));
        let window = h.get_webview_window(LABEL).unwrap();
        toast_ready(window.clone(), h.state());
        show(&overlay, copied(2));
        toast_idle(window, h.state(), 2);
        assert!(!h.state::<Toasts>().queue().clear(), "hidden");
    }

    #[test]
    fn a_reloading_page_gets_the_next_toast_when_it_asks() {
        let app = app();
        let h = app.handle();
        let seen = record(h);
        let overlay = anchor(h);
        reloading(h);
        show(&overlay, copied(1));
        let window = h.get_webview_window(LABEL).unwrap();
        toast_ready(window.clone(), h.state());
        reloading(h);
        show(&overlay, copied(2));
        assert!(
            seen.lock().unwrap().is_empty(),
            "not sent to a loading page"
        );
        let missed = toast_ready(window, h.state());
        assert_eq!(missed.iter().map(|s| s.seq).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn a_capture_hides_the_toasts_and_clears_the_page() {
        let app = app();
        let h = app.handle();
        let seen = record(h);
        hide_for_capture(h);
        assert!(seen.lock().unwrap().is_empty(), "nothing on screen");
        let overlay = anchor(h);
        show(&overlay, copied(1));
        toast_ready(h.get_webview_window(LABEL).unwrap(), h.state());
        hide_for_capture(h);
        assert_eq!(*seen.lock().unwrap(), ["toast:clear null"]);
    }

    #[test]
    fn other_windows_and_apps_without_toasts_are_ignored() {
        let app = app();
        let h = app.handle();
        let overlay = anchor(h);
        assert!(toast_ready(overlay.clone(), h.state()).is_empty());
        toast_idle(overlay.clone(), h.state(), 0);
        let bare = mock_builder().build(mock_context(noop_assets())).unwrap();
        let other = anchor(bare.handle());
        show(&other, copied(1));
        hide_for_capture(bare.handle());
        assert!(bare.handle().get_webview_window(LABEL).is_none());
    }
}
