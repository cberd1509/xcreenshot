//! The global capture shortcut (same accelerator format as the frontend settings).

use std::sync::{Mutex, PoisonError};
use tauri::State;

/// Default capture shortcut on this platform.
#[cfg(target_os = "macos")]
pub const DEFAULT: &str = "Control+Shift+Super+S";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT: &str = "PrintScreen";

/// The capture shortcut that is actually registered, if any (another app may own it).
#[derive(Default)]
pub struct Registered(Mutex<Option<String>>);

impl Registered {
    pub fn set(&self, accelerator: Option<&str>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = accelerator.map(String::from);
    }

    fn get(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The registered capture shortcut, or `None` if registration failed.
#[tauri::command]
pub fn capture_shortcut(registered: State<'_, Registered>) -> Option<String> {
    registered.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

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
    }

    #[test]
    fn reports_the_registered_shortcut() {
        let app = mock_builder()
            .manage(Registered::default())
            .build(mock_context(noop_assets()))
            .unwrap();
        let registered = app.state::<Registered>();
        assert_eq!(capture_shortcut(registered.clone()), None);
        registered.set(Some(DEFAULT));
        assert_eq!(
            capture_shortcut(registered.clone()),
            Some(DEFAULT.to_string())
        );
        registered.set(None);
        assert_eq!(capture_shortcut(registered), None);
    }
}
