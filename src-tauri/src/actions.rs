//! Things the user can trigger from the menu bar or the global shortcut.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Capture,
    OpenSettings,
    Quit,
}

impl Action {
    /// Menu bar items, in display order.
    pub const MENU: [Action; 3] = [Action::Capture, Action::OpenSettings, Action::Quit];

    /// Menu id, label and fixed keyboard shortcut (Capture's comes from the user's setting).
    fn spec(self) -> (&'static str, &'static str, Option<&'static str>) {
        match self {
            Action::Capture => ("capture", "Capture", None),
            Action::OpenSettings => ("settings", "Settings…", Some("CmdOrCtrl+,")),
            Action::Quit => ("quit", "Quit xcreenshot", Some("CmdOrCtrl+Q")),
        }
    }

    pub fn id(self) -> &'static str {
        self.spec().0
    }

    pub fn label(self) -> &'static str {
        self.spec().1
    }

    /// Shortcut shown next to the menu item.
    pub fn accelerator(self, capture: &str) -> Option<&str> {
        match self {
            Action::Capture => Some(capture),
            _ => self.spec().2,
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::MENU.into_iter().find(|a| a.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for action in Action::MENU {
            assert_eq!(Action::from_id(action.id()), Some(action));
            assert!(!action.label().is_empty());
        }
        assert_eq!(Action::from_id("nope"), None);
    }

    #[test]
    fn capture_uses_the_user_shortcut() {
        assert_eq!(Action::Capture.accelerator("F13"), Some("F13"));
        assert_eq!(Action::Quit.accelerator("F13"), Some("CmdOrCtrl+Q"));
    }
}
