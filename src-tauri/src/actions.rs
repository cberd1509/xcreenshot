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

    pub fn id(self) -> &'static str {
        match self {
            Action::Capture => "capture",
            Action::OpenSettings => "settings",
            Action::Quit => "quit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Capture => "Capture",
            Action::OpenSettings => "Settings…",
            Action::Quit => "Quit xcreenshot",
        }
    }

    /// Keyboard shortcut shown next to the menu item, if any.
    pub fn menu_accelerator(self) -> Option<&'static str> {
        match self {
            Action::Capture => None, // set from the user's capture shortcut
            Action::OpenSettings => Some("CmdOrCtrl+,"),
            Action::Quit => Some("CmdOrCtrl+Q"),
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
        }
    }

    #[test]
    fn unknown_id_is_ignored() {
        assert_eq!(Action::from_id("nope"), None);
    }

    #[test]
    fn labels_and_accelerators() {
        assert_eq!(Action::Capture.label(), "Capture");
        assert_eq!(Action::OpenSettings.label(), "Settings…");
        assert_eq!(Action::Quit.label(), "Quit xcreenshot");
        assert_eq!(Action::Capture.menu_accelerator(), None);
        assert_eq!(Action::OpenSettings.menu_accelerator(), Some("CmdOrCtrl+,"));
        assert_eq!(Action::Quit.menu_accelerator(), Some("CmdOrCtrl+Q"));
    }
}
