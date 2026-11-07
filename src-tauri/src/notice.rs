//! The app's small prompts: "Update ready" and, on Windows, "Shortcut changed". Each is an
//! xcreenshot window sized to its card (`notice.html`) instead of a system alert, so it never
//! covers the capture overlay: it is hidden while a capture is open and shown again after.
//!
//! Rust keeps what each window shows and the answer it waits for. The page asks for its
//! content ([`notice`]) and sends back the button clicked ([`notice_answer`]). Every question
//! gets a window of its own, so a closing window never takes a newer question with it.

use crate::focus::{self, Front};
use crate::print_screen::Reason;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::oneshot;

static NEXT: AtomicU64 = AtomicU64::new(1);

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
    /// Hidden while a capture is open.
    hidden: bool,
    /// The app in front as the notice opened, to give focus back to once it's answered.
    front: Front,
}

/// The open notices, by window label.
#[derive(Default)]
pub struct Notices(Mutex<HashMap<String, Pending>>);

impl Notices {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Pending>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Shows `notice` and resolves to the button clicked: `Some(true)` for the main (right) one,
/// `Some(false)` for the other, `None` if the window closed another way.
pub async fn ask<R: Runtime>(app: &AppHandle<R>, notice: Notice) -> Option<bool> {
    let (answer, rx) = oneshot::channel();
    let label = format!("notice-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let size = notice.size();
    let pending = Pending {
        notice,
        answer,
        hidden: false,
        front: Front::Unknown,
    };
    app.state::<Notices>().lock().insert(label.clone(), pending);
    let handle = app.clone();
    let opening = label.clone();
    let shown = app.run_on_main_thread(move || {
        if let Err(e) = open(&handle, &opening, size) {
            log::error!("could not open {opening}: {e}");
            forget(&handle, &opening);
        }
    });
    if let Err(e) = shown {
        log::error!("could not open {label}: {e}");
        forget(app, &label);
    }
    rx.await.ok()
}

/// Opens the window, hidden if a capture is open. Main thread, like the capture's start and
/// end, so a capture can't begin in between.
fn open<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    (width, height): (f64, f64),
) -> tauri::Result<()> {
    // During a capture the overlay is in front: the app before it is the one to go back to.
    let capture = crate::overlay::front(app);
    let busy = capture.is_some();
    let front = capture.unwrap_or_else(|| focus::frontmost(app));
    let url = WebviewUrl::App("notice.html".into());
    let builder = WebviewWindowBuilder::new(app, label, url)
        .title("xcreenshot")
        .inner_size(width, height)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        // Above other apps, like the system alert it replaces; still under the overlay, which
        // hides it anyway.
        .always_on_top(true)
        .center()
        .visible(!busy);
    // The card is the whole window: no title bar, the system's corners, border and shadow.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    let window = builder.build()?;
    #[cfg(all(target_os = "macos", not(test)))]
    hide_buttons(&window);
    let handle = app.clone();
    let closed = label.to_owned();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            forget(&handle, &closed);
        }
    });
    if let Some(pending) = app.state::<Notices>().lock().get_mut(label) {
        // Never un-hides: a capture that began meanwhile may have hidden it already.
        pending.hidden |= busy;
        pending.front = front;
    }
    if !busy && let Err(e) = window.set_focus() {
        // Still on screen and usable: a click focuses it.
        log::warn!("could not focus {label}: {e}");
    }
    Ok(())
}

/// The window closed: its question is dropped, so `ask` resolves to `None`.
fn forget<R: Runtime>(app: &AppHandle<R>, label: &str) {
    app.state::<Notices>().lock().remove(label);
}

/// Hides (`hide`) or shows again the notices not already that way. Main thread only.
fn set_hidden<R: Runtime>(app: &AppHandle<R>, hide: bool) {
    let Some(notices) = app.try_state::<Notices>() else {
        return;
    };
    // Collected first: the lock isn't held while windows hide or show.
    let labels: Vec<String> = notices
        .lock()
        .iter()
        .filter(|(_, p)| p.hidden != hide)
        .map(|(label, _)| label.clone())
        .collect();
    for label in labels {
        let Some(window) = app.get_webview_window(&label) else {
            continue;
        };
        // Marked only once done, so a failed one is tried again next time.
        match if hide { window.hide() } else { window.show() } {
            Ok(()) => {
                if let Some(pending) = notices.lock().get_mut(&label) {
                    pending.hidden = hide;
                }
            }
            Err(e) => log::warn!("could not hide or show {label}: {e}"),
        }
    }
}

