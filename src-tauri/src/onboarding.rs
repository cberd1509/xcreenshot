//! The first-run window: the Screen Recording steps (macOS), the capture shortcut and a place
//! to try it. Shown once, recorded as `onboarded` in the prefs, and on macOS again whenever the
//! permission is missing (as before).
//!
//! Migration: prefs saved before onboarding was recorded have no `onboarded` (`None`). Such an
//! install counts as onboarded if it can already capture (macOS: the permission is on;
//! elsewhere always), so existing users don't see the window again. A fresh install starts at
//! `Some(false)`. Closing the window once capture works marks it done.

use crate::capture;
use crate::overlay::window::CAPTURE_CLOSED;
use crate::prefs::{self, Storage};
use crate::print_screen::{self, SnippingTool};
use crate::settings;
use crate::shortcut::{Registered, Registrar};
use crate::windows::show_window;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Listener, Manager, Runtime, WebviewUrl, WebviewWindow, WindowEvent};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub const WINDOW: &str = "onboarding";
/// `prefs::modify` saves nothing when the change fails: used when there is nothing to change.
const UNCHANGED: &str = "nothing to change";

/// Whether to show the window at startup and the `onboarded` value to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    pub show: bool,
    pub onboarded: bool,
}

/// `granted`: whether capture works (always outside macOS).
pub fn decide(stored: Option<bool>, granted: bool) -> Start {
    // From before onboarding: done if it can already capture.
    let onboarded = stored.unwrap_or(granted);
    Start {
        show: !granted || !onboarded,
        onboarded,
    }
}

/// Saves the resolved `onboarded` if it changed and returns whether to show the window.
fn start(storage: &impl Storage, granted: bool) -> bool {
    let mut show = true;
    // `modify` logs a failed save; the window still shows if it should.
    let _ = prefs::modify(storage, |p| {
        let decided = decide(p.onboarded, granted);
        show = decided.show;
        if p.onboarded == Some(decided.onboarded) {
            return Err(UNCHANGED);
        }
        p.onboarded = Some(decided.onboarded);
        Ok(())
    });
    show
}

/// The window closed: onboarding is done if capture works by now.
fn finish(storage: &impl Storage, granted: bool) {
    if !granted {
        return;
    }
    // `modify` logs a failed save.
    let _ = prefs::modify(storage, |p| {
        if p.onboarded == Some(true) {
            return Err(UNCHANGED);
        }
        p.onboarded = Some(true);
        Ok(())
    });
}

/// Startup, after the settings are applied: shows the window if needed. Returns whether it
/// opened, so the separate "Shortcut changed" notice can be skipped (this window covers it).
pub fn on_startup<R: Runtime>(app: &AppHandle<R>) -> bool {
    let granted = capture::has_permission();
    // No store: nothing can be recorded, so show it.
    let show_it = prefs::with_store(app, |store| Ok(start(store, granted))).unwrap_or(true);
    show_it && show(app)
}

/// Whether to show the "Shortcut changed" notice at startup. The window explains a PrintScreen
/// that Snipping Tool has, so it replaces the notice then; another app holding it isn't
/// explained there.
pub fn needs_notice(reason: print_screen::Reason, onboarding_shown: bool) -> bool {
    !onboarding_shown || reason != print_screen::Reason::SnippingTool
}

/// Whether the capture shortcut works now: the permission (macOS) and a registered shortcut.
fn can_capture<R: Runtime>(app: &AppHandle<R>) -> bool {
    capture::has_permission()
        && app
            .try_state::<Registered>()
            .is_some_and(|r| r.get().registered)
}

/// The window was closed (Done, Later or the close button).
fn closed<R: Runtime>(app: &AppHandle<R>) {
    let works = can_capture(app);
    if let Err(e) = prefs::with_store(app, |store| {
        finish(store, works);
        Ok(())
    }) {
        log::error!("could not record onboarding: {e}");
    }
}

/// The window's events: closing it records onboarding.
fn on_event<R: Runtime>(app: &AppHandle<R>, event: &WindowEvent) {
    if matches!(event, WindowEvent::Destroyed) {
        closed(app);
    }
}

/// A capture ended while the window was open, kept so the page's "Try it" box can't miss it
/// while it loads.
#[derive(Default)]
pub struct Tried(AtomicBool);

/// Starts following a new window: its close, and captures that end while it is open.
fn watch<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    if app.manage(Tried::default()) {
        let handle = app.clone();
        app.listen_any(CAPTURE_CLOSED, move |_| {
            if handle.get_webview_window(WINDOW).is_some() {
                handle.state::<Tried>().0.store(true, Ordering::Relaxed);
            }
        });
    } else {
        app.state::<Tried>().0.store(false, Ordering::Relaxed);
    }
    let handle = app.clone();
    window.on_window_event(move |event| on_event(&handle, event));
}

