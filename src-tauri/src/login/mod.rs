//! Open at login: the `open_at_login` pref drives the system login item. On macOS that is
//! `SMAppService.mainApp` (see [`macos`]), so System Settings › General › Login Items lists
//! xcreenshot by name; on Windows it is a `Run` registry value, through tauri-plugin-autostart.
//!
//! Debug builds never touch the login item, so `pnpm tauri dev` doesn't add the dev binary.
//!
//! Versions before SMAppService added a LaunchAgent (`~/Library/LaunchAgents/xcreenshot.plist`).
//! [`sync`] removes it once, then registers with SMAppService if the pref is on.

#[cfg(target_os = "macos")]
mod macos;

use crate::prefs::Prefs;
use std::io;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

pub const FAILED: &str = "Couldn't change the login item.";

/// The login item in effect on this platform.
#[cfg(target_os = "macos")]
pub use macos::MainApp as System;
#[cfg(not(target_os = "macos"))]
pub use tauri_plugin_autostart::AutoLaunchManager as System;

/// The state of the login item, as `SMAppService.Status` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Enabled,
    NotRegistered,
    /// Registered, but the user has to allow it in System Settings › Login Items.
    RequiresApproval,
    NotFound,
}

impl Status {
    /// From `SMAppServiceStatus`. An unknown value counts as [`Status::NotFound`].
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn from_raw(raw: isize) -> Self {
        match raw {
            0 => Self::NotRegistered,
            1 => Self::Enabled,
            2 => Self::RequiresApproval,
            _ => Self::NotFound,
        }
    }

    /// Registered, whether or not the user has allowed it yet.
    fn is_on(self) -> bool {
        matches!(self, Self::Enabled | Self::RequiresApproval)
    }
}

/// The system login item (SMAppService, the autostart plugin, or a fake in tests).
pub trait LoginItem {
    fn status(&self) -> Result<Status, String>;
    fn set_enabled(&self, on: bool) -> Result<(), String>;
    /// Removes the LaunchAgent older versions added. Returns whether there was one.
    fn remove_launch_agent(&self) -> Result<bool, String> {
        Ok(false)
    }
}

#[cfg(not(target_os = "macos"))]
impl LoginItem for System {
    fn status(&self) -> Result<Status, String> {
        match self.is_enabled() {
            Ok(true) => Ok(Status::Enabled),
            Ok(false) => Ok(Status::NotRegistered),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set_enabled(&self, on: bool) -> Result<(), String> {
        if on { self.enable() } else { self.disable() }.map_err(|e| e.to_string())
    }
}

/// The app's login item, or `None` in debug builds or if it isn't set up.
pub fn item<R: Runtime>(app: &AppHandle<R>) -> Option<tauri::State<'_, System>> {
    if cfg!(debug_assertions) {
        return None;
    }
    app.try_state::<System>()
}

/// The LaunchAgent tauri-plugin-autostart wrote, under `home`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn launch_agent(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents/xcreenshot.plist")
}

/// Deletes `path` if it is there, as the plugin's own `disable` did. It isn't unloaded with
/// `launchctl bootout`: when launched at login, xcreenshot runs as that job, so unloading it
/// would quit the app. Without its file, launchd doesn't load it at the next login.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn remove_file(path: &Path) -> Result<bool, String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Turns the login item on or off for the user's choice and remembers that it was added. A
/// failure where the item already is as asked (unregistering one that isn't registered, or
/// registering one that waits for approval) still counts as done.
pub fn set(item: Option<&impl LoginItem>, prefs: &mut Prefs, on: bool) -> Result<(), &'static str> {
    if let Some(item) = item {
        if let Err(e) = item.set_enabled(on) {
            if item.status().map(Status::is_on) != Ok(on) {
                log::error!(
                    "could not turn {} the login item: {e}",
                    if on { "on" } else { "off" }
                );
                return Err(FAILED);
            }
            log::info!(
                "the login item is already {}: {e}",
                if on { "on" } else { "off" }
            );
        }
        prefs.login_item_added |= on;
    }
    prefs.open_at_login = on;
    Ok(())
}

/// Whether the login item waits for the user to allow it in System Settings.
pub fn needs_approval(item: &impl LoginItem) -> bool {
    item.status() == Ok(Status::RequiresApproval)
}