/// A capture is starting: hides the open notices. Main thread only.
pub fn hide_for_capture<R: Runtime>(app: &AppHandle<R>) {
    set_hidden(app, true);
}

/// The capture closed: shows the notices it hid, without taking focus from the app the user
/// was in. Main thread only.
pub fn show_after_capture<R: Runtime>(app: &AppHandle<R>) {
    set_hidden(app, false);
}

/// What the calling notice window shows; `None` once it was answered.
#[tauri::command]
pub(crate) fn notice<R: Runtime>(window: WebviewWindow<R>) -> Option<Notice> {
    let notices = window.state::<Notices>();
    let open = notices.lock();
    open.get(window.label()).map(|p| p.notice.clone())
}

/// A button was clicked (`primary`: the main one): closes the window, answers, then gives focus
/// back to the app in front when it opened. An error means the window is still open and
/// unanswered; clicking again tries again.
#[tauri::command]
pub(crate) fn notice_answer<R: Runtime>(
    window: WebviewWindow<R>,
    primary: bool,
) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())?;
    let pending = window.state::<Notices>().lock().remove(window.label());
    // Already answered: the first click gave focus back.
    let Some(pending) = pending else {
        return Ok(());
    };
    let _ = pending.answer.send(primary);
    // A capture started meanwhile owns focus and hands it back when it closes.
    let app = window.app_handle();
    if !crate::overlay::is_busy(app) {
        focus::hand_back(app, pending.front, Some(window.label()));
    }
    Ok(())
}

