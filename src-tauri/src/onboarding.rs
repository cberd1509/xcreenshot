//! The first-run window: the Screen Recording steps (macOS), the capture shortcut and a place
//! to try it. Shown once, recorded as `onboarded` in the prefs, and on macOS again whenever the
//! permission is missing (as before).
//!
//! Migration: prefs saved before onboarding was recorded have no `onboarded` (`None`). Such an
//! install counts as onboarded if it can already capture (macOS: the permission is on;
//! elsewhere always), so existing users don't see the window again. A fresh install starts at
//! `Some(false)`. Closing the window once capture works marks it done.

use crate::capture;
use crate::prefs::{self, Prefs, Storage};
use crate::print_screen::{self, SnippingTool};
use crate::settings;
use crate::shortcut::{Registered, Registrar};
use crate::windows::show_window;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub const WINDOW: &str = "onboarding";
/// Sent to the window when a capture closes (copy, save, share or Esc), for its "Try it" box.
pub const CAPTURE_CLOSED: &str = "capture:closed";

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

/// Loads the prefs, saves the resolved `onboarded` if it changed, and returns whether to show
/// the window.
fn start(storage: &impl Storage, granted: bool) -> bool {
    let mut prefs = storage.load();
    let decided = decide(prefs.onboarded, granted);
    if prefs.onboarded != Some(decided.onboarded) {
        prefs.onboarded = Some(decided.onboarded);
        if let Err(e) = storage.save(&prefs) {
            log::error!("could not save prefs: {e}");
        }
    }
    decided.show
}

/// The window closed: onboarding is done if capture works by now.
fn finish(storage: &impl Storage, granted: bool) {
    if !granted {
        return;
    }
    let done = |p: &mut Prefs| {
        p.onboarded = Some(true);
        Ok(())
    };
    if storage.load().onboarded != Some(true) {
        // `modify` logs a failure.
        let _ = prefs::modify(storage, done);
    }
}

/// Startup, after the settings are applied: shows the window if needed. Returns whether it
/// did, so the separate "Shortcut changed" notice can be skipped (this window covers it).
pub fn on_startup<R: Runtime>(app: &AppHandle<R>) -> bool {
    let granted = capture::has_permission();
    let show_it = prefs::with_store(app, |store| Ok(start(store, granted)))
        .unwrap_or_else(|_| start(&prefs::Unsaved, granted));
    if show_it {
        show(app);
    }
    show_it
}

/// The window was closed (Done, Later or the close button).
pub fn closed<R: Runtime>(app: &AppHandle<R>) {
    let granted = capture::has_permission();
    if let Err(e) = prefs::with_store(app, |store| {
        finish(store, granted);
        Ok(())
    }) {
        log::error!("could not record onboarding: {e}");
    }
}

/// Opens (or focuses) the window.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    let url = WebviewUrl::App("onboarding.html".into());
    let shown = show_window(app, WINDOW, url, |b| {
        b.title("xcreenshot")
            .inner_size(560.0, 700.0)
            .resizable(false)
            .center()
    });
    if let Err(e) = shown {
        log::error!("could not open the onboarding window: {e}");
    }
}

/// Tells the window a capture closed. Costs nothing while it isn't open.
pub fn capture_closed<R: Runtime>(app: &AppHandle<R>) {
    if app.get_webview_window(WINDOW).is_some()
        && let Err(e) = app.emit_to(WINDOW, CAPTURE_CLOSED, ())
    {
        log::warn!("could not tell onboarding the capture closed: {e}");
    }
}

/// What the window does about PrintScreen (Windows) when it opens or gets focus back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrintScreen {
    /// The shortcut isn't the fallback: nothing to say.
    Fine,
    /// Snipping Tool still has it: show how to give it to xcreenshot.
    Taken,
    /// Snipping Tool let go of it: try switching to it.
    Free,
}

fn print_screen_state(current: &str, snipping: SnippingTool) -> PrintScreen {
    match snipping {
        Some(read) if current == print_screen::FALLBACK => {
            if read() {
                PrintScreen::Taken
            } else {
                PrintScreen::Free
            }
        }
        _ => PrintScreen::Fine,
    }
}