/// Removes the old LaunchAgent, once. If it was there, it was the login item the app added, so
/// SMAppService's has yet to be added. Returns false if it couldn't be removed (logged), to try
/// again at the next launch.
fn migrate(item: &impl LoginItem, prefs: &mut Prefs) -> bool {
    if prefs.launch_agent_removed {
        return true;
    }
    match item.remove_launch_agent() {
        Ok(found) => {
            if found {
                log::info!("removed the old LaunchAgent");
                prefs.login_item_added = false;
            }
            prefs.launch_agent_removed = true;
            true
        }
        Err(e) => {
            log::warn!("could not remove the old LaunchAgent: {e}");
            false
        }
    }
}

/// Brings the login item and the pref in line at startup, after removing the old LaunchAgent
/// (see [`migrate`]). The first run adds the login item (the pref defaults to on). If it was
/// added before and the user turned it off in System Settings, the pref turns off. Returns
/// what it did to the login item: turned it on or off, or nothing. A failure is logged and
/// leaves the login item and pref as they were, so the next launch tries again.
pub fn sync(item: &impl LoginItem, prefs: &mut Prefs) -> Option<bool> {
    if !migrate(item, prefs) {
        return None;
    }
    let status = match item.status() {
        Ok(status) => status,
        Err(e) => {
            log::warn!("could not check the login item: {e}");
            return None;
        }
    };
    match (prefs.open_at_login, status.is_on()) {
        (true, true) => prefs.login_item_added = true,
        // NotFound isn't something the user can do; register again.
        (true, false) if prefs.login_item_added && status == Status::NotRegistered => {
            log::info!("the login item was turned off in the system settings; turning it off");
            prefs.open_at_login = false;
        }
        (true, false) => return set(Some(item), prefs, true).ok().map(|()| true),
        (false, true) => return set(Some(item), prefs, false).ok().map(|()| false),
        (false, false) => {}
    }
    None
}

/// Whether Settings should ask the user to allow the login item in System Settings.
#[tauri::command]
pub async fn login_item_needs_approval<R: Runtime>(app: AppHandle<R>) -> bool {
    item(&app).is_some_and(|item| needs_approval(&*item))
}

