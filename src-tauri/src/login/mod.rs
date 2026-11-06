//! Open at login: the `open_at_login` pref drives the system login item. On macOS that is
//! `SMAppService.mainApp` (see [`macos`]), so System Settings › General › Login Items lists
//! xcreenshot by name; on Windows it is a `Run` registry value, through tauri-plugin-autostart.
//!
//! Debug builds never touch the login item, so `pnpm tauri dev` doesn't add the dev binary.
//!
//! Versions before SMAppService added a LaunchAgent (`~/Library/LaunchAgents/xcreenshot.plist`).
//! [`sync`] replaces it once: it registers with SMAppService first (if the pref is on) and
//! deletes the LaunchAgent only after that worked, so login keeps working if it fails.

#[cfg(target_os = "macos")]
mod macos;

use crate::prefs::Prefs;
use tauri::{AppHandle, Manager, Runtime};

pub const FAILED: &str = "Couldn't change the login item.";

/// The login item in effect on this platform.
#[cfg(target_os = "macos")]
pub use macos::MainApp as System;
#[cfg(not(target_os = "macos"))]
pub use tauri_plugin_autostart::AutoLaunchManager as System;

/// The state of the login item, as `SMAppService.Status` reports it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Enabled,
    NotRegistered,
    /// Registered, but the user has to allow it in System Settings › Login Items.
    RequiresApproval,
    NotFound,
}

impl Status {
    /// Registered, whether or not the user has allowed it yet.
    fn is_on(self) -> bool {
        matches!(self, Self::Enabled | Self::RequiresApproval)
    }
}

