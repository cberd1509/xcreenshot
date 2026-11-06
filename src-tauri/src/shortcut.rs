//! The global capture shortcut: checking a new one and swapping it in while the app runs.
//!
//! Accelerators use the tauri-plugin-global-shortcut format, e.g. `Control+Shift+Super+S`.

use serde::Serialize;
use std::sync::{Mutex, MutexGuard, PoisonError};
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_global_shortcut::{GlobalShortcut, GlobalShortcutExt, Modifiers, Shortcut};

/// Default capture shortcut on this platform.
#[cfg(target_os = "macos")]
pub const DEFAULT: &str = "Control+Shift+Super+S";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT: &str = "PrintScreen";

pub const IN_USE: &str = "That shortcut is in use by another app.";
/// The shortcut couldn't be let go (to swap or pause it); nothing changed.
pub const NOT_CHANGED: &str = "Couldn't change the shortcut.";
const INVALID: &str = "That shortcut can't be used.";
#[cfg(target_os = "macos")]
const NO_MODIFIER: &str = "Add ⌘, ⌃ or ⌥.";
#[cfg(not(target_os = "macos"))]
const NO_MODIFIER: &str = "Add Ctrl, Alt or Win.";

/// Checks that `accelerator` parses and has ⌘/Win, Control or Alt (Option): Shift alone would
/// take over typing. The default is always fine (bare PrintScreen on Windows).
pub fn validate(accelerator: &str) -> Result<(), &'static str> {
    let shortcut: Shortcut = accelerator.parse().map_err(|_| INVALID)?;
    let held = Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER;
    if !shortcut.mods.intersects(held) && accelerator != DEFAULT {
        return Err(NO_MODIFIER);
    }
    Ok(())
}

/// Registers and unregisters global shortcuts (the plugin, or a fake in tests).
pub trait Registrar {
    fn register(&self, accelerator: &str) -> Result<(), String>;
    fn unregister(&self, accelerator: &str) -> Result<(), String>;
}

impl<R: Runtime> Registrar for GlobalShortcut<R> {
    fn register(&self, accelerator: &str) -> Result<(), String> {
        GlobalShortcut::register(self, accelerator).map_err(|e| e.to_string())
    }

    fn unregister(&self, accelerator: &str) -> Result<(), String> {
        GlobalShortcut::unregister(self, accelerator).map_err(|e| e.to_string())
    }
}

/// [`swap`], [`pause`] and [`resume`] run one at a time, so a resume can't slip in before a
/// pause that is still letting go and leave the shortcut paused.
fn transition() -> MutexGuard<'static, ()> {
    static TRANSITION: Mutex<()> = Mutex::new(());
    TRANSITION.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Replaces the registered capture shortcut (if any) with `new` and records it in `registered`.
/// If `new` can't be registered, the old one stays and the error says the shortcut is taken.
/// If the old one can't be let go, nothing changes.
pub fn swap(
    registrar: &impl Registrar,
    registered: &Registered,
    new: &str,
) -> Result<(), &'static str> {
    let _one_at_a_time = transition();
    let old = registered.active();
    if old.as_deref() == Some(new) {
        return Ok(());
    }
    if let Some(old) = &old
        && let Err(e) = registrar.unregister(old)
    {
        log::warn!("could not unregister the capture shortcut {old}: {e}");
        return Err(NOT_CHANGED);
    }
    let Err(e) = registrar.register(new) else {
        registered.set(new, true);
        return Ok(());
    };
    log::warn!("could not register the capture shortcut {new}: {e}");
    if let Some(old) = &old
        && let Err(e) = registrar.register(old)
    {
        log::error!("could not restore the capture shortcut {old}: {e}");
        registered.set(old, false);
    }
    Err(IN_USE)
}

/// The capture shortcut from the settings and whether it is registered (another app may own it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureShortcut {
    pub accelerator: String,
    pub registered: bool,
}

/// The current [`CaptureShortcut`], shared by the shortcut handler and the pages.
pub struct Registered(Mutex<Current>);

struct Current {
    shortcut: CaptureShortcut,
    /// The registered shortcut's id, so the handler compares without parsing.
    id: Option<u32>,
    /// Let go while the Settings window records a new one (see [`pause`]).
    paused: bool,
}

impl Default for Registered {
    fn default() -> Self {
        Self(Mutex::new(Current {
            shortcut: CaptureShortcut {
                accelerator: DEFAULT.to_owned(),
                registered: false,
            },
            id: None,
            paused: false,
        }))
    }
}