/// Opens System Settings › General › Login Items.
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn open_login_items() {
    macos::open_login_items();
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use Status::*;
    use std::cell::Cell;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    /// A login item in memory. `waiting` makes it wait for approval when on, `missing` makes
    /// it not found when off, and `broken` makes every call fail. `launch_agent` is the old
    /// LaunchAgent, `Err` if it can't be removed.
    pub struct FakeItem {
        pub on: Cell<bool>,
        pub waiting: bool,
        pub missing: bool,
        pub broken: bool,
        pub launch_agent: Cell<Result<bool, ()>>,
    }

    impl Default for FakeItem {
        fn default() -> Self {
            Self {
                on: Cell::default(),
                waiting: false,
                missing: false,
                broken: false,
                launch_agent: Cell::new(Ok(false)),
            }
        }
    }

    impl LoginItem for FakeItem {
        fn status(&self) -> Result<Status, String> {
            if self.broken {
                return Err("broken".into());
            }
            Ok(match (self.on.get(), self.waiting, self.missing) {
                (true, false, _) => Enabled,
                (true, true, _) => RequiresApproval,
                (false, _, false) => NotRegistered,
                (false, _, true) => NotFound,
            })
        }

        fn set_enabled(&self, on: bool) -> Result<(), String> {
            if self.broken {
                return Err("broken".into());
            }
            self.on.set(on);
            Ok(())
        }

        fn remove_launch_agent(&self) -> Result<bool, String> {
            let found = self.launch_agent.get().map_err(|()| "locked".to_owned())?;
            self.launch_agent.set(Ok(false));
            Ok(found)
        }
    }

    pub(crate) fn prefs(open_at_login: bool, login_item_added: bool) -> Prefs {
        Prefs {
            open_at_login,
            login_item_added,
            launch_agent_removed: true,
            ..Prefs::default()
        }
    }

    fn item(status: Status) -> FakeItem {
        FakeItem {
            on: Cell::new(status.is_on()),
            waiting: status == RequiresApproval,
            missing: status == NotFound,
            ..FakeItem::default()
        }
    }

    #[test]
    fn status_follows_smappservice() {
        let all = [NotRegistered, Enabled, RequiresApproval, NotFound];
        for (raw, status) in (0..).zip(all) {
            assert_eq!(Status::from_raw(raw), status);
        }
        assert_eq!(Status::from_raw(7), NotFound);
        assert_eq!(Status::from_raw(-1), NotFound);
    }

    #[test]
    fn first_run_adds_the_login_item() {
        let (item, mut p) = (item(NotRegistered), prefs(true, false));
        assert_eq!(sync(&item, &mut p), Some(true));
        assert!(item.on.get());
        assert_eq!(p, prefs(true, true));
        assert_eq!(sync(&item, &mut p), None, "nothing to do the second time");
        assert_eq!(p, prefs(true, true));
    }

    #[test]
    fn a_login_item_turned_off_in_system_settings_turns_the_pref_off() {
        let (item, mut p) = (item(NotRegistered), prefs(true, true));
        assert_eq!(sync(&item, &mut p), None);
        assert!(!item.on.get());
        assert_eq!(p, prefs(false, true));
    }

    #[test]
    fn a_login_item_that_is_not_found_is_added_again() {
        let (item, mut p) = (item(NotFound), prefs(true, true));
        assert_eq!(sync(&item, &mut p), Some(true));
        assert_eq!(item.status().unwrap(), Enabled);
        assert_eq!(p, prefs(true, true));
    }

    #[test]
    fn a_login_item_waiting_for_approval_keeps_the_pref_on() {
        let (item, mut p) = (item(RequiresApproval), prefs(true, false));
        assert_eq!(sync(&item, &mut p), None);
        assert_eq!(
            item.status().unwrap(),
            RequiresApproval,
            "left for the user"
        );
        assert_eq!(p, prefs(true, true));
        assert!(needs_approval(&item));
        assert!(!needs_approval(&self::item(Enabled)));
        let broken = FakeItem {
            broken: true,
            ..FakeItem::default()
        };
        assert!(!needs_approval(&broken));
    }

    #[test]
    fn a_pref_turned_off_removes_a_leftover_login_item() {
        for status in [Enabled, RequiresApproval] {
            let (item, mut p) = (item(status), prefs(false, true));
            assert_eq!(sync(&item, &mut p), Some(false));
            assert!(!item.on.get());
            assert_eq!(p, prefs(false, true));
        }
        for status in [NotRegistered, NotFound] {
            let (item, mut p) = (item(status), prefs(false, true));
            assert_eq!(sync(&item, &mut p), None);
            assert_eq!(item.status().unwrap(), status);
        }
    }

    #[test]
    fn an_existing_login_item_is_marked_as_added() {
        let (item, mut p) = (item(Enabled), prefs(true, false));
        assert_eq!(sync(&item, &mut p), None);
        assert_eq!(p, prefs(true, true));
    }

    #[test]
    fn the_old_launch_agent_is_replaced_once() {
        // Added by an older version: removed, and SMAppService's login item added instead,
        // not mistaken for the user turning it off.
        let item = FakeItem {
            launch_agent: Cell::new(Ok(true)),
            ..FakeItem::default()
        };
        let mut p = Prefs {
            login_item_added: true,
            ..Prefs::default()
        };
        assert_eq!(sync(&item, &mut p), Some(true));
        assert_eq!(item.launch_agent.get(), Ok(false));
        assert!(item.on.get());
        assert_eq!(p, prefs(true, true));
        // Only once: a LaunchAgent that shows up later is left alone.
        item.launch_agent.set(Ok(true));
        assert_eq!(sync(&item, &mut p), None);
        assert_eq!(item.launch_agent.get(), Ok(true));

        // With the pref off it is only removed.
        let item = FakeItem {
            launch_agent: Cell::new(Ok(true)),
            ..FakeItem::default()
        };
        let mut p = Prefs {
            open_at_login: false,
            login_item_added: true,
            ..Prefs::default()
        };
        assert_eq!(sync(&item, &mut p), None);
        assert!(!item.on.get());
        assert_eq!(p, prefs(false, false));
    }

    #[test]
    fn without_an_old_launch_agent_the_migration_changes_nothing_else() {
        // The user had removed it: the pref turns off, as before.
        let (item, mut p) = (
            FakeItem::default(),
            Prefs {
                login_item_added: true,
                ..Prefs::default()
            },
        );
        assert_eq!(sync(&item, &mut p), None);
        assert_eq!(p, prefs(false, true));
    }

    #[test]
    fn a_launch_agent_that_cannot_be_removed_is_tried_again_next_time() {
        let item = FakeItem {
            launch_agent: Cell::new(Err(())),
            ..FakeItem::default()
        };
        let mut p = Prefs::default();
        assert_eq!(sync(&item, &mut p), None);
        assert!(
            !item.on.get(),
            "no second login item while the old one is there"
        );
        assert_eq!(p, Prefs::default());
        item.launch_agent.set(Ok(true));
        assert_eq!(sync(&item, &mut p), Some(true));
        assert_eq!(p, prefs(true, true));
    }

    /// Checks fine, but turning it on or off fails.
    struct Stuck(Status);

    impl LoginItem for Stuck {
        fn status(&self) -> Result<Status, String> {
            Ok(self.0)
        }

        fn set_enabled(&self, _: bool) -> Result<(), String> {
            Err("denied".into())
        }
    }

    #[test]
    fn a_login_item_that_cannot_change_leaves_everything_for_next_time() {
        let mut p = prefs(true, false);
        assert_eq!(sync(&Stuck(NotRegistered), &mut p), None);
        assert_eq!(p, prefs(true, false), "not marked as added, so tried again");
        let mut p = prefs(false, true);
        assert_eq!(sync(&Stuck(Enabled), &mut p), None);
        assert_eq!(p, prefs(false, true));
    }

    #[test]
    fn a_failure_that_leaves_the_item_as_asked_counts_as_done() {
        let mut p = prefs(false, false);
        set(Some(&Stuck(RequiresApproval)), &mut p, true).unwrap();
        assert_eq!(p, prefs(true, true));
        set(Some(&Stuck(NotFound)), &mut p, false).unwrap();
        assert_eq!(p, prefs(false, true));
        assert_eq!(set(Some(&Stuck(Enabled)), &mut p, false), Err(FAILED));
        assert_eq!(set(Some(&Stuck(NotFound)), &mut p, true), Err(FAILED));
        assert_eq!(p, prefs(false, true));
    }

    #[test]
    fn a_broken_login_item_changes_nothing() {
        let broken = FakeItem {
            broken: true,
            ..FakeItem::default()
        };
        let mut p = prefs(true, false);
        assert_eq!(sync(&broken, &mut p), None);
        assert_eq!(p, prefs(true, false));
        assert_eq!(set(Some(&broken), &mut p, false), Err(FAILED));
        assert_eq!(p, prefs(true, false));
    }

    #[test]
    fn set_follows_the_choice() {
        let (item, mut p) = (item(NotRegistered), prefs(false, false));
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
    fn the_old_launch_agent_file_is_deleted_if_there() {
        let home = tempfile::tempdir().unwrap();
        let path = launch_agent(home.path());
        assert!(path.ends_with("Library/LaunchAgents/xcreenshot.plist"));
        assert_eq!(remove_file(&path), Ok(false));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "<plist/>").unwrap();
        assert_eq!(remove_file(&path), Ok(true));
        assert!(!path.exists());
        // A folder in its place can't be removed as a file.
        std::fs::create_dir(&path).unwrap();
        assert!(remove_file(&path).unwrap_err().contains("xcreenshot.plist"));
    }

    fn app_with_login_item() -> tauri::App<tauri::test::MockRuntime> {
        let builder = mock_builder();
        #[cfg(target_os = "macos")]
        let builder = builder.manage(System::default());
        #[cfg(not(target_os = "macos"))]
        let builder = builder.plugin(tauri_plugin_autostart::Builder::new().build());
        builder.build(mock_context(noop_assets())).unwrap()
    }

    #[test]
    fn debug_builds_have_no_login_item() {
        let app = app_with_login_item();
        assert!(app.try_state::<System>().is_some());
        assert!(super::item(app.handle()).is_none());
        let needs = tauri::async_runtime::block_on(login_item_needs_approval(app.handle().clone()));
        assert!(!needs);
    }
}