/// The system login item (SMAppService, the autostart plugin, or a fake in tests).
pub trait LoginItem {
    fn status(&self) -> Result<Status, String>;
    fn set_enabled(&self, on: bool) -> Result<(), String>;
    /// Whether the LaunchAgent older versions added is still there; an error if that can't be
    /// told (then the migration waits for the next launch).
    fn has_launch_agent(&self) -> Result<bool, String> {
        Ok(false)
    }
    /// Deletes that LaunchAgent.
    fn remove_launch_agent(&self) -> Result<(), String> {
        Ok(())
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

/// Turns the login item on or off for the user's choice and remembers that it was added. A
/// failure where the item already is as asked (unregistering one that isn't registered, or
/// registering one that waits for approval) still counts as done.
pub fn set(item: Option<&impl LoginItem>, prefs: &mut Prefs, on: bool) -> Result<(), &'static str> {
    if let Some(item) = item {
        if let Err(e) = item.set_enabled(on) {
            let state = if on { "on" } else { "off" };
            if item.status().map(Status::is_on) != Ok(on) {
                log::error!("could not turn {state} the login item: {e}");
                return Err(FAILED);
            }
            log::info!("the login item is already {state}: {e}");
        }
        prefs.login_item_added |= on;
    }
    prefs.open_at_login = on;
    Ok(())
}

/// Replaces the old LaunchAgent with SMAppService's login item: with the pref on, registers
/// first, and deletes the LaunchAgent only once registered (or waiting for approval). If
/// registering or deleting fails, the migration isn't marked done and the next launch tries
/// again; the LaunchAgent keeps login working meanwhile. Returns what it did, as [`sync`].
fn replace_launch_agent(item: &impl LoginItem, prefs: &mut Prefs) -> Option<bool> {
    let mut turned = None;
    if prefs.open_at_login {
        match item.status() {
            Ok(status) if status.is_on() => prefs.login_item_added = true,
            Ok(_) => {
                set(Some(item), prefs, true).ok()?;
                turned = Some(true);
            }
            Err(e) => {
                log::warn!("could not check the login item: {e}");
                return None;
            }
        }
    }
    match item.remove_launch_agent() {
        Ok(()) => {
            log::info!("replaced the old LaunchAgent");
            prefs.launch_agent_removed = true;
        }
        Err(e) => log::warn!("could not remove the old LaunchAgent: {e}"),
    }
    turned
}

/// Brings the login item and the pref in line at startup, first replacing the old LaunchAgent
/// if it is there (see [`replace_launch_agent`]). The first run adds the login item (the pref defaults to on). If it was
/// added before and the user turned it off in System Settings, the pref turns off. Returns
/// what it did to the login item: turned it on or off, or nothing. A failure is logged and
/// leaves the login item and pref as they were, so the next launch tries again.
pub fn sync(item: &impl LoginItem, prefs: &mut Prefs) -> Option<bool> {
    if !prefs.launch_agent_removed {
        match item.has_launch_agent() {
            Ok(true) => return replace_launch_agent(item, prefs),
            Ok(false) => prefs.launch_agent_removed = true,
            Err(e) => {
                log::warn!("could not look for the old LaunchAgent: {e}");
                return None;
            }
        }
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
    // Only SMAppService asks for approval: on Windows this would just read the registry.
    cfg!(target_os = "macos")
        && item(&app).is_some_and(|item| item.status() == Ok(Status::RequiresApproval))
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
    /// it not found when off, `broken` makes every call fail and `stuck` makes turning it on or
    /// off fail. `launch_agent` is the old LaunchAgent, `Err` if it can't be found out;
    /// `locked` makes deleting it fail.
    pub struct FakeItem {
        pub on: Cell<bool>,
        pub waiting: bool,
        pub missing: bool,
        pub broken: bool,
        pub stuck: bool,
        pub launch_agent: Cell<Result<bool, ()>>,
        pub locked: bool,
    }

    impl Default for FakeItem {
        fn default() -> Self {
            Self {
                on: Cell::default(),
                waiting: false,
                missing: false,
                broken: false,
                stuck: false,
                launch_agent: Cell::new(Ok(false)),
                locked: false,
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
            if self.broken || self.stuck {
                return Err("denied".into());
            }
            self.on.set(on);
            Ok(())
        }

        fn has_launch_agent(&self) -> Result<bool, String> {
            self.launch_agent
                .get()
                .map_err(|()| "no home folder".to_owned())
        }

        fn remove_launch_agent(&self) -> Result<(), String> {
            if self.locked {
                return Err("locked".into());
            }
            self.launch_agent.set(Ok(false));
            Ok(())
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

    /// Prefs from a version with the LaunchAgent: it was added, the migration isn't done.
    fn old_prefs(open_at_login: bool) -> Prefs {
        Prefs {
            open_at_login,
            login_item_added: true,
            ..Prefs::default()
        }
    }

    fn with_launch_agent() -> FakeItem {
        FakeItem {
            launch_agent: Cell::new(Ok(true)),
            ..FakeItem::default()
        }
    }

    #[test]
    fn the_old_launch_agent_is_replaced_once() {
        let (item, mut p) = (with_launch_agent(), old_prefs(true));
        assert_eq!(sync(&item, &mut p), Some(true));
        assert!(item.on.get(), "registered");
        assert_eq!(item.launch_agent.get(), Ok(false), "then deleted");
        assert_eq!(p, prefs(true, true));
        // Only once: a LaunchAgent that shows up later is left alone.
        item.launch_agent.set(Ok(true));
        assert_eq!(sync(&item, &mut p), None);
        assert_eq!(item.launch_agent.get(), Ok(true));
    }

    #[test]
    fn an_old_launch_agent_is_deleted_when_already_registered_or_waiting() {
        for status in [Enabled, RequiresApproval] {
            let item = FakeItem {
                launch_agent: Cell::new(Ok(true)),
                ..item(status)
            };
            let mut p = old_prefs(true);
            assert_eq!(sync(&item, &mut p), None);
            assert_eq!(item.launch_agent.get(), Ok(false));
            assert_eq!(p, prefs(true, true));
        }
    }

    #[test]
    fn with_the_pref_off_the_old_launch_agent_is_only_deleted() {
        let (item, mut p) = (with_launch_agent(), old_prefs(false));
        assert_eq!(sync(&item, &mut p), None);
        assert!(!item.on.get());
        assert_eq!(item.launch_agent.get(), Ok(false));
        assert_eq!(p, prefs(false, true));
    }

    #[test]
    fn a_failed_registration_keeps_the_old_launch_agent() {
        let item = FakeItem {
            stuck: true,
            ..with_launch_agent()
        };
        let mut p = old_prefs(true);
        assert_eq!(sync(&item, &mut p), None);
        assert_eq!(item.launch_agent.get(), Ok(true), "login still works");
        assert_eq!(
            p,
            old_prefs(true),
            "not done, and not taken for the user turning it off"
        );
        // Next launch, registering works.
        let item = with_launch_agent();
        assert_eq!(sync(&item, &mut p), Some(true));
        assert_eq!(p, prefs(true, true));
        // A broken check also leaves it.
        let broken = FakeItem {
            broken: true,
            ..with_launch_agent()
        };
        let mut p = old_prefs(true);
        assert_eq!(sync(&broken, &mut p), None);
        assert_eq!(p, old_prefs(true));
    }

    #[test]
    fn an_old_launch_agent_that_cannot_be_deleted_is_tried_again() {
        let item = FakeItem {
            locked: true,
            ..with_launch_agent()
        };
        let mut p = old_prefs(true);
        assert_eq!(sync(&item, &mut p), Some(true));
        assert!(item.on.get());
        assert!(!p.launch_agent_removed);
        // Next launch: registered already, so only the deletion is left.
        let item = FakeItem {
            on: Cell::new(true),
            ..with_launch_agent()
        };
        assert_eq!(sync(&item, &mut p), None);
        assert_eq!(item.launch_agent.get(), Ok(false));
        assert_eq!(p, prefs(true, true));
    }

    #[test]
    fn without_an_old_launch_agent_the_migration_changes_nothing_else() {
        // The user had removed it: the pref turns off, as before.
        let (item, mut p) = (FakeItem::default(), old_prefs(true));
        assert_eq!(sync(&item, &mut p), None);
        assert_eq!(p, prefs(false, true));
    }

    #[test]
    fn without_a_home_folder_the_migration_waits() {
        let item = FakeItem {
            launch_agent: Cell::new(Err(())),
            ..FakeItem::default()
        };
        let mut p = old_prefs(true);
        assert_eq!(sync(&item, &mut p), None);
        assert!(!item.on.get());
        assert_eq!(p, old_prefs(true), "not marked done, and the pref stays on");
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
