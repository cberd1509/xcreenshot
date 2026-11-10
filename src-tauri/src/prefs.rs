//! Preferences kept across launches, in `prefs.json` in the app data folder.
//!
//! [`Prefs`] is a typed struct with defaults, stored as one value under one key of a
//! `tauri-plugin-store` store. A missing or invalid field falls back to its default on its own,
//! and a file that is not valid JSON is moved aside, so prefs never get stuck. Rust is the only
//! reader and writer: the pages get no store permission; they read and change settings through
//! `settings`. A file from before a field existed loads with that field's default.

use crate::region::Region;
use crate::shortcut;
use crate::tip::Tip;
use chrono::NaiveDateTime;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::{Store, StoreExt};

const FILE: &str = "prefs.json";
const KEY: &str = "prefs";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    /// The folder of the last screenshot saved; the Save dialog opens there.
    #[serde(default, deserialize_with = "lenient")]
    pub last_save_dir: Option<String>,
    /// The capture shortcut, an accelerator like `Control+Shift+Super+S`.
    #[serde(default = "default_shortcut", deserialize_with = "lenient_shortcut")]
    pub shortcut: String,
    /// Start the selection from the last region.
    #[serde(default = "on", deserialize_with = "lenient_on")]
    pub remember_region: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub magnifier: bool,
    /// Copy as soon as the drag ends, with no toolbar (see `overlay`).
    #[serde(default, deserialize_with = "lenient")]
    pub quick_copy: bool,
    #[serde(default = "on", deserialize_with = "lenient_on")]
    pub open_at_login: bool,
    /// Automatic update checks. On by default; the updater reads it before each check.
    #[serde(default = "on", deserialize_with = "lenient_on")]
    pub check_for_updates: bool,
    /// The app has added its login item before. If it is gone after that, the user removed it
    /// in the system settings (see `login`).
    #[serde(default, deserialize_with = "lenient")]
    pub login_item_added: bool,
    /// The LaunchAgent older versions used as the login item is gone (macOS; see `login`).
    #[serde(default, deserialize_with = "lenient")]
    pub launch_agent_removed: bool,
    /// The user was told PrintScreen went to Snipping Tool (Windows; see `print_screen`).
    #[serde(default, deserialize_with = "lenient")]
    pub print_screen_notice_shown: bool,
    /// The first-run window was finished (see `onboarding`). `None` (missing) in prefs saved
    /// before this was recorded, an install from before onboarding; a fresh install starts at
    /// `false`, and so does an invalid value.
    #[serde(
        default,
        deserialize_with = "lenient_onboarded",
        skip_serializing_if = "Option::is_none"
    )]
    pub onboarded: Option<bool>,
    /// When to suggest quick copy (see `tip`).
    #[serde(default, deserialize_with = "lenient")]
    pub quick_copy_tip: Tip,
    /// The last region per display arrangement, newest first (see `region`).
    #[serde(default, deserialize_with = "lenient")]
    pub regions: Vec<Region>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            last_save_dir: None,
            shortcut: default_shortcut(),
            remember_region: on(),
            magnifier: false,
            quick_copy: false,
            open_at_login: on(),
            check_for_updates: on(),
            login_item_added: false,
            launch_agent_removed: false,
            print_screen_notice_shown: false,
            onboarded: Some(false),
            quick_copy_tip: Tip::default(),
            regions: Vec::new(),
        }
    }
}

fn default_shortcut() -> String {
    shortcut::DEFAULT.to_owned()
}

fn on() -> bool {
    true
}

/// A field that does not parse keeps its default instead of failing the whole struct.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    lenient_or(d, T::default)
}

/// A switch that is on unless it is stored as `false` (missing, null or invalid is on).
fn lenient_on<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    lenient_or(d, on)
}

