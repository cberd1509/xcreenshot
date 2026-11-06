//! Open at login: the `open_at_login` pref drives the system login item (a LaunchAgent on
//! macOS, a `Run` registry value on Windows, through tauri-plugin-autostart).
//!
//! Debug builds never touch the login item, so `pnpm tauri dev` doesn't add the dev binary.
//!
//! Known limitation (macOS): the plugin reports the login item as on while its LaunchAgent
//! file exists. Turning xcreenshot off under System Settings › General › Login Items ›
//! "Allow in the Background" keeps that file, so [`sync`] can't see it and the pref stays on.
//! Only a deleted LaunchAgent file counts as removed.

use crate::prefs::Prefs;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_autostart::AutoLaunchManager;

pub const FAILED: &str = "Couldn't change the login item.";

/// The system login item (the plugin, or a fake in tests).
pub trait LoginItem {
    fn is_enabled(&self) -> Result<bool, String>;
    fn set_enabled(&self, on: bool) -> Result<(), String>;
}

impl LoginItem for AutoLaunchManager {
    fn is_enabled(&self) -> Result<bool, String> {
        AutoLaunchManager::is_enabled(self).map_err(|e| e.to_string())
    }

    fn set_enabled(&self, on: bool) -> Result<(), String> {
        if on { self.enable() } else { self.disable() }.map_err(|e| e.to_string())
    }
}

/// The app's login item, or `None` in debug builds or if the plugin isn't set up.
pub fn item<R: Runtime>(app: &AppHandle<R>) -> Option<tauri::State<'_, AutoLaunchManager>> {
    if cfg!(debug_assertions) {
        return None;
    }
    app.try_state::<AutoLaunchManager>()
}

/// Turns the login item on or off for the user's choice and remembers that it was added.
pub fn set(item: Option<&impl LoginItem>, prefs: &mut Prefs, on: bool) -> Result<(), &'static str> {
    if let Some(item) = item {
        item.set_enabled(on).map_err(|e| {
            log::error!(
                "could not turn {} the login item: {e}",
                if on { "on" } else { "off" }
            );
            FAILED
        })?;
        prefs.login_item_added |= on;
    }
    prefs.open_at_login = on;
    Ok(())
}

