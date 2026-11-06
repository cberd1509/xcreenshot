//! The PNG files handed to the share sheet: named after the capture time, written whole, and
//! deleted when sharing ends (or at the next startup if it never reports back).

use chrono::NaiveDateTime;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

const PREFIX: &str = "xcreenshot-";
const EXTENSION: &str = ".png";

/// Temp files in one directory owned by the app. Managed as app state by [`super::init`].
#[derive(Debug)]
pub struct TempFiles {
    dir: PathBuf,
    /// Files still being shared. Never reused for a new file, never swept.
    live: Mutex<HashSet<PathBuf>>,
}

impl TempFiles {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            live: Mutex::default(),
        }
    }

    fn live(&self) -> MutexGuard<'_, HashSet<PathBuf>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Writes `png` to a new file named after `now`, e.g. `xcreenshot-2026-11-06-13.22.05.png`.
    /// A second share in the same second gets `-2`, `-3`… A leftover file with the same name
    /// is replaced: the write truncates, so no bytes of an older, longer file survive.
    pub fn create(&self, png: &[u8], now: NaiveDateTime) -> io::Result<PathBuf> {
        // Reserve the name under the lock (so sweep leaves it alone), write without it.
        let path = {
            let mut live = self.live();
            let path = (1..)
                .map(|n| self.dir.join(file_name(now, n)))
                .find(|p| !live.contains(p))
                .expect("unbounded range");
            live.insert(path.clone());
            path
        };
        let written = std::fs::create_dir_all(&self.dir).and_then(|()| std::fs::write(&path, png));
        if let Err(e) = written {
            self.live().remove(&path);
            return Err(e);
        }
        Ok(path)
    }

    /// Sharing ended: deletes the file. Unknown paths are left alone.
    pub fn release(&self, path: &Path) {
        if self.live().remove(path) {
            remove(path);
        }
    }

    /// Deletes our files that are not being shared, e.g. left behind by a crash or by a
    /// service that never reported back. Returns how many were deleted.
    pub fn sweep(&self) -> usize {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        // Held throughout, so a file reserved by `create` meanwhile is never deleted.
        let live = self.live();
        entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .filter(|p| is_ours(p) && !live.contains(p))
            .filter(|p| remove(p))
            .count()
    }
}

fn remove(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => {
            log::warn!("could not delete {}: {e}", path.display());
            false
        }
    }
}

/// `xcreenshot-2026-11-06-13.22.05.png` for the first file (`n == 1`), then `…-13.22.05-2.png`.
/// Dots in the time, since some services and file systems reject colons.
fn file_name(now: NaiveDateTime, n: u32) -> String {
    let stamp = now.format("%Y-%m-%d-%H.%M.%S");
    if n == 1 {
        format!("{PREFIX}{stamp}{EXTENSION}")
    } else {
        format!("{PREFIX}{stamp}-{n}{EXTENSION}")
    }
}

fn is_ours(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(EXTENSION))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(h: u32, m: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 6)
            .unwrap()
            .and_hms_opt(h, m, s)
            .unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xcreenshot-share-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn names_come_from_the_clock() {
        assert_eq!(
            file_name(at(13, 22, 5), 1),
            "xcreenshot-2026-11-06-13.22.05.png"
        );
        assert_eq!(
            file_name(at(9, 0, 0), 1),
            "xcreenshot-2026-11-06-09.00.00.png"
        );
        assert_eq!(
            file_name(at(13, 22, 5), 3),
            "xcreenshot-2026-11-06-13.22.05-3.png"
        );
    }

    #[test]
    fn our_files_are_recognized_by_name() {
        assert!(is_ours(Path::new("/x/xcreenshot-2026-11-06-13.22.05.png")));
        for other in ["/x/photo.png", "/x/xcreenshot-notes.txt", "/x/", "/"] {
            assert!(!is_ours(Path::new(other)), "{other}");
        }
    }

    #[test]
    fn create_writes_the_png_and_release_deletes_it() {
        let dir = scratch("create");
        let files = TempFiles::new(dir.clone());
        let path = files.create(b"png", at(13, 22, 5)).unwrap();
        assert_eq!(path, dir.join("xcreenshot-2026-11-06-13.22.05.png"));
        assert_eq!(std::fs::read(&path).unwrap(), b"png");
        files.release(&path);
        assert!(!path.exists());
        // Releasing twice, or a path we never made, is harmless.
        files.release(&path);
        let stranger = dir.join("xcreenshot-stranger.png");
        std::fs::write(&stranger, b"x").unwrap();
        files.release(&stranger);
        assert!(stranger.exists(), "not ours to delete");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shares_in_the_same_second_get_their_own_file() {
        let dir = scratch("same-second");
        let files = TempFiles::new(dir.clone());
        let a = files.create(b"a", at(13, 22, 5)).unwrap();
        let b = files.create(b"b", at(13, 22, 5)).unwrap();
        assert_eq!(b, dir.join("xcreenshot-2026-11-06-13.22.05-2.png"));
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        // Once the first is done its name is free again.
        files.release(&a);
        assert_eq!(files.create(b"c", at(13, 22, 5)).unwrap(), a);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_leftover_longer_file_is_truncated_not_patched() {
        let dir = scratch("truncate");
        std::fs::create_dir_all(&dir).unwrap();
        let name = dir.join(file_name(at(13, 22, 5), 1));
        std::fs::write(&name, vec![0xAA; 4096]).unwrap();
        let files = TempFiles::new(dir.clone());
        let path = files.create(b"short", at(13, 22, 5)).unwrap();
        assert_eq!(path, name);
        assert_eq!(std::fs::read(&path).unwrap(), b"short", "no trailing bytes");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sweep_deletes_only_idle_files_of_ours() {
        let dir = scratch("sweep");
        let files = TempFiles::new(dir.clone());
        assert_eq!(files.sweep(), 0, "no directory yet");
        let live = files.create(b"live", at(13, 22, 5)).unwrap();
        let stale = dir.join("xcreenshot-2026-01-01-00.00.00.png");
        let foreign = dir.join("notes.txt");
        let folder = dir.join("xcreenshot-folder.png");
        std::fs::write(&stale, b"old").unwrap();
        std::fs::write(&foreign, b"keep").unwrap();
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(files.sweep(), 1);
        assert!(!stale.exists());
        assert!(live.exists() && foreign.exists() && folder.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn create_fails_when_the_directory_cannot_be_made() {
        let dir = scratch("blocked");
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(&dir, b"a file, not a directory").unwrap();
        let files = TempFiles::new(dir.join("share"));
        assert!(files.create(b"png", at(13, 22, 5)).is_err());
        assert_eq!(files.live().len(), 0);
        std::fs::remove_file(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_deleted_is_logged_and_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("readonly");
        let files = TempFiles::new(dir.clone());
        let path = files.create(b"png", at(13, 22, 5)).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        files.release(&path);
        let kept = path.exists();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(kept);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