/// A stored shortcut that is no longer valid falls back to the default too.
fn lenient_shortcut<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s: String = lenient_or(d, default_shortcut)?;
    Ok(match shortcut::validate(&s) {
        Ok(()) => s,
        Err(e) => {
            log::warn!("ignoring the stored shortcut {s:?}: {e}");
            default_shortcut()
        }
    })
}

/// A stored `onboarded` that isn't a bool counts as not done (a missing one is `None`).
fn lenient_onboarded<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    lenient_or(d, || false).map(Some)
}

fn lenient_or<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    d: D,
    default: impl FnOnce() -> T,
) -> Result<T, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_else(|e| {
        log::warn!("ignoring an invalid preference: {e}");
        default()
    }))
}

impl Prefs {
    /// The stored prefs, or the defaults if there are none or they are not an object.
    pub fn load<R: Runtime>(store: &Store<R>) -> Self {
        store
            .get(KEY)
            .and_then(Self::from_stored)
            .unwrap_or_default()
    }

    /// The prefs in a stored value, or `None` if it is not an object of them. Prefs from before
    /// the quick copy tip, with quick copy on, have the tip over: the user knows the mode.
    fn from_stored(value: Value) -> Option<Self> {
        let legacy_quick_copy = value.get("quickCopyTip").is_none();
        let mut prefs: Self = serde_json::from_value(value)
            .inspect_err(|e| log::warn!("ignoring invalid prefs: {e}"))
            .ok()?;
        if legacy_quick_copy && prefs.quick_copy {
            prefs.quick_copy_tip.end();
        }
        Some(prefs)
    }

    /// Stores the prefs in memory only, where every reader sees them at once. The file follows
    /// with the next save (see [`flush`]).
    pub fn stage<R: Runtime>(&self, store: &Store<R>) -> Result<(), String> {
        let value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        store.set(KEY, value);
        Ok(())
    }

    /// Stores the prefs and writes them to disk. If writing fails, the store keeps the previous
    /// prefs, so memory never differs from the file.
    pub fn save<R: Runtime>(&self, store: &Store<R>) -> Result<(), String> {
        let value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        let previous = store.get(KEY);
        store.set(KEY, value);
        store.save().map_err(|e| {
            match previous {
                Some(previous) => store.set(KEY, previous),
                None => {
                    store.delete(KEY);
                }
            }
            e.to_string()
        })
    }

    /// The last save folder as a path.
    pub fn last_save_dir(&self) -> Option<PathBuf> {
        self.last_save_dir.as_ref().map(PathBuf::from)
    }

    /// Remembers `dir` as the last save folder. Paths are stored as strings, so a folder whose
    /// name is not valid UTF-8 is not remembered (logged); returns whether it was.
    pub fn set_last_save_dir(&mut self, dir: &Path) -> bool {
        let Some(dir) = dir.to_str() else {
            log::info!(
                "not remembering a save folder that is not UTF-8: {}",
                dir.display()
            );
            return false;
        };
        self.last_save_dir = Some(dir.to_owned());
        true
    }
}

pub const NOT_SAVED: &str = "Couldn't save the settings.";

/// One change at a time: each loads, changes and saves the whole [`Prefs`].
static CHANGING: Mutex<()> = Mutex::new(());

/// Where the prefs are kept: the store, or a fake in tests.
pub trait Storage {
    fn load(&self) -> Prefs;
    fn save(&self, prefs: &Prefs) -> Result<(), String>;
    /// Keeps `prefs` in memory without writing the file (see [`Prefs::stage`]).
    fn stage(&self, prefs: &Prefs) -> Result<(), String>;
}

impl<R: Runtime> Storage for Store<R> {
    fn load(&self) -> Prefs {
        Prefs::load(self)
    }

    fn save(&self, prefs: &Prefs) -> Result<(), String> {
        prefs.save(self)
    }

    fn stage(&self, prefs: &Prefs) -> Result<(), String> {
        prefs.stage(self)
    }
}

/// No store (it couldn't be opened): the defaults, and nothing can be saved.
pub struct Unsaved;