/// Brings the login item and the pref in line at startup. The first run adds the login item
/// (the pref defaults to on). If it was added before and is gone now, the user removed it in
/// the system settings, so the pref turns off. Returns what it did to the login item: turned
/// it on or off, or nothing. A failure is logged (by [`set`]) and leaves everything as it was,
/// so the next launch tries again.
pub fn sync(item: &impl LoginItem, prefs: &mut Prefs) -> Option<bool> {
    let enabled = match item.is_enabled() {
        Ok(enabled) => enabled,
        Err(e) => {
            log::warn!("could not check the login item: {e}");
            return None;
        }
    };
    match (prefs.open_at_login, enabled) {
        (true, true) => prefs.login_item_added = true,
        (true, false) if prefs.login_item_added => {
            log::info!("the login item was removed in the system settings; turning it off");
            prefs.open_at_login = false;
        }
        (true, false) => return set(Some(item), prefs, true).ok().map(|()| true),
        (false, true) => return set(Some(item), prefs, false).ok().map(|()| false),
        (false, false) => {}
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::Cell;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    /// A login item in memory; `broken` makes every call fail.
    #[derive(Default)]
    pub struct FakeItem {
        pub on: Cell<bool>,
        pub broken: bool,
    }

    impl LoginItem for FakeItem {
        fn is_enabled(&self) -> Result<bool, String> {
            if self.broken {
                return Err("broken".into());
            }
            Ok(self.on.get())
        }

        fn set_enabled(&self, on: bool) -> Result<(), String> {
            if self.broken {
                return Err("broken".into());
            }
            self.on.set(on);
            Ok(())
        }
    }

    fn prefs(open_at_login: bool, login_item_added: bool) -> Prefs {
        Prefs {
            open_at_login,
            login_item_added,
            ..Prefs::default()
        }
    }

    fn item(on: bool) -> FakeItem {
        FakeItem {
            on: Cell::new(on),
            broken: false,
        }
    }

    #[test]
    fn first_run_adds_the_login_item() {
        let (item, mut p) = (item(false), Prefs::default());
        assert_eq!(sync(&item, &mut p), Some(true));
        assert!(item.on.get());
        assert_eq!(p, prefs(true, true));
        assert_eq!(sync(&item, &mut p), None, "nothing to do the second time");
        assert_eq!(p, prefs(true, true));
    }

    #[test]
    fn a_login_item_removed_in_system_settings_turns_the_pref_off() {
        let (item, mut p) = (item(false), prefs(true, true));
        assert_eq!(sync(&item, &mut p), None);
        assert!(!item.on.get());
        assert_eq!(p, prefs(false, true));
    }

    #[test]
    fn a_pref_turned_off_removes_a_leftover_login_item() {
        let (item, mut p) = (item(true), prefs(false, true));
        assert_eq!(sync(&item, &mut p), Some(false));
        assert!(!item.on.get());
        assert_eq!(p, prefs(false, true));
        let (item, mut p) = (self::item(false), prefs(false, true));
        assert_eq!(sync(&item, &mut p), None);
        assert!(!item.on.get());
    }

    #[test]
    fn an_existing_login_item_is_marked_as_added() {
        let (item, mut p) = (item(true), prefs(true, false));
        assert_eq!(sync(&item, &mut p), None);
        assert_eq!(p, prefs(true, true));
    }

    /// Checks fine, but turning it on or off fails.
    struct Stuck(bool);

    impl LoginItem for Stuck {
        fn is_enabled(&self) -> Result<bool, String> {
            Ok(self.0)
        }

        fn set_enabled(&self, _: bool) -> Result<(), String> {
            Err("denied".into())
        }
    }

    #[test]
    fn a_login_item_that_cannot_change_leaves_everything_for_next_time() {
        let mut p = Prefs::default();
        assert_eq!(sync(&Stuck(false), &mut p), None);
        assert_eq!(
            p,
            Prefs::default(),
            "not marked as added, so it is tried again"
        );
        let mut p = prefs(false, true);
        assert_eq!(sync(&Stuck(true), &mut p), None);
        assert_eq!(p, prefs(false, true));
    }

    #[test]
    fn a_broken_login_item_changes_nothing() {
        let broken = FakeItem {
            broken: true,
            ..FakeItem::default()
        };
        let mut p = Prefs::default();
        assert_eq!(sync(&broken, &mut p), None);
        assert_eq!(p, Prefs::default());
        assert_eq!(set(Some(&broken), &mut p, false), Err(FAILED));
        assert_eq!(p, Prefs::default());
    }

    #[test]
    fn set_follows_the_choice() {
        let (item, mut p) = (item(false), prefs(false, false));
        set(Some(&item), &mut p, true).unwrap();
        assert!(item.on.get());
        assert_eq!(p, prefs(true, true));
        set(Some(&item), &mut p, false).unwrap();
        assert!(!item.on.get());
        assert_eq!(p, prefs(false, true));
        // Without a login item (debug builds) only the choice changes: nothing was added.
        let mut p = prefs(false, false);
        set(None::<&FakeItem>, &mut p, true).unwrap();
        assert_eq!(p, prefs(true, false));
    }

    #[test]
    fn debug_builds_have_no_login_item_even_with_the_plugin() {
        let app = mock_builder()
            .plugin(tauri_plugin_autostart::Builder::new().build())
            .build(mock_context(noop_assets()))
            .unwrap();
        assert!(app.try_state::<AutoLaunchManager>().is_some());
        assert!(super::item(app.handle()).is_none());
    }
}
