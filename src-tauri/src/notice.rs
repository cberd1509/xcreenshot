//! The app's small prompts: "Update ready" and, on Windows, "Shortcut changed". Each is an
//! xcreenshot window sized to its card (`notice.html`) instead of a system alert, so it never
//! covers the capture overlay: it is hidden while a capture is open and shown again after.
//!
//! Rust keeps what each window shows and the answer it waits for. The page asks for its
//! content ([`notice`]) and sends back the button clicked ([`notice_answer`]).

use crate::print_screen::Reason;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::oneshot;

const PREFIX: &str = "notice-";

/// What a notice window shows. The page has the copy for each.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Notice {
    /// A downloaded update waits for a restart.
    UpdateReady { version: String },
    /// The capture shortcut moved off PrintScreen to `accelerator`.
    ShortcutChanged { reason: Reason, accelerator: String },
}

impl Notice {
    /// One window per kind.
    fn label(&self) -> &'static str {
        match self {
            Notice::UpdateReady { .. } => "notice-update",
            Notice::ShortcutChanged { .. } => "notice-shortcut",
        }
    }

    /// The card's size on the design board, in logical px.
    fn size(&self) -> (f64, f64) {
        match self {
            Notice::UpdateReady { .. } => (440.0, 200.0),
            Notice::ShortcutChanged { .. } => (440.0, 220.0),
        }
    }
}

/// An open notice and where its answer goes.
struct Pending {
    notice: Notice,
    answer: oneshot::Sender<bool>,
}

/// The open notices, by window label, and those hidden for a capture.
#[derive(Default)]
pub struct Notices {
    open: Mutex<HashMap<&'static str, Pending>>,
    hidden: Mutex<Vec<String>>,
}

impl Notices {
    /// Records `label` as hidden for a capture, once.
    fn hid(&self, label: &str) {
        let mut hidden = lock(&self.hidden);
        if !hidden.iter().any(|l| l == label) {
            hidden.push(label.to_owned());
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Shows `notice` and resolves to the button clicked: `Some(true)` for the main (right) one,
/// `Some(false)` for the other, `None` if the window closed another way.
pub async fn ask<R: Runtime>(app: &AppHandle<R>, notice: Notice) -> Option<bool> {
    let (answer, rx) = oneshot::channel();
    let label = notice.label();
    let size = notice.size();
    // Replaces an older one of the same kind: its `ask` resolves to `None`.
    lock(&app.state::<Notices>().open).insert(label, Pending { notice, answer });
    let handle = app.clone();
    let shown = app.run_on_main_thread(move || {
        if let Err(e) = open(&handle, label, size) {
            log::error!("could not open {label}: {e}");
            forget(&handle, label);
        }
    });
    if let Err(e) = shown {
        log::error!("could not open {label}: {e}");
        forget(app, label);
    }
    rx.await.ok()
}

/// Opens (or brings back) the window; hidden if a capture is open. Main thread only.
fn open<R: Runtime>(
    app: &AppHandle<R>,
    label: &'static str,
    size: (f64, f64),
) -> tauri::Result<()> {
    let busy = crate::overlay::is_busy(app);
    let window = match app.get_webview_window(label) {
        Some(window) => window,
        None => build(app, label, size, !busy)?,
    };
    if busy {
        window.hide()?;
        app.state::<Notices>().hid(label);
        return Ok(());
    }
    window.show()?;
    window.set_focus()
}

fn build<R: Runtime>(
    app: &AppHandle<R>,
    label: &'static str,
    (width, height): (f64, f64),
    visible: bool,
) -> tauri::Result<WebviewWindow<R>> {
    let url = WebviewUrl::App("notice.html".into());
    let builder = WebviewWindowBuilder::new(app, label, url)
        .title("xcreenshot")
        .inner_size(width, height)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .center()
        .visible(visible)
        .focused(visible);
    // The card is the whole window: no title bar, the system's corners, border and shadow.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    let window = builder.build()?;
    #[cfg(all(target_os = "macos", not(test)))]
    macos::hide_buttons(&window);
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            forget(&handle, label);
        }
    });
    Ok(window)
}

/// The window closed: its question is dropped, so `ask` resolves to `None`.
fn forget<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let notices = app.state::<Notices>();
    lock(&notices.open).remove(label);
    lock(&notices.hidden).retain(|l| l != label);
}

/// A capture is starting: hides the open notices. Main thread only.
pub fn hide_for_capture<R: Runtime>(app: &AppHandle<R>) {
    let Some(notices) = app.try_state::<Notices>() else {
        return;
    };
    for (label, window) in app.webview_windows() {
        if !label.starts_with(PREFIX) {
            continue;
        }
        match window.hide() {
            Ok(()) => notices.hid(&label),
            Err(e) => log::warn!("could not hide {label}: {e}"),
        }
    }
}

/// The capture closed: shows the notices it hid. Main thread only.
pub fn show_after_capture<R: Runtime>(app: &AppHandle<R>) {
    let Some(notices) = app.try_state::<Notices>() else {
        return;
    };
    let hidden = std::mem::take(&mut *lock(&notices.hidden));
    for label in hidden {
        let Some(window) = app.get_webview_window(&label) else {
            continue;
        };
        if let Err(e) = window.show().and_then(|()| window.set_focus()) {
            log::warn!("could not show {label}: {e}");
        }
    }
}

/// What the calling notice window shows; `None` once it was answered.
#[tauri::command]
pub(crate) fn notice<R: Runtime>(window: WebviewWindow<R>) -> Option<Notice> {
    let notices = window.state::<Notices>();
    let open = lock(&notices.open);
    open.get(window.label()).map(|p| p.notice.clone())
}