impl Storage for Unsaved {
    fn load(&self) -> Prefs {
        Prefs::default()
    }

    fn save(&self, _: &Prefs) -> Result<(), String> {
        Err(format!("{FILE} could not be opened"))
    }

    fn stage(&self, prefs: &Prefs) -> Result<(), String> {
        self.save(prefs)
    }
}

/// The saved prefs, or the defaults if the store can't be opened. Waits for a change being
/// saved, so the file is never read half-written.
pub fn load<R: Runtime>(app: &AppHandle<R>) -> Prefs {
    let _changing = CHANGING.lock().unwrap_or_else(PoisonError::into_inner);
    read(app)
}

/// The saved prefs without waiting for a change being saved, for the capture hotkey. Once the
/// file was checked (at startup) the store is read from memory; before that, this is [`load`].
pub fn peek<R: Runtime>(app: &AppHandle<R>) -> Prefs {
    if !CHECKED.load(Ordering::Acquire) {
        return load(app);
    }
    read(app)
}

fn read<R: Runtime>(app: &AppHandle<R>) -> Prefs {
    open(app)
        .map(|store| Prefs::load(&store))
        .unwrap_or_default()
}

/// Runs `f` with the store, one change at a time.
pub fn with_store<R: Runtime, T>(
    app: &AppHandle<R>,
    f: impl FnOnce(&Store<R>) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    let _changing = CHANGING.lock().unwrap_or_else(PoisonError::into_inner);
    let store = open(app).ok_or(NOT_SAVED)?;
    f(&store)
}

/// Loads the prefs, changes them with `change` and saves them, one change at a time. Nothing is
/// saved if `change` fails. Returns the saved prefs.
pub fn update<R: Runtime>(
    app: &AppHandle<R>,
    change: impl FnOnce(&mut Prefs) -> Result<(), &'static str>,
) -> Result<Prefs, &'static str> {
    with_store(app, |store| modify(store, change))
}

/// Loads, changes and saves the prefs in `storage`.
pub fn modify(
    storage: &impl Storage,
    change: impl FnOnce(&mut Prefs) -> Result<(), &'static str>,
) -> Result<Prefs, &'static str> {
    let mut prefs = storage.load();
    change(&mut prefs)?;
    storage.save(&prefs).map_err(|e| {
        log::error!("could not save prefs: {e}");
        NOT_SAVED
    })?;
    Ok(prefs)
}

/// Writes the prefs in memory to the file, after [`Prefs::stage`]. A failure is logged; the next
/// save writes them again.
pub fn flush<R: Runtime>(app: &AppHandle<R>) {
    let written = with_store(app, |store| {
        store.save().map_err(|e| {
            log::error!("could not save prefs: {e}");
            NOT_SAVED
        })
    });
    if let Err(e) = written {
        log::warn!("{e}");
    }
}

