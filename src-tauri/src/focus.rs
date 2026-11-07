//! Where focus goes when the overlay or a notice closes. Each records the app that was in front
//! ([`frontmost`]); closing hands focus back to it ([`hand_back`]). The decisions are
//! [`handback`] and [`Focus::hand_back`]; the AppKit calls live in `macos.rs`, behind [`Apps`]
//! so tests can see which ran. Only macOS manages a [`Focus`]; elsewhere the system moves focus
//! itself when a window hides.
#![cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]

use crate::windows;
use tauri::{AppHandle, Manager, Runtime};

/// One launch of an app: a pid can be reused by a later app, a launch date can't. Only macOS
/// reads one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppId {
    pub pid: i32,
    /// Seconds since 1970; `None` for apps not launched through LaunchServices.
    pub launched: Option<f64>,
}

/// The app that was in front when the capture started or the notice opened.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Front {
    /// xcreenshot itself (the capture started from Settings, say).
    Us,
    Other(AppId),
    /// AppKit could not say, or nothing was recorded.
    #[default]
    Unknown,
}

impl Front {
    /// Classifies the app in front against our own pid.
    pub fn of(front: Option<AppId>, ours: u32) -> Self {
        match front {
            Some(app) if u32::try_from(app.pid).is_ok_and(|p| p == ours) => Self::Us,
            Some(app) => Self::Other(app),
            None => Self::Unknown,
        }
    }
}

/// What to do with focus once the overlay or a notice is hidden.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Handback {
    /// Leave it with our open window (Settings): the user started there.
    Keep,
    /// Hide xcreenshot, so macOS gives focus to the app behind it.
    Hide,
    /// Activate the previous app: one of our windows is open, so staying active would bring it
    /// to the front.
    Activate(AppId),
}

/// `window_open` says whether one of our other windows (Settings, a notice) is on screen.
pub fn handback(before: Front, window_open: bool) -> Handback {
    match (before, window_open) {
        (Front::Us, true) => Handback::Keep,
        (Front::Other(app), true) => Handback::Activate(app),
        // Hiding beats leaving Settings in front of an app we can't name.
        (Front::Unknown, true) | (_, false) => Handback::Hide,
    }
}

/// The platform calls [`Focus`] makes.
pub trait Apps: Send + Sync {
    fn frontmost(&self) -> Option<AppId>;
    /// The app running as `pid` now, if any.
    fn running(&self, pid: i32) -> Option<AppId>;
    /// Asks `app` to come to the front; `false` if that launch is gone or macOS refused.
    fn activate(&self, app: &AppId) -> bool;
    /// Hides xcreenshot.
    fn hide(&self);
}

/// Hands focus back after a capture or a notice; managed by the app on macOS.
pub struct Focus(Box<dyn Apps>);

impl Focus {
    pub fn new(apps: impl Apps + 'static) -> Self {
        Self(Box::new(apps))
    }

    /// AppKit, on macOS.
    #[cfg(all(target_os = "macos", not(test)))]
    pub fn os() -> Self {
        Self::new(Os)
    }

    fn hand_back(&self, before: Front, window_open: bool) {
        let app = match handback(before, window_open) {
            Handback::Keep => return,
            Handback::Hide => return self.0.hide(),
            Handback::Activate(app) => app,
        };
        // Only the same launch: its pid may now belong to another app.
        let same = app.launched.is_some() && self.0.running(app.pid) == Some(app);
        if !same {
            log::info!("the app in front before the capture has quit; hiding xcreenshot");
            self.0.hide();
        } else if !self.0.activate(&app) {
            log::info!("macOS did not activate the previous app; hiding xcreenshot");
            self.0.hide();
        }
    }
}

/// The app in front right now. Read on the hotkey or as a notice opens, before its window takes
/// focus. Main thread only.
pub fn frontmost<R: Runtime>(app: &AppHandle<R>) -> Front {
    app.try_state::<Focus>().map_or(Front::Unknown, |focus| {
        Front::of(focus.0.frontmost(), std::process::id())
    })
}

/// Gives focus back after a capture or a notice, once its windows are hidden; `closing` is a
/// window still closing, which doesn't count as open. Main thread only.
pub fn hand_back<R: Runtime>(app: &AppHandle<R>, before: Front, closing: Option<&str>) {
    if let Some(focus) = app.try_state::<Focus>() {
        focus.hand_back(before, windows::window_open(app, closing));
    }
}

