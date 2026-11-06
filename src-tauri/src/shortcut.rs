//! The global capture shortcut (same accelerator format as the frontend settings).

/// Default capture shortcut on this platform.
#[cfg(target_os = "macos")]
pub const DEFAULT: &str = "Control+Shift+Super+S";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT: &str = "PrintScreen";

#[cfg(test)]
mod tests {
    use super::DEFAULT;
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
}