/// [`modify`] with a `change` that says whether it changed anything: nothing is saved if not.
/// Returns whether it saved.
pub fn modify_if(
    storage: &impl Storage,
    change: impl FnOnce(&mut Prefs) -> bool,
) -> Result<bool, &'static str> {
    const UNCHANGED: &str = "unchanged";
    match modify(storage, |p| change(p).then_some(()).ok_or(UNCHANGED)) {
        Ok(_) => Ok(true),
        Err(UNCHANGED) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Whether the file was checked this run (see [`check`]); after that the open store is reused.
static CHECKED: AtomicBool = AtomicBool::new(false);

/// The app's prefs store. `None` (logged) if it cannot be opened; callers use the defaults.
/// The file is checked for corruption only the first time it opens.
pub fn open<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<Store<R>>> {
    let dir = app
        .path()
        .app_data_dir()
        .inspect_err(|e| log::error!("no app data folder for {FILE}: {e}"))
        .ok()?;
    let path = dir.join(FILE);
    if CHECKED.load(Ordering::Acquire) {
        return app
            .store(&path)
            .inspect_err(|e| log::error!("could not open {}: {e}", path.display()))
            .ok();
    }
    let store = open_at(app, &path, chrono::Local::now().naive_local())?;
    CHECKED.store(true, Ordering::Release);
    Some(store)
}

/// Opens the store at `path`, first moving a corrupt file aside (see [`set_aside`]).
fn open_at<R: Runtime>(
    app: &AppHandle<R>,
    path: &Path,
    now: NaiveDateTime,
) -> Option<Arc<Store<R>>> {
    match check(path, now) {
        Check::Usable => {}
        Check::MovedAside(aside) => log::warn!(
            "{} was corrupt; moved it to {}",
            path.display(),
            aside.display()
        ),
        // Maybe a passing error: leave the file alone and use the defaults this time.
        Check::Unreadable => return None,
    }
    app.store(path)
        .inspect_err(|e| log::error!("could not open {}: {e}", path.display()))
        .ok()
}

/// What [`check`] found.
#[derive(Debug, PartialEq, Eq)]
enum Check {
    /// Missing, valid, or corrupt but it could not be moved (the store then starts empty and
    /// the next save replaces it).
    Usable,
    /// It was not a JSON object; it was renamed to this path.
    MovedAside(PathBuf),
    /// It could not be read.
    Unreadable,
}

/// Moves the file at `path` aside if it was read and is not a JSON object, so the app starts
/// fresh instead of being stuck. A file that cannot be read is left alone.
fn check(path: &Path, now: NaiveDateTime) -> Check {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Check::Usable,
        Err(e) => {
            log::error!("could not read {}: {e}", path.display());
            return Check::Unreadable;
        }
    };
    if serde_json::from_slice::<Map<String, Value>>(&bytes).is_ok() {
        return Check::Usable;
    }
    let aside = aside_name(path, now, Path::exists);
    match fs::rename(path, &aside) {
        Ok(()) => Check::MovedAside(aside),
        Err(e) => {
            log::error!("could not move {} aside: {e}", path.display());
            Check::Usable
        }
    }
}