/// Lets go of the capture shortcut while a new one is recorded, so pressing it reaches the
/// recorder instead of starting a capture. [`resume`] takes it back. If it can't be let go,
/// it stays active and the error is returned.
pub fn pause(registrar: &impl Registrar, registered: &Registered) -> Result<(), &'static str> {
    let _one_at_a_time = transition();
    let Some(accelerator) = registered.active() else {
        return Ok(());
    };
    registrar.unregister(&accelerator).map_err(|e| {
        log::warn!("could not pause the capture shortcut {accelerator}: {e}");
        NOT_CHANGED
    })?;
    let mut current = registered.lock();
    current.paused = true;
    current.id = None;
    Ok(())
}

/// Registers the paused capture shortcut again. Does nothing if it isn't paused (a new
/// shortcut was saved meanwhile, or it was never paused).
pub fn resume(registrar: &impl Registrar, registered: &Registered) {
    let _one_at_a_time = transition();
    let accelerator = {
        let current = registered.lock();
        if !current.paused {
            return;
        }
        current.shortcut.accelerator.clone()
    };
    let ok = registrar
        .register(&accelerator)
        .inspect_err(|e| log::warn!("could not resume the capture shortcut {accelerator}: {e}"))
        .is_ok();
    registered.set(&accelerator, ok);
}

impl Registered {
    fn lock(&self) -> MutexGuard<'_, Current> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set(&self, accelerator: &str, registered: bool) {
        let id = registered
            .then(|| accelerator.parse::<Shortcut>().ok().map(|s| s.id()))
            .flatten();
        *self.lock() = Current {
            shortcut: CaptureShortcut {
                accelerator: accelerator.to_owned(),
                registered,
            },
            id,
            paused: false,
        };
    }

    pub fn get(&self) -> CaptureShortcut {
        self.lock().shortcut.clone()
    }

    /// The accelerator if the app holds it right now (registered and not paused).
    pub fn active(&self) -> Option<String> {
        let current = self.lock();
        (current.shortcut.registered && !current.paused)
            .then(|| current.shortcut.accelerator.clone())
    }

    /// Whether `pressed` is the registered capture shortcut.
    pub fn is_capture(&self, pressed: &Shortcut) -> bool {
        self.lock().id == Some(pressed.id())
    }
}

/// The capture shortcut for the permission page.
#[tauri::command]
pub fn capture_shortcut(registered: State<'_, Registered>) -> CaptureShortcut {
    registered.get()
}

/// The Settings window started recording a shortcut.
#[tauri::command]
pub fn pause_capture_shortcut<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    Ok(pause(app.global_shortcut(), &app.state::<Registered>())?)
}

/// The Settings window stopped recording.
#[tauri::command]
pub fn resume_capture_shortcut<R: Runtime>(app: AppHandle<R>) {
    resume_capture(&app);
}