/// Whether a capture ended since the window opened, for a page that just loaded.
#[tauri::command]
pub fn onboarding_tried<R: Runtime>(app: AppHandle<R>) -> bool {
    app.try_state::<Tried>()
        .is_some_and(|t| t.0.load(Ordering::Relaxed))
}

/// Opens (or focuses) the window. Returns whether it is open.
pub fn show<R: Runtime>(app: &AppHandle<R>) -> bool {
    let new = app.get_webview_window(WINDOW).is_none();
    let url = WebviewUrl::App("onboarding.html".into());
    let shown = show_window(app, WINDOW, url, |b| {
        b.title("xcreenshot")
            .inner_size(560.0, 700.0)
            .resizable(false)
            .center()
    });
    // As soon as it exists, even if showing it failed afterwards.
    if new && let Some(window) = app.get_webview_window(WINDOW) {
        watch(app, &window);
    }
    shown
        .inspect_err(|e| log::error!("could not open the onboarding window: {e}"))
        .is_ok()
}

/// Whether Snipping Tool keeps the shortcut on the fallback (Windows): the window then shows
/// the steps. With `claim` (the window showed them), switches to PrintScreen once Snipping
/// Tool lets go, if the fallback is still the saved shortcut; without it, a fallback picked on
/// purpose or kept off PrintScreen by another app stays.
fn claim_print_screen<R: Runtime>(
    app: &AppHandle<R>,
    shortcuts: &impl Registrar,
    snipping: SnippingTool,
    claim: bool,
) -> bool {
    let current = app.state::<Registered>().get().accelerator;
    let Some(read) = snipping.filter(|_| current == print_screen::FALLBACK) else {
        return false;
    };
    if read() {
        return true;
    }
    if !claim {
        return false;
    }
    // Same path as the recorder: checked, saved and sent to every page as `prefs:changed`.
    let (from, to) = (print_screen::FALLBACK, "PrintScreen");
    if let Err(e) = settings::replace_shortcut(app, shortcuts, snipping, from, to) {
        log::info!("could not switch to PrintScreen: {e}");
    }
    false
}

/// Whether Snipping Tool keeps xcreenshot off PrintScreen; with `claim`, switches to it once
/// it doesn't (see [`claim_print_screen`]).
#[tauri::command]
pub fn onboarding_print_screen<R: Runtime>(app: AppHandle<R>, claim: bool) -> bool {
    claim_print_screen(
        &app,
        app.global_shortcut(),
        print_screen::SNIPPING_TOOL,
        claim,
    )
}

/// Settings › Accessibility › Keyboard, where Snipping Tool's PrintScreen setting is.
#[cfg(target_os = "windows")]
const KEYBOARD_SETTINGS: &str = "ms-settings:easeofaccess-keyboard";

