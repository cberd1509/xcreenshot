//! Things the user can trigger from the menu bar or the global shortcut.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Capture,
    OpenSettings,
    CheckForUpdates,
    /// Only shown once an update is downloaded.
    RestartToUpdate,
    Quit,
}

impl Action {
    /// Menu bar items, in display order.
    pub const MENU: [Action; 5] = [
        Action::Capture,
        Action::OpenSettings,
        Action::CheckForUpdates,
        Action::RestartToUpdate,
        Action::Quit,
    ];

    /// Menu id, label and fixed keyboard shortcut (Capture's comes from the user's setting).
    fn spec(self) -> (&'static str, &'static str, Option<&'static str>) {
        match self {
            Action::Capture => ("capture", "Capture", None),
            Action::OpenSettings => ("settings", "Settings…", Some("CmdOrCtrl+,")),
            Action::CheckForUpdates => ("check-updates", "Check for updates…", None),
            Action::RestartToUpdate => ("restart-to-update", "Restart to update", None),
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

    /// Whether a separator follows this item in the menu bar.
    pub fn separator_after(self) -> bool {
        matches!(self, Action::Capture | Action::CheckForUpdates)
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

    #[test]
    fn menu_order_matches_the_agreed_tray() {
        assert_eq!(
            Action::MENU.map(Action::label),
            [
                "Capture",
                "Settings…",
                "Check for updates…",
                "Restart to update",
                "Quit xcreenshot"
            ]
        );
        assert_eq!(
            Action::from_id("check-updates"),
            Some(Action::CheckForUpdates)
        );
        assert_eq!(Action::CheckForUpdates.accelerator("F13"), None);
    }

    /// The tray is built from `MENU` alone, separators included.
    #[test]
    fn separators_group_capture_and_the_app_items() {
        let after: Vec<_> = Action::MENU
            .into_iter()
            .filter(|a| a.separator_after())
            .collect();
        assert_eq!(after, [Action::Capture, Action::CheckForUpdates]);
    }
}
