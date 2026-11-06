//! The settings the pages read and change: `get_prefs`, `set_pref` and, for the overlay,
//! `set_capture_option`.
//!
//! [`Prefs`] is the only source of truth. A change is checked, applied (the capture shortcut
//! and the login item change live), then saved; if saving fails the system change is undone,
//! so the system always matches the saved prefs. The new settings go to every page as
//! [`CHANGED`], numbered by [`Settings::version`] so a page never shows an older snapshot
//! after a newer one.

use crate::login::{self, LoginItem};
use crate::prefs::{self, Prefs, Storage};
use crate::print_screen;
use crate::shortcut::{self, Registered, Registrar};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

/// Sent to every page with the new [`Settings`] after a change.
pub const CHANGED: &str = "prefs:changed";
const UNKNOWN: &str = "Unknown setting.";

/// Counts saved changes in this run; see [`Settings::version`].
static VERSION: AtomicU64 = AtomicU64::new(0);

/// What the pages see of [`Prefs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub shortcut: String,
    pub remember_region: bool,
    pub magnifier: bool,
    pub open_at_login: bool,
    pub check_for_updates: bool,
    /// Goes up with every saved change. Pages ignore a snapshot older than the one they show.
    pub version: u64,
}

impl Settings {
    fn new(p: &Prefs, version: u64) -> Self {
        Self {
            shortcut: p.shortcut.clone(),
            remember_region: p.remember_region,
            magnifier: p.magnifier,
            open_at_login: p.open_at_login,
            check_for_updates: p.check_for_updates,
            version,
        }
    }
}

/// A `set_pref` call: the key and its value.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "key", content = "value", rename_all = "camelCase")]
enum Change {
    /// `null` resets it to the default.
    Shortcut(Option<String>),
    RememberRegion(bool),
    Magnifier(bool),
    OpenAtLogin(bool),
    CheckForUpdates(bool),
}

/// The system side of the settings: the capture shortcut and the login item.
struct Os<'a, G, L> {
    shortcuts: &'a G,
    registered: &'a Registered,
    login: Option<&'a L>,
    /// Whether Snipping Tool has PrintScreen (Windows; see [`print_screen`]).
    snipping: print_screen::SnippingTool,
}

impl Change {
    fn parse(key: &str, value: Value) -> Result<Self, &'static str> {
        serde_json::from_value(json!({ "key": key, "value": value })).map_err(|e| {
            log::warn!("rejected setting {key}: {e}");
            UNKNOWN
        })
    }

    /// Applies the change to `prefs` and to the system (shortcut, login item). On error,
    /// neither changed.
    fn apply<G: Registrar, L: LoginItem>(
        self,
        prefs: &mut Prefs,
        os: &Os<G, L>,
    ) -> Result<(), &'static str> {
        match self {
            Change::Shortcut(Some(new)) => {
                shortcut::validate(&new)?;
                print_screen::check_pick(&new, os.snipping)?;
                shortcut::swap(os.shortcuts, os.registered, &new)?;
                prefs.shortcut = new;
            }
            Change::Shortcut(None) => {
                let default = print_screen::use_default(os.shortcuts, os.registered, os.snipping)?;
                prefs.shortcut = default.to_owned();
            }
            Change::RememberRegion(on) => {
                prefs.remember_region = on;
                // Off forgets the regions too (and, once saved, any still being saved).
                if !on {
                    prefs.regions.clear();
                }
            }
            Change::Magnifier(on) => prefs.magnifier = on,
            Change::OpenAtLogin(on) => login::set(os.login, prefs, on)?,
            Change::CheckForUpdates(on) => prefs.check_for_updates = on,
        }
        Ok(())
    }
}

/// Puts the system back the way `before` had it, after `after` couldn't be saved.
fn undo<G: Registrar, L: LoginItem>(before: &Prefs, after: &Prefs, os: &Os<G, L>) {
    if before.shortcut != after.shortcut
        && let Err(e) = shortcut::swap(os.shortcuts, os.registered, &before.shortcut)
    {
        log::error!("could not put back the capture shortcut: {e}");
    }
    if before.open_at_login != after.open_at_login
        && let Err(e) = login::set(os.login, &mut after.clone(), before.open_at_login)
    {
        log::error!("could not put back the login item: {e}");
    }
}

