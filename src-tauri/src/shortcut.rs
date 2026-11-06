//! The global capture shortcut.

use tauri_plugin_global_shortcut::Shortcut;

/// Default capture shortcut for the current platform (same format as the frontend settings).
pub fn default_accelerator() -> &'static str {
    default_for(cfg!(target_os = "macos"))
}

fn default_for(mac: bool) -> &'static str {
    if mac {
        "Control+Shift+Super+S"
    } else {
        "PrintScreen"
    }
}

/// Parses an accelerator such as "Control+Shift+Super+S".
pub fn parse(accelerator: &str) -> Result<Shortcut, String> {
    accelerator
        .parse::<Shortcut>()
        .map_err(|e| format!("invalid shortcut {accelerator:?}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_global_shortcut::{Code, Modifiers};

    #[test]
    fn mac_default_is_control_shift_command_s() {
        let s = parse(default_for(true)).unwrap();
        assert!(s.matches(
            Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::SUPER,
            Code::KeyS
        ));
    }

    #[test]
    fn other_default_is_print_screen() {
        let s = parse(default_for(false)).unwrap();
        assert!(s.matches(Modifiers::empty(), Code::PrintScreen));
    }

    #[test]
    fn platform_default_parses() {
        assert!(parse(default_accelerator()).is_ok());
    }

    #[test]
    fn invalid_shortcut_is_an_error() {
        let err = parse("Control+NotAKey").unwrap_err();
        assert!(err.contains("Control+NotAKey"));
    }
}