#[cfg(all(target_os = "macos", not(test)))]
struct Os;

#[cfg(all(target_os = "macos", not(test)))]
impl Apps for Os {
    fn frontmost(&self) -> Option<AppId> {
        crate::macos::frontmost()
    }
    fn running(&self, pid: i32) -> Option<AppId> {
        crate::macos::running(pid)
    }
    fn activate(&self, app: &AppId) -> bool {
        crate::macos::activate(app)
    }
    fn hide(&self) {
        crate::macos::hide()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    pub(crate) const SAFARI: AppId = AppId {
        pid: 7,
        launched: Some(1.0),
    };

    /// Fake apps: `front` is in front, `running` are running, and activation works if
    /// `activates`. Records each call it gets.
    #[derive(Clone)]
    pub(crate) struct Fake {
        pub front: Option<AppId>,
        pub running: Vec<AppId>,
        pub activates: bool,
        pub calls: Arc<Mutex<Vec<String>>>,
    }

    impl Fake {
        pub(crate) fn new(front: Option<AppId>) -> Self {
            Self {
                front,
                running: front.into_iter().collect(),
                activates: true,
                calls: Arc::default(),
            }
        }

        pub(crate) fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn call(&self, what: String) {
            self.calls.lock().unwrap().push(what);
        }
    }

    impl Apps for Fake {
        fn frontmost(&self) -> Option<AppId> {
            self.front
        }
        fn running(&self, pid: i32) -> Option<AppId> {
            self.running.iter().copied().find(|a| a.pid == pid)
        }
        fn activate(&self, app: &AppId) -> bool {
            self.call(format!("activate {}", app.pid));
            self.activates
        }
        fn hide(&self) {
            self.call("hide".into());
        }
    }

    fn hand_back_with(fake: &Fake, before: Front, window_open: bool) -> Vec<String> {
        Focus::new(fake.clone()).hand_back(before, window_open);
        fake.calls()
    }

    #[test]
    fn our_own_pid_is_us() {
        assert_eq!(Front::of(Some(SAFARI), 7), Front::Us);
        assert_eq!(Front::of(Some(SAFARI), 42), Front::Other(SAFARI));
        assert_eq!(Front::of(None, 42), Front::Unknown);
        // No pid (-1) is never us.
        let no_pid = AppId { pid: -1, ..SAFARI };
        assert_eq!(Front::of(Some(no_pid), u32::MAX), Front::Other(no_pid));
    }

    #[test]
    fn with_no_window_open_xcreenshot_hides() {
        for before in [Front::Unknown, Front::Us, Front::Other(SAFARI)] {
            assert_eq!(handback(before, false), Handback::Hide);
            assert_eq!(hand_back_with(&Fake::new(None), before, false), ["hide"]);
        }
    }

    #[test]
    fn with_settings_open_focus_returns_to_the_previous_app() {
        let fake = Fake::new(Some(SAFARI));
        assert_eq!(
            hand_back_with(&fake, Front::Other(SAFARI), true),
            ["activate 7"]
        );
    }

    #[test]
    fn a_capture_started_from_settings_keeps_settings_focused() {
        assert!(hand_back_with(&Fake::new(None), Front::Us, true).is_empty());
    }

    #[test]
    fn an_unknown_app_hides_instead_of_keeping_settings_in_front() {
        assert_eq!(
            hand_back_with(&Fake::new(None), Front::Unknown, true),
            ["hide"]
        );
    }

    #[test]
    fn a_refused_activation_hides() {
        let fake = Fake {
            activates: false,
            ..Fake::new(Some(SAFARI))
        };
        assert_eq!(
            hand_back_with(&fake, Front::Other(SAFARI), true),
            ["activate 7", "hide"]
        );
    }

    #[test]
    fn an_app_that_quit_or_lost_its_pid_is_not_activated() {
        let reused = AppId {
            launched: Some(2.0),
            ..SAFARI
        };
        let undated = AppId {
            launched: None,
            ..SAFARI
        };
        for (running, before) in [
            (vec![], SAFARI),
            (vec![reused], SAFARI),
            (vec![undated], undated),
        ] {
            let fake = Fake {
                running,
                ..Fake::new(None)
            };
            assert_eq!(hand_back_with(&fake, Front::Other(before), true), ["hide"]);
        }
    }

    #[test]
    fn without_focus_managed_nothing_is_read() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        assert_eq!(frontmost(app.handle()), Front::Unknown);
    }
}