/// `prefs.corrupt-<time to the millisecond>.json` next to `path`, with `-2`, `-3`... if that
/// name is taken, so an earlier copy is never replaced.
fn aside_name(path: &Path, now: NaiveDateTime, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let stamp = now.format("%Y%m%d-%H%M%S%.3f");
    let name = |n: u32| match n {
        1 => format!("prefs.corrupt-{stamp}.json"),
        n => format!("prefs.corrupt-{stamp}-{n}.json"),
    };
    (1..)
        .map(|n| path.with_file_name(name(n)))
        .find(|p| !exists(p))
        .unwrap_or_else(|| path.with_file_name(name(1)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::NaiveDate;
    use serde_json::json;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    fn app() -> tauri::App<MockRuntime> {
        mock_builder()
            .plugin(tauri_plugin_store::Builder::new().build())
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 6)
            .unwrap()
            .and_hms_opt(13, 22, 5)
            .unwrap()
    }

    /// A store in a temp folder (an absolute path, so the real app data folder is untouched).
    fn store(app: &tauri::App<MockRuntime>, dir: &Path) -> Arc<Store<MockRuntime>> {
        open_at(app.handle(), &dir.join(FILE), now()).unwrap()
    }

    #[test]
    fn defaults_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        assert_eq!(Prefs::load(&*store(&app, dir.path())), Prefs::default());
        assert_eq!(Prefs::default().last_save_dir(), None);
    }

    #[test]
    fn saves_the_struct_under_one_key_and_loads_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let s = store(&app, dir.path());
        let mut prefs = Prefs::default();
        assert!(prefs.set_last_save_dir(Path::new("/saved")));
        prefs.save(&*s).unwrap();
        let written: Value =
            serde_json::from_slice(&fs::read(dir.path().join(FILE)).unwrap()).unwrap();
        assert_eq!(written["prefs"]["lastSaveDir"], json!("/saved"));
        assert_eq!(written["prefs"]["checkForUpdates"], json!(true));
        assert_eq!(
            Prefs::load(&*s).last_save_dir(),
            Some(PathBuf::from("/saved"))
        );
    }

    #[test]
    fn a_bad_field_or_value_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let s = store(&app, dir.path());
        for bad in [
            json!({ "lastSaveDir": 42 }),
            json!({ "lastSaveDir": ["/a"] }),
            json!({ "checkForUpdates": "no" }),
            json!({ "checkForUpdates": null }),
            json!({ "shortcut": 1, "rememberRegion": "yes", "magnifier": null }),
            json!({ "quickCopy": "yes" }),
            json!({ "shortcut": "Control+Nope", "openAtLogin": 0, "checkForUpdates": [] }),
            json!({ "shortcut": "S", "loginItemAdded": "no" }),
            json!({ "printScreenNoticeShown": 1 }),
            json!({}),
            json!({ "unknown": true }),
            json!("not an object"),
            json!(null),
        ] {
            s.set(KEY, bad.clone());
            // Stored prefs without `onboarded` are from before onboarding (see `onboarding`).
            let onboarded = if bad.is_object() { None } else { Some(false) };
            let expected = Prefs {
                onboarded,
                ..Prefs::default()
            };
            assert_eq!(Prefs::load(&*s), expected, "{bad}");
        }
        // A missing one stays missing when saved, so the migration still sees it.
        let from_before = Prefs {
            onboarded: None,
            ..Prefs::default()
        };
        let stored = serde_json::to_value(&from_before).unwrap();
        assert!(stored.get("onboarded").is_none(), "{stored}");
        // An invalid `onboarded` is not done, unlike a missing one.
        for bad in [json!("yes"), json!(null), json!(1)] {
            s.set(KEY, json!({ "onboarded": bad }));
            assert_eq!(Prefs::load(&*s).onboarded, Some(false), "{bad}");
        }
    }

    #[test]
    fn update_checks_default_to_on_and_can_be_turned_off() {
        assert!(Prefs::default().check_for_updates);
        let off: Prefs = serde_json::from_value(json!({ "checkForUpdates": false })).unwrap();
        assert!(!off.check_for_updates);
    }

    #[test]
    fn defaults_for_each_setting() {
        let p = Prefs::default();
        assert_eq!(p.shortcut, shortcut::DEFAULT);
        assert!(p.remember_region && !p.magnifier && p.open_at_login && p.check_for_updates);
        assert!(!p.quick_copy);
        assert!(!p.login_item_added && !p.launch_agent_removed && !p.print_screen_notice_shown);
        assert_eq!(p.onboarded, Some(false), "a fresh install");
    }

    /// Also how onboarding tells an install from before it (`onboarded: None`).
    #[test]
    fn a_file_from_before_the_settings_loads_with_their_defaults() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(FILE),
            br#"{"prefs":{"lastSaveDir":"/old"}}"#,
        )
        .unwrap();
        let app = app();
        let loaded = Prefs::load(&*store(&app, dir.path()));
        let expected = Prefs {
            last_save_dir: Some("/old".into()),
            onboarded: None,
            ..Prefs::default()
        };
        assert_eq!(loaded, expected);
    }

    #[test]
    fn stored_settings_load_back() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let s = store(&app, dir.path());
        let prefs = Prefs {
            last_save_dir: None,
            shortcut: "Alt+Shift+F5".into(),
            remember_region: false,
            magnifier: true,
            quick_copy: true,
            open_at_login: false,
            check_for_updates: false,
            login_item_added: true,
            launch_agent_removed: true,
            print_screen_notice_shown: true,
            onboarded: Some(true),
            quick_copy_tip: Tip::default(),
            regions: Vec::new(),
        };
        prefs.save(&*s).unwrap();
        assert_eq!(Prefs::load(&*s), prefs);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_is_not_utf8_is_not_remembered() {
        use std::os::unix::ffi::OsStrExt;
        let mut prefs = Prefs::default();
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/\xff"));
        assert!(!prefs.set_last_save_dir(odd));
        assert_eq!(prefs, Prefs::default());
    }

    #[test]
    fn a_failed_write_keeps_the_previous_prefs_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("sub");
        fs::create_dir(&folder).unwrap();
        let app = app();
        let s = store(&app, &folder);
        let old = Prefs {
            magnifier: true,
            ..Prefs::default()
        };
        old.save(&*s).unwrap();
        // A file where the folder was: writing fails.
        fs::remove_dir_all(&folder).unwrap();
        fs::write(&folder, b"").unwrap();
        assert!(Prefs::default().save(&*s).is_err());
        assert_eq!(Prefs::load(&*s), old);

        // Nothing saved before: the key is gone again.
        let other = tempfile::tempdir().unwrap();
        let fresh_folder = other.path().join("sub");
        fs::create_dir(&fresh_folder).unwrap();
        let fresh = store(&app, &fresh_folder);
        fs::remove_dir_all(&fresh_folder).unwrap();
        fs::write(&fresh_folder, b"").unwrap();
        assert!(old.save(&*fresh).is_err());
        assert!(fresh.get(KEY).is_none());
    }

    /// Prefs in memory that count saves, which can be made to fail.
    #[derive(Default)]
    pub struct FakeStorage {
        pub prefs: std::cell::RefCell<Prefs>,
        pub fail: bool,
        /// Calls to `save`, failed ones included.
        pub saves: std::cell::Cell<usize>,
        /// Calls to `stage`, which always works.
        pub staged: std::cell::Cell<usize>,
    }

    impl FakeStorage {
        pub fn failing() -> Self {
            Self {
                fail: true,
                ..Self::default()
            }
        }
    }

    impl Storage for FakeStorage {
        fn load(&self) -> Prefs {
            self.prefs.borrow().clone()
        }

        fn save(&self, prefs: &Prefs) -> Result<(), String> {
            self.saves.set(self.saves.get() + 1);
            if self.fail {
                return Err("disk full".into());
            }
            *self.prefs.borrow_mut() = prefs.clone();
            Ok(())
        }

        fn stage(&self, prefs: &Prefs) -> Result<(), String> {
            self.staged.set(self.staged.get() + 1);
            *self.prefs.borrow_mut() = prefs.clone();
            Ok(())
        }
    }

    #[test]
    fn modify_saves_only_a_successful_change() {
        let mut storage = FakeStorage::default();
        let saved = modify(&storage, |p| {
            p.magnifier = true;
            Ok(())
        })
        .unwrap();
        assert!(saved.magnifier && storage.load().magnifier);
        assert_eq!(storage.saves.get(), 1);
        assert_eq!(modify(&storage, |_| Err("no")), Err("no"));
        assert_eq!(storage.saves.get(), 1, "a failed change writes nothing");
        storage.fail = true;
        assert_eq!(
            modify(&storage, |p| {
                p.magnifier = false;
                Ok(())
            }),
            Err(NOT_SAVED)
        );
        assert!(storage.load().magnifier);
    }

    #[test]
    fn without_a_store_nothing_is_saved() {
        assert_eq!(Unsaved.load(), Prefs::default());
        assert!(Unsaved.save(&Prefs::default()).is_err());
        assert!(Unsaved.stage(&Prefs::default()).is_err());
    }

    #[test]
    fn staged_prefs_are_read_at_once_and_written_by_the_next_save() {
        let dir = tempfile::tempdir().unwrap();
        let app = app();
        let s = store(&app, dir.path());
        let staged = Prefs {
            magnifier: true,
            ..Prefs::default()
        };
        Storage::stage(&*s, &staged).unwrap();
        assert_eq!(Storage::load(&*s), staged);
        assert!(!dir.path().join(FILE).exists(), "not written yet");
        s.save().unwrap();
        let written: Value =
            serde_json::from_slice(&fs::read(dir.path().join(FILE)).unwrap()).unwrap();
        assert_eq!(written["prefs"]["magnifier"], json!(true));
    }

    #[test]
    fn prefs_from_before_the_tip_with_quick_copy_on_have_it_over() {
        let load = |v: Value| Prefs::from_stored(v).unwrap();
        assert!(load(json!({ "quickCopy": true })).quick_copy_tip.done);
        assert!(!load(json!({ "quickCopy": false })).quick_copy_tip.done);
        let current = json!({ "quickCopy": true, "quickCopyTip": { "shown": 1 } });
        assert!(
            !load(current).quick_copy_tip.done,
            "only prefs from before it"
        );
        assert!(Prefs::from_stored(json!("nope")).is_none());
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_and_prefs_start_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        fs::write(&file, b"{ not json").unwrap();
        let app = app();
        let s = store(&app, dir.path());
        let aside = dir.path().join("prefs.corrupt-20261106-132205.000.json");
        assert_eq!(fs::read(&aside).unwrap(), b"{ not json");
        assert_eq!(Prefs::load(&*s), Prefs::default());
        let mut prefs = Prefs::default();
        prefs.set_last_save_dir(Path::new("/new"));
        prefs.save(&*s).unwrap();
        assert!(fs::read_to_string(&file).unwrap().contains("/new"));
    }

    #[test]
    fn only_a_file_that_is_not_a_json_object_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        assert_eq!(check(&file, now()), Check::Usable, "missing");
        fs::write(&file, b"{\"prefs\":{}}").unwrap();
        assert_eq!(check(&file, now()), Check::Usable, "valid");
        fs::write(&file, b"[1, 2]").unwrap();
        let first = dir.path().join("prefs.corrupt-20261106-132205.000.json");
        assert_eq!(check(&file, now()), Check::MovedAside(first.clone()));
        // A second one in the same millisecond never replaces the first.
        fs::write(&file, b"nope").unwrap();
        let second = dir.path().join("prefs.corrupt-20261106-132205.000-2.json");
        assert_eq!(check(&file, now()), Check::MovedAside(second.clone()));
        assert_eq!(fs::read(&first).unwrap(), b"[1, 2]");
        assert_eq!(fs::read(&second).unwrap(), b"nope");
    }

    #[test]
    fn an_unreadable_file_is_left_alone_and_defaults_are_used() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be cannot be read as a file.
        let file = dir.path().join(FILE);
        fs::create_dir(&file).unwrap();
        assert_eq!(check(&file, now()), Check::Unreadable);
        let app = app();
        assert!(open_at(app.handle(), &file, now()).is_none());
        assert!(file.is_dir(), "left where it was");
    }

    #[cfg(unix)]
    #[test]
    fn a_corrupt_file_that_cannot_be_moved_is_still_usable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        fs::write(&file, b"nope").unwrap();
        // A read-only folder: the file can be read but not renamed.
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let checked = check(&file, now());
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(checked, Check::Usable);
    }

    #[test]
    fn aside_names_count_up_when_taken() {
        let path = Path::new("/a/prefs.json");
        let taken = |n: usize| {
            move |p: &Path| {
                (1..=n).any(|i| {
                    let suffix = if i == 1 {
                        String::new()
                    } else {
                        format!("-{i}")
                    };
                    p == Path::new(&format!(
                        "/a/prefs.corrupt-20261106-132205.000{suffix}.json"
                    ))
                })
            }
        };
        assert_eq!(
            aside_name(path, now(), taken(0)),
            Path::new("/a/prefs.corrupt-20261106-132205.000.json")
        );
        assert_eq!(
            aside_name(path, now(), taken(3)),
            Path::new("/a/prefs.corrupt-20261106-132205.000-4.json")
        );
    }
}