/// A button was clicked (`primary`: the main one): answers and closes the window.
#[tauri::command]
pub(crate) fn notice_answer<R: Runtime>(window: WebviewWindow<R>, primary: bool) {
    let pending = lock(&window.state::<Notices>().open).remove(window.label());
    if let Some(pending) = pending {
        let _ = pending.answer.send(primary);
    }
    if let Err(e) = window.close() {
        log::error!("could not close {}: {e}", window.label());
    }
}

/// Hides the traffic lights: the title bar is transparent and the board has none.
#[cfg(all(target_os = "macos", not(test)))]
mod macos {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use tauri::{Runtime, WebviewWindow};

    pub fn hide_buttons<R: Runtime>(window: &WebviewWindow<R>) {
        let ptr = match window.ns_window() {
            Ok(ptr) => ptr,
            Err(e) => return log::error!("no NSWindow for {}: {e}", window.label()),
        };
        // SAFETY: Tauri returns the live NSWindow of this window; `open` runs on the main thread.
        let ns_window = unsafe { &*ptr.cast::<NSWindow>() };
        for button in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(button) = ns_window.standardWindowButton(button) {
                button.setHidden(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::window::tests::{self as overlay, Handle, wait_until};
    use tauri::async_runtime::{JoinHandle, block_on, spawn};
    use tauri::test::{mock_builder, mock_context, noop_assets};

    fn app() -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .manage(crate::overlay::Overlay::default())
            .manage(Notices::default())
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    fn update() -> Notice {
        Notice::UpdateReady {
            version: "0.3.0".into(),
        }
    }

    /// Asks in the background and waits for the window.
    fn asking(app: &Handle, notice: Notice) -> JoinHandle<Option<bool>> {
        let label = notice.label();
        let handle = app.clone();
        let task = spawn(async move { ask(&handle, notice).await });
        // The mock runtime keeps closed windows: wait for the question too.
        wait_until("the notice window", || {
            app.get_webview_window(label).is_some()
                && lock(&app.state::<Notices>().open).contains_key(label)
        });
        task
    }

    fn window(app: &Handle, label: &str) -> WebviewWindow<tauri::test::MockRuntime> {
        app.get_webview_window(label).unwrap()
    }

    fn hidden(app: &Handle) -> Vec<String> {
        lock(&app.state::<Notices>().hidden).clone()
    }

    #[test]
    fn the_page_gets_its_notice_as_camel_case_json() {
        let shortcut = Notice::ShortcutChanged {
            reason: Reason::SnippingTool,
            accelerator: "Control+Shift+PrintScreen".into(),
        };
        assert_eq!(
            serde_json::to_value(&shortcut).unwrap(),
            serde_json::json!({
                "kind": "shortcutChanged",
                "reason": "snippingTool",
                "accelerator": "Control+Shift+PrintScreen",
            })
        );
        assert_eq!(
            serde_json::to_value(update()).unwrap(),
            serde_json::json!({ "kind": "updateReady", "version": "0.3.0" })
        );
        assert_eq!(shortcut.size(), (440.0, 220.0));
        assert_eq!(update().size(), (440.0, 200.0));
    }

    #[test]
    fn the_answer_is_the_button_clicked() {
        let app = app();
        let h = app.handle();
        for primary in [true, false] {
            let task = asking(h, update());
            let w = window(h, "notice-update");
            assert_eq!(notice(w.clone()), Some(update()));
            notice_answer(w.clone(), primary);
            assert_eq!(block_on(task).unwrap(), Some(primary));
            assert_eq!(notice(w), None, "answered");
        }
    }

    #[test]
    fn a_window_closed_another_way_answers_nothing() {
        let app = app();
        let h = app.handle();
        let task = asking(h, update());
        forget(h, "notice-update");
        assert_eq!(block_on(task).unwrap(), None);
    }

    #[test]
    fn asking_again_replaces_the_open_notice() {
        let app = app();
        let h = app.handle();
        let first = asking(h, update());
        let again = asking(h, update());
        assert_eq!(block_on(first).unwrap(), None);
        assert_eq!(h.webview_windows().len(), 1, "same window");
        notice_answer(window(h, "notice-update"), true);
        assert_eq!(block_on(again).unwrap(), Some(true));
    }

    #[test]
    fn a_capture_hides_the_notice_until_it_closes() {
        let app = app();
        let h = app.handle();
        let task = asking(h, update());
        let session = overlay::open(h, &[1]);
        assert_eq!(hidden(h), ["notice-update"]);
        crate::overlay::window::close_session(h, session);
        assert!(hidden(h).is_empty(), "shown again");
        notice_answer(window(h, "notice-update"), false);
        assert_eq!(block_on(task).unwrap(), Some(false));
    }

    #[test]
    fn a_notice_asked_during_a_capture_waits_for_it() {
        let app = app();
        let h = app.handle();
        let session = overlay::open(h, &[1]);
        let task = asking(h, update());
        assert_eq!(hidden(h), ["notice-update"]);
        // Hiding twice keeps one entry.
        hide_for_capture(h);
        assert_eq!(hidden(h), ["notice-update"]);
        crate::overlay::window::close_session(h, session);
        assert!(hidden(h).is_empty());
        forget(h, "notice-update");
        assert_eq!(block_on(task).unwrap(), None);
    }

    #[test]
    fn showing_skips_windows_already_gone() {
        let app = app();
        let h = app.handle();
        lock(&h.state::<Notices>().hidden).push("notice-gone".into());
        show_after_capture(h);
        assert!(hidden(h).is_empty());
    }

    #[test]
    fn without_notices_a_capture_has_nothing_to_hide() {
        let app = overlay::app();
        hide_for_capture(app.handle());
        show_after_capture(app.handle());
    }
}
