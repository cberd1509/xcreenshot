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

    /// The menu bar rows, `None` for a separator. "Restart to update" is only there once an
    /// update is downloaded, right under "Check for updates…".
    pub fn menu_rows(update_ready: bool) -> Vec<Option<Action>> {
        let mut rows = Vec::new();
        for action in Action::MENU {
            if action == Action::RestartToUpdate && !update_ready {
                continue;
            }
            if matches!(action, Action::OpenSettings | Action::Quit) {
                rows.push(None);
            }
            rows.push(Some(action));
        }
        rows
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::MENU.into_iter().find(|a| a.id() == id)
    }
}

/// What the menu bar shows that can change: the capture shortcut and a pending update.
type Shown = (String, Option<String>);

/// Remembers what the menu bar shows, so it is rebuilt only when that changes.
#[derive(Default)]
pub struct MenuShows(std::sync::Mutex<Option<Shown>>);

impl MenuShows {
    /// Runs `install` if `capture` and `update` differ from what is shown, and remembers them
    /// only if it succeeds. One refresh at a time, so an older one can't install over a newer.
    pub fn refresh<E>(
        &self,
        capture: &str,
        update: Option<&str>,
        install: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), E> {
        let next = Some((capture.to_owned(), update.map(str::to_owned)));
        let mut shown = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *shown != next {
            install()?;
            *shown = next;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_changes_only_with_the_shortcut_or_update() {
        let shows = MenuShows::default();
        let installs = std::cell::Cell::new(0);
        let install = || {
            installs.set(installs.get() + 1);
            Ok::<(), ()>(())
        };
        for (capture, update) in [
            ("Alt+S", None),
            ("Alt+S", None),
            ("Alt+D", None),
            ("Alt+D", Some("1.2.0")),
            ("Alt+D", Some("1.2.0")),
        ] {
            shows.refresh(capture, update, install).unwrap();
        }
        assert_eq!(installs.get(), 3);
    }

    #[test]
    fn a_failed_install_is_tried_again() {
        let shows = MenuShows::default();
        assert_eq!(
            shows.refresh("Alt+S", None, || Err("no tray")),
            Err("no tray")
        );
        let mut installed = false;
        shows
            .refresh("Alt+S", None, || {
                installed = true;
                Ok::<(), &str>(())
            })
            .unwrap();
        assert!(installed, "nothing was remembered after the failure");
    }

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

    #[test]
    fn separators_group_capture_the_app_items_and_quit() {
        use Action::*;
        assert_eq!(
            Action::menu_rows(false),
            [
                Some(Capture),
                None,
                Some(OpenSettings),
                Some(CheckForUpdates),
                None,
                Some(Quit)
            ]
        );
        assert_eq!(
            Action::menu_rows(true),
            [
                Some(Capture),
                None,
                Some(OpenSettings),
                Some(CheckForUpdates),
                Some(RestartToUpdate),
                None,
                Some(Quit)
            ]
        );
    }
}