/// Hides the traffic lights: the title bar is transparent and the board has none.
#[cfg(all(target_os = "macos", not(test)))]
fn hide_buttons<R: Runtime>(window: &WebviewWindow<R>) {
    use objc2_app_kit::NSWindowButton;
    let ns_window = match crate::macos::ns_window(window) {
        Ok(w) => w,
        Err(e) => return log::error!("no NSWindow for {}: {e}", window.label()),
    };
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::overlay::window::tests::{self as overlay, Handle, wait_until};
    use tauri::async_runtime::{JoinHandle, block_on, spawn};
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    /// An app with the state a notice needs.
    pub(crate) fn app() -> tauri::App<MockRuntime> {
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

    /// The label of the open notice showing `notice`, once its window is up.
    pub(crate) fn opened(app: &Handle, notice: &Notice) -> String {
        let find = || {
            let notices = app.state::<Notices>();
            let open = notices.lock();
            let (label, _) = open.iter().find(|(_, p)| &p.notice == notice)?;
            app.get_webview_window(label).map(|_| label.clone())
        };
        wait_until("the notice window", || find().is_some());
        find().unwrap()
    }

    /// Asks in the background; returns the answer's task and the window.
    fn asking(
        app: &Handle,
        notice: Notice,
    ) -> (JoinHandle<Option<bool>>, WebviewWindow<MockRuntime>) {
        let handle = app.clone();
        let asked = notice.clone();
        let task = spawn(async move { ask(&handle, asked).await });
        let label = opened(app, &notice);
        (task, app.get_webview_window(&label).unwrap())
    }

    fn hidden(app: &Handle) -> Vec<String> {
        let notices = app.state::<Notices>();
        let open = notices.lock();
        open.iter()
            .filter(|(_, p)| p.hidden)
            .map(|(l, _)| l.clone())
            .collect()
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
            let (task, window) = asking(h, update());
            assert_eq!(notice(window.clone()), Some(update()));
            assert_eq!(notice_answer(window.clone(), primary), Ok(()));
            assert_eq!(block_on(task).unwrap(), Some(primary));
            assert_eq!(notice(window.clone()), None, "answered");
            // A second click only closes the window again.
            assert_eq!(notice_answer(window, primary), Ok(()));
        }
    }

    #[test]
    fn each_question_gets_its_own_window() {
        let app = app();
        let h = app.handle();
        let (first, a) = asking(h, update());
        let newer = Notice::UpdateReady {
            version: "0.4.0".into(),
        };
        let (again, b) = asking(h, newer.clone());
        assert_ne!(a.label(), b.label());
        assert_eq!(notice(b.clone()), Some(newer), "each shows its own");
        // The first window closing doesn't drop the newer question.
        forget(h, a.label());
        assert_eq!(block_on(first).unwrap(), None);
        notice_answer(b, true).unwrap();
        assert_eq!(block_on(again).unwrap(), Some(true));
    }

    #[test]
    fn a_capture_hides_the_notice_until_it_closes() {
        let app = app();
        let h = app.handle();
        let (task, window) = asking(h, update());
        let session = overlay::open(h, &[1]);
        assert_eq!(hidden(h), [window.label()]);
        crate::overlay::window::close_session(h, session);
        assert!(hidden(h).is_empty(), "shown again");
        notice_answer(window, false).unwrap();
        assert_eq!(block_on(task).unwrap(), Some(false));
    }

    #[test]
    fn a_share_that_ends_the_capture_shows_the_notice_again() {
        use crate::overlay::window::{close_for_share, phase};
        let app = app();
        let h = app.handle();
        let (task, window) = asking(h, update());
        let session = overlay::shown(h, &[1]);
        assert!(phase(h).begin_share(session));
        assert_eq!(hidden(h), [window.label()]);
        assert!(close_for_share(h, session));
        assert!(hidden(h).is_empty(), "shown again");
        forget(h, window.label());
        assert_eq!(block_on(task).unwrap(), None);
    }

    #[test]
    fn a_notice_asked_during_a_capture_waits_for_it() {
        let app = app();
        let h = app.handle();
        let session = overlay::open(h, &[1]);
        let (task, window) = asking(h, update());
        assert_eq!(hidden(h), [window.label()]);
        crate::overlay::window::close_session(h, session);
        assert!(hidden(h).is_empty());
        forget(h, window.label());
        assert_eq!(block_on(task).unwrap(), None);
    }

    #[test]
    fn without_notices_a_capture_has_nothing_to_hide() {
        let app = overlay::app();
        hide_for_capture(app.handle());
        show_after_capture(app.handle());
    }

    mod focus {
        use super::*;
        use crate::focus::tests::{Fake, SAFARI};
        use crate::focus::{AppId, Focus};

        const MAIL: AppId = AppId {
            pid: 8,
            launched: Some(2.0),
        };

        /// [`app`] with `fake` for the platform's apps and Settings open.
        fn app_with(fake: &Fake) -> tauri::App<MockRuntime> {
            let app = app();
            app.manage(Focus::new(fake.clone()));
            WebviewWindowBuilder::new(app.handle(), "settings", WebviewUrl::default())
                .build()
                .unwrap();
            app
        }

        /// The focus calls made answering a notice asked while `front` was in front.
        fn answering_with(front: Option<AppId>) -> Vec<String> {
            let fake = Fake::new(front);
            let app = app_with(&fake);
            let (task, window) = asking(app.handle(), update());
            notice_answer(window.clone(), true).unwrap();
            assert_eq!(block_on(task).unwrap(), Some(true));
            let calls = fake.calls();
            // A second click has nothing to hand back.
            notice_answer(window, true).unwrap();
            assert_eq!(fake.calls(), calls);
            calls
        }

        #[test]
        fn with_settings_behind_another_app_focus_returns_to_that_app() {
            assert_eq!(answering_with(Some(SAFARI)), ["activate 7"]);
        }

        #[test]
        fn a_notice_that_came_up_over_xcreenshot_keeps_it_in_front() {
            let us = AppId {
                pid: std::process::id() as i32,
                ..SAFARI
            };
            assert!(answering_with(Some(us)).is_empty());
        }

        #[test]
        fn an_unknown_previous_app_hides_xcreenshot() {
            assert_eq!(answering_with(None), ["hide"]);
        }

        #[test]
        fn an_answer_during_a_capture_leaves_focus_to_the_capture() {
            let fake = Fake::new(Some(SAFARI));
            let app = app_with(&fake);
            let h = app.handle();
            let (task, window) = asking(h, update());
            let session = overlay::open(h, &[1]);
            notice_answer(window, true).unwrap();
            assert_eq!(block_on(task).unwrap(), Some(true));
            assert!(fake.calls().is_empty(), "the capture keeps focus");
            crate::overlay::window::close_session(h, session);
            assert_eq!(fake.calls(), ["activate 7"], "only the capture hands back");
        }

        #[test]
        fn a_notice_asked_during_a_capture_returns_to_the_app_before_it() {
            // AppKit says Safari now, but Mail was in front at the hotkey.
            let fake = Fake {
                running: vec![SAFARI, MAIL],
                ..Fake::new(Some(SAFARI))
            };
            let app = app_with(&fake);
            let h = app.handle();
            let (session, release) = overlay::press(h, &[1]);
            crate::overlay::window::phase(h).set_front(Front::Other(MAIL));
            let (task, window) = asking(h, update());
            release.send(true).unwrap();
            wait_until("the session to open", || overlay::is_open(h, session));
            crate::overlay::window::close_session(h, session);
            notice_answer(window, false).unwrap();
            assert_eq!(block_on(task).unwrap(), Some(false));
            assert_eq!(fake.calls(), ["activate 8", "activate 8"]);
        }
    }
}
