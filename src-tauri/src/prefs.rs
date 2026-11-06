//! Preferences kept across launches, in `prefs.json` in the app data folder.
//!
//! [`Prefs`] is a typed struct with defaults, stored as one value under one key of a
//! `tauri-plugin-store` store. A missing or invalid field falls back to its default on its own,
//! and a file that is not valid JSON is moved aside, so prefs never get stuck. Rust is the only
//! reader and writer: the pages get no store permission.

use chrono::NaiveDateTime;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::{Store, StoreExt};

const FILE: &str = "prefs.json";
const KEY: &str = "prefs";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    /// The folder of the last screenshot saved; the Save dialog opens there.
    #[serde(default, deserialize_with = "lenient")]
    pub last_save_dir: Option<String>,
}

/// A field that does not parse keeps its default instead of failing the whole struct.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_else(|e| {
        log::warn!("ignoring an invalid preference: {e}");
        T::default()
    }))
}

impl Prefs {
    /// The stored prefs, or the defaults if there are none or they are not an object.
    pub fn load<R: Runtime>(store: &Store<R>) -> Self {
        store
            .get(KEY)
            .and_then(|v| {
                serde_json::from_value(v)
                    .inspect_err(|e| log::warn!("ignoring invalid prefs: {e}"))
                    .ok()
            })
            .unwrap_or_default()
    }

    /// Stores the prefs and writes them to disk.
    pub fn save<R: Runtime>(&self, store: &Store<R>) -> Result<(), String> {
        let value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        store.set(KEY, value);
        store.save().map_err(|e| e.to_string())
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

/// The app's prefs store. `None` (logged) if it cannot be opened; callers use the defaults.
pub fn open<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<Store<R>>> {
    let dir = app
        .path()
        .app_data_dir()
        .inspect_err(|e| log::error!("no app data folder for {FILE}: {e}"))
        .ok()?;
    open_at(app, &dir.join(FILE), chrono::Local::now().naive_local())
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
mod tests {
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
        assert_eq!(written, json!({ "prefs": { "lastSaveDir": "/saved" } }));
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
            json!({}),
            json!({ "unknown": true }),
            json!("not an object"),
            json!(null),
        ] {
            s.set(KEY, bad.clone());
            assert_eq!(Prefs::load(&*s), Prefs::default(), "{bad}");
        }
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