/// Opens Windows' keyboard settings. The URI is fixed, so the page can't ask for any other.
#[cfg(target_os = "windows")]
#[tauri::command]
pub fn open_keyboard_settings() -> Result<(), String> {
    open::that_detached(KEYBOARD_SETTINGS).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::tests::FakeStorage;
    use crate::print_screen::tests::{OFF, ON};
    use crate::shortcut::{self, tests::FakeRegistrar};
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    #[test]
    fn decides_who_sees_the_window() {
        let shown = |onboarded| Start {
            show: true,
            onboarded,
        };
        let hidden = Start {
            show: false,
            onboarded: true,
        };
        // Fresh install: shown, with or without the permission.
        assert_eq!(decide(Some(false), false), shown(false));
        assert_eq!(decide(Some(false), true), shown(false));
        // Done: only shown again if the permission went away (macOS).
        assert_eq!(decide(Some(true), true), hidden);
        assert_eq!(decide(Some(true), false), shown(true));
        // From before onboarding: done if it can already capture.
        assert_eq!(decide(None, true), hidden);
        assert_eq!(decide(None, false), shown(false));
    }

    fn storage(onboarded: Option<bool>) -> FakeStorage {
        let storage = FakeStorage::default();
        storage.prefs.borrow_mut().onboarded = onboarded;
        storage
    }

    #[test]
    fn startup_saves_the_decision_only_when_it_changes() {
        let existing = storage(None);
        assert!(!start(&existing, true));
        assert_eq!(existing.prefs.borrow().onboarded, Some(true));
        assert_eq!(existing.saves.get(), 1);
        assert!(!start(&existing, true));
        assert_eq!(existing.saves.get(), 1, "nothing new to save");

        let fresh = storage(Some(false));
        assert!(start(&fresh, true));
        assert_eq!(fresh.saves.get(), 0);

        // Still shown if the decision can't be saved.
        let mut failing = FakeStorage::failing();
        failing.prefs.get_mut().onboarded = None;
        assert!(start(&failing, false));
        assert_eq!(failing.saves.get(), 1);
    }

    #[test]
    fn closing_finishes_onboarding_once_capture_works() {
        let waiting = storage(Some(false));
        finish(&waiting, false);
        assert_eq!(waiting.prefs.borrow().onboarded, Some(false), "Later");
        finish(&waiting, true);
        assert_eq!(waiting.prefs.borrow().onboarded, Some(true));
        finish(&waiting, true);
        assert_eq!(waiting.saves.get(), 1, "already done");
        // A failed save is logged; nothing else to do.
        finish(&FakeStorage::failing(), true);
    }

    /// An app with its own data folder and the store.
    fn app(name: &str) -> tauri::App<MockRuntime> {
        let mut context = mock_context(noop_assets());
        context.config_mut().identifier =
            format!("com.bervex.xcreenshot.test.{name}.{}", std::process::id());
        let app = mock_builder()
            .plugin(tauri_plugin_store::Builder::new().build())
            .manage(Registered::default())
            .build(context)
            .unwrap();
        let _ = std::fs::remove_dir_all(app.path().app_data_dir().unwrap());
        app
    }

    #[test]
    fn switches_to_print_screen_once_snipping_tool_lets_go() {
        let app = app("onboarding-prtsc");
        let h = app.handle();
        let fake = FakeRegistrar::holding(print_screen::FALLBACK);
        let registered = h.state::<Registered>();
        registered.set(print_screen::FALLBACK, true);
        assert!(claim_print_screen(h, &fake, ON, true), "still taken");
        assert!(
            !claim_print_screen(h, &fake, None, true),
            "no Snipping Tool"
        );
        assert!(!claim_print_screen(h, &fake, OFF, false), "fallback kept");
        assert_eq!(fake.held(), [print_screen::FALLBACK]);
        assert!(!claim_print_screen(h, &fake, OFF, true));
        // Bare PrintScreen is only a valid shortcut where it is the default (Windows).
        let windows = shortcut::DEFAULT == "PrintScreen";
        let now = if windows {
            "PrintScreen"
        } else {
            print_screen::FALLBACK
        };
        assert_eq!(fake.held(), [now]);
        assert_eq!(registered.get().accelerator, now);
        if windows {
            assert_eq!(prefs::load(h).shortcut, "PrintScreen", "saved");
            assert!(!claim_print_screen(h, &fake, ON, true), "not the fallback");
        }

        // Another app took PrintScreen meanwhile: the fallback stays.
        let mut taken = FakeRegistrar::holding(print_screen::FALLBACK);
        taken.taken.insert("PrintScreen".into());
        registered.set(print_screen::FALLBACK, true);
        assert!(!claim_print_screen(h, &taken, OFF, true));
        assert_eq!(registered.get().accelerator, print_screen::FALLBACK);
        let _ = std::fs::remove_dir_all(h.path().app_data_dir().unwrap());
    }

    #[test]
    fn startup_and_close_record_onboarding_and_open_the_window() {
        let app = app("onboarding-startup");
        let h = app.handle();
        let granted = capture::has_permission();
        // A fresh install: shown, once.
        assert!(on_startup(h));
        assert!(show(h));
        assert_eq!(h.webview_windows().len(), 1);
        assert_eq!(prefs::load(h).onboarded, Some(false));
        // The window's own event handler (the mock runtime sends none) records the close, and
        // only a close.
        on_event(h, &WindowEvent::Focused(false));
        assert_eq!(prefs::load(h).onboarded, Some(false));
        on_event(h, &WindowEvent::Destroyed);
        assert_eq!(
            prefs::load(h).onboarded,
            Some(false),
            "the shortcut isn't registered"
        );
        h.state::<Registered>().set("Alt+S", true);
        on_event(h, &WindowEvent::Destroyed);
        assert_eq!(prefs::load(h).onboarded, Some(granted));
        // Done: shown again only without the permission.
        assert_eq!(on_startup(h), !granted);
        let _ = std::fs::remove_dir_all(h.path().app_data_dir().unwrap());
    }

    #[test]
    fn remembers_a_capture_that_ended_while_the_window_was_open() {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        let h = app.handle();
        assert!(!onboarding_tried(h.clone()), "never opened");
        assert!(show(h));
        assert!(!onboarding_tried(h.clone()));
        tauri::Emitter::emit(h, CAPTURE_CLOSED, ()).unwrap();
        assert!(onboarding_tried(h.clone()));
        // A new window starts over (the mock runtime can't destroy one, so watch it again).
        watch(h, &h.get_webview_window(WINDOW).unwrap());
        assert!(!onboarding_tried(h.clone()));
    }

    #[test]
    fn onboarding_replaces_only_the_snipping_tool_notice() {
        use print_screen::Reason::{InUse, SnippingTool};
        assert!(!needs_notice(SnippingTool, true));
        assert!(needs_notice(InUse, true), "not explained in the window");
        assert!(needs_notice(SnippingTool, false));
        assert!(needs_notice(InUse, false));
    }
}