/// Switches to PrintScreen if Snipping Tool let go of it. Returns whether Snipping Tool still
/// has it (the window then shows the steps).
fn claim_print_screen<R: Runtime>(
    app: &AppHandle<R>,
    shortcuts: &impl Registrar,
    snipping: SnippingTool,
) -> bool {
    let current = app.state::<Registered>().get().accelerator;
    match print_screen_state(&current, snipping) {
        PrintScreen::Fine => false,
        PrintScreen::Taken => true,
        PrintScreen::Free => {
            // Same path as the recorder: saved and sent to every page as `prefs:changed`.
            let key_name = serde_json::Value::from("PrintScreen");
            if let Err(e) = settings::set_with(app, shortcuts, "shortcut", key_name) {
                log::info!("could not switch to PrintScreen: {e}");
            }
            false
        }
    }
}

/// Whether Snipping Tool keeps xcreenshot off PrintScreen; switches to it once it doesn't.
#[tauri::command]
pub fn onboarding_print_screen<R: Runtime>(app: AppHandle<R>) -> bool {
    claim_print_screen(&app, app.global_shortcut(), print_screen::SNIPPING_TOOL)
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
    use std::sync::{Arc, Mutex};
    use tauri::Listener;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    #[test]
    fn decides_who_sees_the_window() {
        let shown = |onboarded| Start {
            show: true,
            onboarded,
        };
        // Fresh install: shown, with or without the permission.
        assert_eq!(decide(Some(false), false), shown(false));
        assert_eq!(decide(Some(false), true), shown(false));
        // Done: only shown again if the permission went away (macOS).
        assert_eq!(
            decide(Some(true), true),
            Start {
                show: false,
                onboarded: true
            }
        );
        assert_eq!(decide(Some(true), false), shown(true));
        // From before onboarding: done if it can already capture.
        assert_eq!(
            decide(None, true),
            Start {
                show: false,
                onboarded: true
            }
        );
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

    #[test]
    fn print_screen_steps_only_for_the_fallback_on_windows() {
        let fallback = print_screen::FALLBACK;
        assert_eq!(print_screen_state(fallback, ON), PrintScreen::Taken);
        assert_eq!(print_screen_state(fallback, OFF), PrintScreen::Free);
        assert_eq!(print_screen_state("PrintScreen", ON), PrintScreen::Fine);
        assert_eq!(print_screen_state("Alt+S", OFF), PrintScreen::Fine);
        assert_eq!(print_screen_state(fallback, None), PrintScreen::Fine);
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
        assert!(claim_print_screen(h, &fake, ON), "still taken");
        assert_eq!(fake.held(), [print_screen::FALLBACK]);
        assert!(!claim_print_screen(h, &fake, OFF));
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
            assert!(!claim_print_screen(h, &fake, OFF), "nothing more to do");
        }

        // Another app took PrintScreen meanwhile: the fallback stays.
        let mut taken = FakeRegistrar::holding(print_screen::FALLBACK);
        taken.taken.insert("PrintScreen".into());
        registered.set(print_screen::FALLBACK, true);
        assert!(!claim_print_screen(h, &taken, OFF));
        assert_eq!(registered.get().accelerator, print_screen::FALLBACK);
        let _ = std::fs::remove_dir_all(h.path().app_data_dir().unwrap());
    }

    #[test]
    fn startup_and_close_record_onboarding_and_open_the_window() {
        let app = app("onboarding-startup");
        let h = app.handle();
        let granted = capture::has_permission();
        // A fresh install: shown.
        assert!(on_startup(h));
        assert!(h.get_webview_window(WINDOW).is_some());
        assert_eq!(prefs::load(h).onboarded, Some(false));
        closed(h);
        assert_eq!(prefs::load(h).onboarded, Some(granted));
        // Done: shown again only without the permission.
        assert_eq!(on_startup(h), !granted);
        let _ = std::fs::remove_dir_all(h.path().app_data_dir().unwrap());
    }

    #[test]
    fn the_window_hears_when_a_capture_closes() {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        let h = app.handle();
        let heard = Arc::new(Mutex::new(0));
        let sink = heard.clone();
        h.listen_any(CAPTURE_CLOSED, move |_| *sink.lock().unwrap() += 1);
        capture_closed(h);
        assert_eq!(*heard.lock().unwrap(), 0, "no window, no event");
        show(h);
        show(h);
        assert_eq!(h.webview_windows().len(), 1);
        capture_closed(h);
        assert_eq!(*heard.lock().unwrap(), 1);
    }
}