/// Applies `change` and saves it, undoing the system change if saving fails. Returns the
/// saved prefs.
fn transact<G: Registrar, L: LoginItem>(
    storage: &impl Storage,
    change: Change,
    os: &Os<G, L>,
) -> Result<Prefs, &'static str> {
    let before = storage.load();
    let mut after = before.clone();
    change.apply(&mut after, os)?;
    if let Err(e) = storage.save(&after) {
        log::error!("could not save prefs: {e}");
        undo(&before, &after, os);
        return Err(prefs::NOT_SAVED);
    }
    Ok(after)
}

/// Startup: registers the capture shortcut (falling back from a PrintScreen that Snipping Tool
/// has, see [`print_screen`]) and syncs the login item, saving what changed. If that can't be
/// saved, a login item it added is taken away again, so the next launch doesn't mistake its
/// absence for the user removing it. Returns the prefs in effect and why to tell the user about the
/// PrintScreen fallback, if they should be told.
fn start(
    storage: &dyn Storage,
    shortcuts: &impl Registrar,
    registered: &Registered,
    login: Option<&impl LoginItem>,
    snipping: print_screen::SnippingTool,
) -> (Prefs, Option<print_screen::Reason>) {
    let loaded = storage.load();
    let mut prefs = loaded.clone();
    let ok = shortcuts
        .register(&prefs.shortcut)
        // Another app may own it; the menu bar item still works.
        .inspect_err(|e| {
            log::warn!(
                "could not register capture shortcut {}: {e}",
                prefs.shortcut
            )
        })
        .is_ok();
    registered.set(&prefs.shortcut, ok);
    let tell = print_screen::at_startup(shortcuts, registered, &mut prefs, ok, snipping);
    let turned = login.and_then(|item| login::sync(item, &mut prefs));
    if prefs != loaded
        && let Err(e) = storage.save(&prefs)
    {
        log::error!("could not save prefs: {e}");
        // A login item that replaced the old LaunchAgent stays: that one is gone already, and
        // without either the next launch would take it for the user turning it off.
        let replaced = loaded.login_item_added && !loaded.launch_agent_removed;
        if let (Some(item), Some(on), false) = (login, turned, replaced) {
            // `set` logs a failure; there is nothing more to do then.
            let _ = login::set(Some(item), &mut prefs.clone(), !on);
        }
        return (loaded, tell);
    }
    (prefs, tell)
}

/// Applies the saved settings at startup. Returns the capture shortcut, for the menu bar, and
/// why to tell the user about the PrintScreen fallback, if at all (see [`print_screen::tell`]).
pub fn init<R: Runtime>(app: &AppHandle<R>) -> (String, Option<print_screen::Reason>) {
    init_with(app, app.global_shortcut(), print_screen::SNIPPING_TOOL)
}

fn init_with<R: Runtime>(
    app: &AppHandle<R>,
    shortcuts: &impl Registrar,
    snipping: print_screen::SnippingTool,
) -> (String, Option<print_screen::Reason>) {
    let login = login::item(app);
    let registered = app.state::<Registered>();
    let store = prefs::open(app);
    let storage: &dyn Storage = match &store {
        Some(store) => &**store,
        None => &prefs::Unsaved,
    };
    let (_, tell) = start(storage, shortcuts, &registered, login.as_deref(), snipping);
    (registered.get().accelerator, tell)
}

// Async, so reading and writing the file stays off the main thread.
#[tauri::command]
pub async fn get_prefs<R: Runtime>(app: AppHandle<R>) -> Settings {
    // Read before loading: a change saved meanwhile is then newer, never older, than claimed.
    let version = VERSION.load(Ordering::Acquire);
    Settings::new(&prefs::load(&app), version)
}