/// Takes the capture shortcut back if it is paused. Also called when the Settings page
/// reloads or its window closes, so the shortcut always comes back.
pub fn resume_capture<R: Runtime>(app: &AppHandle<R>) {
    if let Some(shortcuts) = app.try_state::<GlobalShortcut<R>>() {
        resume(&*shortcuts, &app.state::<Registered>());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashSet;
    use tauri::Manager;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri_plugin_global_shortcut::Code;

    /// Registers anything except the `taken` accelerators; fails to unregister what it doesn't hold.
    #[derive(Default)]
    pub struct FakeRegistrar {
        pub taken: HashSet<String>,
        pub held: RefCell<HashSet<String>>,
    }

    impl FakeRegistrar {
        pub fn holding(held: &str) -> Self {
            let fake = Self::default();
            fake.held.borrow_mut().insert(held.to_owned());
            fake
        }

        pub fn held(&self) -> Vec<String> {
            let mut held: Vec<_> = self.held.borrow().iter().cloned().collect();
            held.sort();
            held
        }
    }

    impl Registrar for FakeRegistrar {
        fn register(&self, accelerator: &str) -> Result<(), String> {
            if self.taken.contains(accelerator)
                || !self.held.borrow_mut().insert(accelerator.into())
            {
                return Err("taken".into());
            }
            Ok(())
        }

        fn unregister(&self, accelerator: &str) -> Result<(), String> {
            if self.held.borrow_mut().remove(accelerator) {
                Ok(())
            } else {
                Err("not held".into())
            }
        }
    }

    #[test]
    fn default_parses_to_the_expected_keys() {
        let s: Shortcut = DEFAULT.parse().unwrap();
        if cfg!(target_os = "macos") {
            assert!(s.matches(
                Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::SUPER,
                Code::KeyS
            ));
        } else {
            assert!(s.matches(Modifiers::empty(), Code::PrintScreen));
        }
        assert_eq!(validate(DEFAULT), Ok(()));
    }

    #[test]
    fn validation_needs_a_parsable_shortcut_with_a_modifier() {
        assert_eq!(validate("Control+Shift+Super+S"), Ok(()));
        assert_eq!(validate("Alt+F5"), Ok(()));
        assert_eq!(validate("Super+ArrowUp"), Ok(()));
        assert_eq!(
            validate("Shift+S"),
            Err(NO_MODIFIER),
            "Shift alone takes over typing"
        );
        assert_eq!(validate("Shift+F5"), Err(NO_MODIFIER));
        assert_eq!(validate("S"), Err(NO_MODIFIER));
        assert_eq!(validate("F5"), Err(NO_MODIFIER));
        assert_eq!(validate("Control+Nope"), Err(INVALID));
        assert_eq!(validate(""), Err(INVALID));
        assert_eq!(validate("Control+Shift"), Err(INVALID));
        // Bare PrintScreen only where it is the default (Windows).
        let bare = if DEFAULT == "PrintScreen" {
            Ok(())
        } else {
            Err(NO_MODIFIER)
        };
        assert_eq!(validate("PrintScreen"), bare);
    }

    #[test]
    fn swap_replaces_the_old_shortcut() {
        let (fake, registered) = (FakeRegistrar::holding("Alt+S"), holding("Alt+S"));
        assert_eq!(swap(&fake, &registered, "Alt+D"), Ok(()));
        assert_eq!(fake.held(), ["Alt+D"]);
        assert_eq!(registered.active().as_deref(), Some("Alt+D"));
    }

    #[test]
    fn swap_to_the_same_shortcut_does_nothing() {
        let (fake, registered) = (FakeRegistrar::holding("Alt+S"), holding("Alt+S"));
        assert_eq!(swap(&fake, &registered, "Alt+S"), Ok(()));
        assert_eq!(fake.held(), ["Alt+S"]);
    }

    #[test]
    fn swap_keeps_the_old_shortcut_when_the_new_one_is_taken() {
        let (mut fake, registered) = (FakeRegistrar::holding("Alt+S"), holding("Alt+S"));
        fake.taken.insert("Alt+D".into());
        assert_eq!(swap(&fake, &registered, "Alt+D"), Err(IN_USE));
        assert_eq!(fake.held(), ["Alt+S"]);
        assert_eq!(registered.active().as_deref(), Some("Alt+S"));
    }

    #[test]
    fn swap_with_nothing_registered_registers_the_new_one() {
        let (fake, registered) = (FakeRegistrar::default(), Registered::default());
        assert_eq!(swap(&fake, &registered, "Alt+D"), Ok(()));
        assert_eq!(fake.held(), ["Alt+D"]);
        let (mut fake, registered) = (FakeRegistrar::default(), Registered::default());
        fake.taken.insert("Alt+D".into());
        assert_eq!(swap(&fake, &registered, "Alt+D"), Err(IN_USE));
        assert!(fake.held().is_empty());
        assert_eq!(registered.active(), None);
    }

    #[test]
    fn swap_marks_the_old_shortcut_lost_if_it_cannot_come_back() {
        // Another app grabs the old one as soon as it is let go, and the new one is taken.
        let (mut fake, registered) = (FakeRegistrar::holding("Alt+S"), holding("Alt+S"));
        fake.taken.extend(["Alt+S".to_owned(), "Alt+D".to_owned()]);
        assert_eq!(swap(&fake, &registered, "Alt+D"), Err(IN_USE));
        assert!(fake.held().is_empty());
        assert_eq!(
            registered.get(),
            CaptureShortcut {
                accelerator: "Alt+S".into(),
                registered: false
            }
        );
    }

    #[test]
    fn swap_changes_nothing_if_the_old_shortcut_cannot_be_let_go() {
        // Registered says Alt+S, but the registrar doesn't hold it: unregister fails.
        let (fake, registered) = (FakeRegistrar::default(), holding("Alt+S"));
        assert_eq!(swap(&fake, &registered, "Alt+D"), Err(NOT_CHANGED));
        assert!(fake.held().is_empty(), "the new one isn't registered");
        assert_eq!(registered.active().as_deref(), Some("Alt+S"));
        assert!(registered.is_capture(&"Alt+S".parse().unwrap()));
    }

    fn holding(accelerator: &str) -> Registered {
        let registered = Registered::default();
        registered.set(accelerator, true);
        registered
    }

    #[test]
    fn resuming_without_global_shortcuts_does_nothing() {
        let app = mock_builder()
            .manage(Registered::default())
            .build(mock_context(noop_assets()))
            .unwrap();
        resume_capture(app.handle());
        assert!(!app.state::<Registered>().get().registered);
    }

    #[test]
    fn pause_lets_go_and_resume_takes_it_back() {
        let (fake, registered) = (FakeRegistrar::holding("Alt+S"), holding("Alt+S"));
        let alt_s: Shortcut = "Alt+S".parse().unwrap();
        assert_eq!(pause(&fake, &registered), Ok(()));
        assert!(fake.held().is_empty());
        assert_eq!(registered.active(), None);
        assert!(!registered.is_capture(&alt_s));
        assert_eq!(pause(&fake, &registered), Ok(()), "nothing more to let go");
        resume(&fake, &registered);
        assert_eq!(fake.held(), ["Alt+S"]);
        assert!(registered.is_capture(&alt_s));
        resume(&fake, &registered); // Not paused: nothing to do.
        assert_eq!(fake.held(), ["Alt+S"]);
    }

    #[test]
    fn re_recording_the_same_shortcut_while_paused_registers_it() {
        let (fake, registered) = (FakeRegistrar::holding("Alt+S"), holding("Alt+S"));
        pause(&fake, &registered).unwrap();
        assert_eq!(swap(&fake, &registered, "Alt+S"), Ok(()));
        assert_eq!(fake.held(), ["Alt+S"]);
        resume(&fake, &registered); // Already back: nothing to do.
        assert_eq!(registered.active().as_deref(), Some("Alt+S"));
    }

    #[test]
    fn a_taken_shortcut_while_paused_leaves_the_old_one_to_resume() {
        let (mut fake, registered) = (FakeRegistrar::holding("Alt+S"), holding("Alt+S"));
        fake.taken.insert("Alt+D".into());
        pause(&fake, &registered).unwrap();
        assert_eq!(swap(&fake, &registered, "Alt+D"), Err(IN_USE));
        resume(&fake, &registered);
        assert_eq!(fake.held(), ["Alt+S"]);
        assert_eq!(registered.active().as_deref(), Some("Alt+S"));
    }

    #[test]
    fn a_shortcut_taken_while_paused_is_marked_unregistered() {
        let (mut fake, registered) = (FakeRegistrar::holding("Alt+S"), holding("Alt+S"));
        pause(&fake, &registered).unwrap();
        fake.taken.insert("Alt+S".into());
        resume(&fake, &registered);
        assert!(!registered.get().registered);
    }

    #[test]
    fn a_shortcut_that_cannot_be_let_go_stays_active() {
        let (fake, registered) = (FakeRegistrar::default(), holding("Alt+S"));
        assert_eq!(pause(&fake, &registered), Err(NOT_CHANGED));
        assert_eq!(registered.active().as_deref(), Some("Alt+S"));
        assert!(registered.is_capture(&"Alt+S".parse().unwrap()));
        resume(&fake, &registered); // Not paused: nothing to do.
        assert!(fake.held().is_empty());
    }

    #[test]
    fn knows_the_registered_capture_shortcut() {
        let app = mock_builder()
            .manage(Registered::default())
            .build(mock_context(noop_assets()))
            .unwrap();
        let registered = app.state::<Registered>();
        let alt_s: Shortcut = "Alt+S".parse().unwrap();
        let expected = CaptureShortcut {
            accelerator: DEFAULT.into(),
            registered: false,
        };
        assert_eq!(capture_shortcut(registered.clone()), expected);
        assert_eq!(registered.active(), None);
        registered.set("Alt+S", true);
        assert_eq!(registered.active(), Some("Alt+S".into()));
        assert!(registered.is_capture(&alt_s));
        assert!(!registered.is_capture(&"Alt+D".parse().unwrap()));
        registered.set("Alt+S", false);
        assert!(!registered.is_capture(&alt_s));
        assert!(!capture_shortcut(registered).registered);
    }
}