/// Changes one setting. Returns all the settings, which are also sent as [`CHANGED`].
#[tauri::command]
pub async fn set_pref<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    value: Value,
) -> Result<Settings, String> {
    set_with(&app, app.global_shortcut(), &key, value).map_err(Into::into)
}

/// The overlay's ⋯ menu: only Remember region and Magnifier.
#[tauri::command]
pub async fn set_capture_option<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    on: bool,
) -> Result<Settings, String> {
    capture_option(&key)?;
    set_with(&app, app.global_shortcut(), &key, Value::Bool(on)).map_err(Into::into)
}

fn capture_option(key: &str) -> Result<(), &'static str> {
    match key {
        "rememberRegion" | "magnifier" => Ok(()),
        _ => Err(UNKNOWN),
    }
}

fn set_with<R: Runtime>(
    app: &AppHandle<R>,
    shortcuts: &impl Registrar,
    key: &str,
    value: Value,
) -> Result<Settings, &'static str> {
    let change = Change::parse(key, value)?;
    let login = login::item(app);
    let registered = app.state::<Registered>();
    let os = Os {
        shortcuts,
        registered: &registered,
        login: login.as_deref(),
        snipping: print_screen::SNIPPING_TOOL,
    };
    prefs::with_store(app, |store| {
        let was_on = Prefs::load(store).remember_region;
        let prefs = transact(store, change, &os)?;
        if was_on && !prefs.remember_region {
            crate::region::ORDER.forget();
        }
        let settings = Settings::new(&prefs, VERSION.fetch_add(1, Ordering::AcqRel) + 1);
        // Still holding the lock, so pages get the changes in order.
        if let Err(e) = app.emit(CHANGED, &settings) {
            log::error!("could not send {CHANGED}: {e}");
        }
        Ok(settings)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login::tests::FakeItem;
    use crate::prefs::tests::FakeStorage;
    use crate::shortcut::tests::FakeRegistrar;
    use std::sync::{Arc, Mutex};
    use tauri::Listener;
    use tauri::async_runtime::block_on;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    use crate::print_screen::tests::{OFF, ON};

    fn change(key: &str, value: Value) -> Change {
        Change::parse(key, value).unwrap()
    }

    #[test]
    fn parses_each_key_and_rejects_bad_ones() {
        assert_eq!(
            change("shortcut", json!("Alt+S")),
            Change::Shortcut(Some("Alt+S".into()))
        );
        assert_eq!(change("shortcut", json!(null)), Change::Shortcut(None));
        assert_eq!(
            change("rememberRegion", json!(false)),
            Change::RememberRegion(false)
        );
        assert_eq!(change("magnifier", json!(true)), Change::Magnifier(true));
        assert_eq!(
            change("openAtLogin", json!(false)),
            Change::OpenAtLogin(false)
        );
        assert_eq!(
            change("checkForUpdates", json!(false)),
            Change::CheckForUpdates(false)
        );
        for (key, value) in [
            ("magnifier", json!("yes")),
            ("magnifier", json!(null)),
            ("shortcut", json!(1)),
            ("lastSaveDir", json!("/tmp")),
            ("loginItemAdded", json!(true)),
            ("nope", json!(true)),
        ] {
            assert_eq!(Change::parse(key, value), Err(UNKNOWN), "{key}");
        }
    }

    #[test]
    fn the_overlay_can_change_only_the_capture_options() {
        assert_eq!(capture_option("rememberRegion"), Ok(()));
        assert_eq!(capture_option("magnifier"), Ok(()));
        for key in ["shortcut", "openAtLogin", "checkForUpdates", "nope"] {
            assert_eq!(capture_option(key), Err(UNKNOWN), "{key}");
        }
    }

    struct Fakes {
        shortcuts: FakeRegistrar,
        registered: Registered,
        login: FakeItem,
        snipping: print_screen::SnippingTool,
    }

    impl Fakes {
        fn new() -> Self {
            let fakes = Self {
                shortcuts: FakeRegistrar::holding(shortcut::DEFAULT),
                registered: Registered::default(),
                login: FakeItem::default(),
                snipping: OFF,
            };
            fakes.registered.set(shortcut::DEFAULT, true);
            fakes
        }

        fn os(&self) -> Os<'_, FakeRegistrar, FakeItem> {
            Os {
                shortcuts: &self.shortcuts,
                registered: &self.registered,
                login: Some(&self.login),
                snipping: self.snipping,
            }
        }

        fn apply(&self, prefs: &mut Prefs, key: &str, value: Value) -> Result<(), &'static str> {
            change(key, value).apply(prefs, &self.os())
        }
    }

    fn storage(fail: bool) -> FakeStorage {
        FakeStorage {
            fail,
            ..FakeStorage::default()
        }
    }

    #[test]
    fn toggles_change_their_pref() {
        let fakes = Fakes::new();
        let mut p = Prefs::default();
        fakes.apply(&mut p, "rememberRegion", json!(false)).unwrap();
        fakes.apply(&mut p, "magnifier", json!(true)).unwrap();
        fakes
            .apply(&mut p, "checkForUpdates", json!(false))
            .unwrap();
        fakes.apply(&mut p, "openAtLogin", json!(false)).unwrap();
        let s = Settings::new(&p, 0);
        assert!(!s.remember_region && s.magnifier && !s.check_for_updates && !s.open_at_login);
        fakes.apply(&mut p, "openAtLogin", json!(true)).unwrap();
        assert!(fakes.login.on.get() && p.open_at_login && p.login_item_added);
    }

    #[test]
    fn turning_remember_region_off_forgets_the_regions() {
        let fakes = Fakes::new();
        let region = crate::region::Region {
            arrangement: "a".into(),
            display: crate::region::tests::monitor(1),
            rect: crate::region::tests::rect(1.0, 2.0, 3.0, 4.0),
        };
        let mut p = Prefs {
            regions: vec![region.clone()],
            ..Prefs::default()
        };
        fakes.apply(&mut p, "rememberRegion", json!(true)).unwrap();
        assert_eq!(p.regions, [region], "on keeps them");
        fakes.apply(&mut p, "rememberRegion", json!(false)).unwrap();
        assert!(!p.remember_region && p.regions.is_empty());
    }

    #[test]
    fn a_failed_login_item_keeps_the_pref() {
        let mut fakes = Fakes::new();
        fakes.login.broken = true;
        let mut p = Prefs::default();
        assert_eq!(
            fakes.apply(&mut p, "openAtLogin", json!(false)),
            Err(login::FAILED)
        );
        assert!(p.open_at_login);
    }

    #[test]
    fn a_new_shortcut_replaces_the_old_one_live() {
        let fakes = Fakes::new();
        let mut p = Prefs::default();
        fakes
            .apply(&mut p, "shortcut", json!("Alt+Shift+F5"))
            .unwrap();
        assert_eq!(p.shortcut, "Alt+Shift+F5");
        assert_eq!(fakes.shortcuts.held(), ["Alt+Shift+F5"]);
        assert_eq!(fakes.registered.active().as_deref(), Some("Alt+Shift+F5"));
        // Reset to default.
        fakes.apply(&mut p, "shortcut", json!(null)).unwrap();
        assert_eq!(p.shortcut, print_screen::default_shortcut(false, true));
        assert_eq!(fakes.shortcuts.held(), [p.shortcut.as_str()]);
    }

    #[test]
    fn with_snipping_tool_on_reset_picks_the_default_that_works() {
        let mut fakes = Fakes::new();
        fakes.snipping = ON;
        let mut p = Prefs::default();
        fakes
            .apply(&mut p, "shortcut", json!("Alt+Shift+F5"))
            .unwrap();
        fakes.apply(&mut p, "shortcut", json!(null)).unwrap();
        // Windows: the fallback; macOS: its usual default.
        assert_eq!(p.shortcut, print_screen::default_shortcut(true, true));
        assert_eq!(fakes.shortcuts.held(), [p.shortcut.as_str()]);
    }

    #[test]
    fn with_snipping_tool_on_the_recorder_refuses_print_screen() {
        let mut fakes = Fakes::new();
        fakes.snipping = ON;
        let mut p = Prefs {
            shortcut: "Alt+S".into(),
            ..Prefs::default()
        };
        fakes.registered.set("Alt+S", true);
        let fakes = Fakes {
            shortcuts: FakeRegistrar::holding("Alt+S"),
            ..fakes
        };
        let picked = fakes.apply(&mut p, "shortcut", json!(shortcut::DEFAULT));
        if print_screen::is_print_screen(shortcut::DEFAULT) {
            assert_eq!(picked, Err(print_screen::TAKEN_BY_SNIPPING_TOOL));
            assert_eq!(p.shortcut, "Alt+S", "the old one stays");
            assert_eq!(fakes.shortcuts.held(), ["Alt+S"]);
        } else {
            assert_eq!(picked, Ok(()));
        }
        // PrintScreen itself, wherever validation lets it through.
        assert_eq!(
            print_screen::check_pick("PrintScreen", fakes.snipping),
            Err(print_screen::TAKEN_BY_SNIPPING_TOOL)
        );
    }

    #[test]
    fn startup_saves_the_print_screen_fallback_and_tells_once() {
        let (fakes, storage) = (Fakes::new(), storage(false));
        storage.prefs.borrow_mut().shortcut = "PrintScreen".into();
        let shortcuts = FakeRegistrar::default();
        let on = ON;
        let (p, tell) = start(
            &storage,
            &shortcuts,
            &fakes.registered,
            None::<&FakeItem>,
            on,
        );
        assert_eq!(tell, Some(print_screen::Reason::SnippingTool));
        assert_eq!(shortcuts.held(), [print_screen::FALLBACK]);
        assert_eq!(p.shortcut, print_screen::FALLBACK);
        assert!(p.print_screen_notice_shown);
        assert_eq!(storage.load(), p, "saved");
        // The next launch has nothing to do.
        let (_, tell) = start(
            &storage,
            &FakeRegistrar::default(),
            &fakes.registered,
            None::<&FakeItem>,
            on,
        );
        assert_eq!(tell, None);
        assert_eq!(storage.saves.get(), 1);
    }

    #[test]
    fn startup_keeps_the_fallback_when_it_cannot_be_saved() {
        let (fakes, storage) = (Fakes::new(), FakeStorage::failing());
        storage.prefs.borrow_mut().shortcut = "PrintScreen".into();
        let shortcuts = FakeRegistrar::default();
        let (_, tell) = start(
            &storage,
            &shortcuts,
            &fakes.registered,
            None::<&FakeItem>,
            ON,
        );
        // Told now, and again next launch (the notice wasn't saved either).
        assert_eq!(tell, Some(print_screen::Reason::SnippingTool));
        assert_eq!(storage.saves.get(), 1);
        assert_eq!(
            fakes.registered.active().as_deref(),
            Some(print_screen::FALLBACK)
        );
    }

    #[test]
    fn a_taken_or_invalid_shortcut_keeps_the_old_one() {
        let mut fakes = Fakes::new();
        fakes.shortcuts.taken.insert("Alt+Shift+F5".into());
        let mut p = Prefs::default();
        assert_eq!(
            fakes.apply(&mut p, "shortcut", json!("Alt+Shift+F5")),
            Err(shortcut::IN_USE)
        );
        assert!(
            fakes
                .apply(&mut p, "shortcut", json!("Control+Nope"))
                .is_err()
        );
        assert!(fakes.apply(&mut p, "shortcut", json!("Shift+S")).is_err());
        assert_eq!(p, Prefs::default());
        assert_eq!(fakes.shortcuts.held(), [shortcut::DEFAULT]);
        assert_eq!(
            fakes.registered.active().as_deref(),
            Some(shortcut::DEFAULT)
        );
    }

    #[test]
    fn a_change_is_saved_with_its_system_side() {
        let (fakes, storage) = (Fakes::new(), storage(false));
        let saved = transact(&storage, change("shortcut", json!("Alt+D")), &fakes.os()).unwrap();
        assert_eq!(saved.shortcut, "Alt+D");
        assert_eq!(storage.load().shortcut, "Alt+D");
        assert_eq!(fakes.shortcuts.held(), ["Alt+D"]);
    }

    #[test]
    fn a_failed_system_change_saves_nothing() {
        let (mut fakes, storage) = (Fakes::new(), storage(false));
        fakes.login.broken = true;
        let change = change("openAtLogin", json!(false));
        assert_eq!(transact(&storage, change, &fakes.os()), Err(login::FAILED));
        assert_eq!(storage.load(), Prefs::default());
        assert_eq!(storage.saves.get(), 0, "a failed change writes nothing");
    }

    #[test]
    fn a_failed_save_puts_the_shortcut_back() {
        let (fakes, storage) = (Fakes::new(), storage(true));
        let change = change("shortcut", json!("Alt+D"));
        assert_eq!(
            transact(&storage, change, &fakes.os()),
            Err(prefs::NOT_SAVED)
        );
        assert_eq!(fakes.shortcuts.held(), [shortcut::DEFAULT]);
        assert_eq!(
            fakes.registered.active().as_deref(),
            Some(shortcut::DEFAULT)
        );
    }

    #[test]
    fn a_failed_save_puts_the_login_item_back() {
        let (fakes, storage) = (Fakes::new(), storage(true));
        fakes.login.on.set(true);
        let change = change("openAtLogin", json!(false));
        assert_eq!(
            transact(&storage, change, &fakes.os()),
            Err(prefs::NOT_SAVED)
        );
        assert!(fakes.login.on.get());
    }

    #[test]
    fn undo_logs_what_it_cannot_put_back() {
        let mut fakes = Fakes::new();
        fakes.login.broken = true;
        // The new shortcut isn't held, so letting it go fails; the login item is broken.
        let after = Prefs {
            shortcut: "Alt+D".into(),
            open_at_login: false,
            ..Prefs::default()
        };
        fakes.registered.set("Alt+D", true);
        undo(&Prefs::default(), &after, &fakes.os());
        assert_eq!(fakes.registered.active().as_deref(), Some("Alt+D"));
    }

    #[test]
    fn startup_registers_the_shortcut_and_adds_the_login_item() {
        let (fakes, storage) = (Fakes::new(), storage(false));
        let shortcuts = FakeRegistrar::default();
        let (p, _) = start(
            &storage,
            &shortcuts,
            &fakes.registered,
            Some(&fakes.login),
            OFF,
        );
        assert_eq!(shortcuts.held(), [shortcut::DEFAULT]);
        assert!(fakes.registered.get().registered);
        assert!(fakes.login.on.get() && p.login_item_added);
        assert_eq!(storage.load(), p, "saved");
        // Nothing changed the second time: nothing is written.
        start(
            &storage,
            &FakeRegistrar::default(),
            &fakes.registered,
            Some(&fakes.login),
            OFF,
        );
        assert_eq!(storage.saves.get(), 1);
        // Without a login item (debug builds) nothing changes.
        let other = super::tests::storage(false);
        start(
            &other,
            &FakeRegistrar::default(),
            &fakes.registered,
            None::<&FakeItem>,
            OFF,
        );
        assert_eq!(other.saves.get(), 0);
    }

    #[test]
    fn startup_takes_back_a_login_item_it_cannot_record() {
        let (fakes, storage) = (Fakes::new(), FakeStorage::failing());
        let (p, _) = start(
            &storage,
            &FakeRegistrar::default(),
            &fakes.registered,
            Some(&fakes.login),
            OFF,
        );
        assert!(!fakes.login.on.get(), "added, then taken away again");
        assert_eq!(p, Prefs::default());
        // Without any store it is the same.
        let fakes = Fakes::new();
        start(
            &prefs::Unsaved,
            &FakeRegistrar::default(),
            &fakes.registered,
            Some(&fakes.login),
            OFF,
        );
        assert!(!fakes.login.on.get());
    }

    #[test]
    fn startup_keeps_a_login_item_that_replaced_the_old_launch_agent() {
        let (fakes, storage) = (Fakes::new(), FakeStorage::failing());
        fakes.login.launch_agent.set(Ok(true));
        *storage.prefs.borrow_mut() = Prefs {
            login_item_added: true,
            ..Prefs::default()
        };
        start(
            &storage,
            &FakeRegistrar::default(),
            &fakes.registered,
            Some(&fakes.login),
            OFF,
        );
        assert_eq!(fakes.login.launch_agent.get(), Ok(false), "removed");
        assert!(fakes.login.on.get(), "kept, so a login item is still there");
    }

    #[test]
    fn startup_with_a_taken_shortcut_marks_it_unregistered() {
        let fakes = Fakes::new();
        let mut shortcuts = FakeRegistrar::default();
        // Both taken, so even Windows keeps PrintScreen (see `print_screen`).
        shortcuts
            .taken
            .extend([shortcut::DEFAULT.into(), print_screen::FALLBACK.into()]);
        start(
            &storage(false),
            &shortcuts,
            &fakes.registered,
            None::<&FakeItem>,
            OFF,
        );
        assert_eq!(
            fakes.registered.get(),
            shortcut::CaptureShortcut {
                accelerator: shortcut::DEFAULT.into(),
                registered: false
            }
        );
    }

    /// An app with its own data folder (not the real app's) and the store.
    fn app() -> tauri::App<MockRuntime> {
        let mut context = mock_context(noop_assets());
        context.config_mut().identifier =
            format!("com.bervex.xcreenshot.test.{}", std::process::id());
        mock_builder()
            .plugin(tauri_plugin_store::Builder::new().build())
            .manage(Registered::default())
            .build(context)
            .unwrap()
    }

    /// Uses a fake registrar: the real global shortcut allows one manager per process, and the
    /// overlay tests own it.
    #[test]
    fn changes_are_saved_and_broadcast_in_order() {
        let app = app();
        let h = app.handle().clone();
        let data = h.path().app_data_dir().unwrap();
        let _ = std::fs::remove_dir_all(&data);
        let heard = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = heard.clone();
        h.listen(CHANGED, move |e| {
            sink.lock().unwrap().push(e.payload().to_owned())
        });
        let shortcuts = FakeRegistrar::default();

        assert_eq!(
            init_with(&h, &shortcuts, OFF),
            (shortcut::DEFAULT.to_owned(), None)
        );
        assert_eq!(shortcuts.held(), [shortcut::DEFAULT]);
        let first = block_on(get_prefs(h.clone()));
        assert_eq!(first, Settings::new(&Prefs::default(), first.version));

        let saved = set_with(&h, &shortcuts, "magnifier", json!(true)).unwrap();
        assert!(saved.magnifier);
        assert!(saved.version > first.version);
        let loaded = block_on(get_prefs(h.clone()));
        assert!(loaded.magnifier, "saved");
        assert_eq!(loaded.version, saved.version);
        assert_eq!(
            heard.lock().unwrap().as_slice(),
            [serde_json::to_string(&saved).unwrap()]
        );

        let next = set_with(&h, &shortcuts, "shortcut", json!("Alt+Shift+F5")).unwrap();
        assert_eq!(next.shortcut, "Alt+Shift+F5");
        assert!(next.version > saved.version);
        assert_eq!(shortcuts.held(), ["Alt+Shift+F5"]);
        assert_eq!(block_on(get_prefs(h.clone())).shortcut, "Alt+Shift+F5");
        // The next launch registers the saved shortcut.
        let relaunch = FakeRegistrar::default();
        assert_eq!(
            init_with(&h, &relaunch, OFF),
            ("Alt+Shift+F5".to_owned(), None)
        );

        assert_eq!(set_with(&h, &shortcuts, "nope", json!(true)), Err(UNKNOWN));
        assert!(set_with(&h, &shortcuts, "shortcut", json!("S")).is_err());
        assert_eq!(prefs::load(&h).shortcut, "Alt+Shift+F5", "not saved");
        assert_eq!(
            heard.lock().unwrap().len(),
            2,
            "no event for a rejected change"
        );
        let _ = std::fs::remove_dir_all(data);
    }
}
