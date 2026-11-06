//! The PNG files handed to the share sheet: named after the capture time, written whole, and
//! deleted when sharing ends (or at the next startup if it never reports back).

use chrono::NaiveDateTime;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;
use tokio::sync::Notify;

const PREFIX: &str = "xcreenshot-";
const EXTENSION: &str = ".png";

/// Temp files in one directory owned by the app. Managed as app state by [`super::init`].
#[derive(Debug)]
pub struct TempFiles {
    dir: PathBuf,
    /// Files still being shared, with the time to delete them once sharing has ended. Never
    /// reused for a new file, never swept. Every file operation on a live path happens under
    /// this lock, so a name is never handed out while its old file is still being deleted.
    live: Mutex<HashMap<PathBuf, Option<Instant>>>,
    /// Wakes [`Self::run_deadlines`] when a deadline is added.
    wake: Notify,
}

impl TempFiles {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            live: Mutex::default(),
            wake: Notify::new(),
        }
    }

    fn live(&self) -> MutexGuard<'_, HashMap<PathBuf, Option<Instant>>> {
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
                .find(|p| !live.contains_key(p))
                .expect("unbounded range");
            live.insert(path.clone(), None);
            path
        };
        if let Err(e) = write_private(&self.dir, &path, png) {
            // A partial file goes too, before the name is freed.
            let mut live = self.live();
            remove(&path);
            live.remove(&path);
            return Err(e);
        }
        Ok(path)
    }

    /// Sharing ended: deletes the file now. Unknown paths are left alone.
    pub fn release(&self, path: &Path) {
        let mut live = self.live();
        if live.contains_key(path) {
            // Deleted before the name is freed, both under the lock.
            remove(path);
            live.remove(path);
        }
    }

    /// Sharing ended, but the file is kept until `at` (see [`Self::run_deadlines`]).
    pub fn release_after(&self, path: &Path, at: Instant) {
        if let Some(deadline) = self.live().get_mut(path) {
            *deadline = Some(at);
            self.wake.notify_one();
        }
    }

    /// Deletes the files whose deadline has passed. Returns how many.
    pub fn release_due(&self, now: Instant) -> usize {
        let mut live = self.live();
        let due: Vec<PathBuf> = live
            .iter()
            .filter(|(_, at)| at.is_some_and(|at| at <= now))
            .map(|(p, _)| p.clone())
            .collect();
        for path in &due {
            remove(path);
            live.remove(path);
        }
        due.len()
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.live().values().filter_map(|at| *at).min()
    }

    /// The one sweeper for deadlines: sleeps until the next one (or until a new one is added)
    /// and deletes what is due. Runs for the life of the app.
    pub async fn run_deadlines(&self) {
        loop {
            let added = self.wake.notified();
            match self.next_deadline() {
                Some(at) => {
                    let wait = at.saturating_duration_since(Instant::now());
                    let _ = tokio::time::timeout(wait, added).await;
                }
                None => added.await,
            }
            self.release_due(Instant::now());
        }
    }

    /// Deletes our files that are not being shared, e.g. left behind by a crash or by a
    /// service that never reported back. Returns how many were deleted.
    pub fn sweep(&self) -> usize {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!("could not sweep {}: {e}", self.dir.display());
                }
                return 0;
            }
        };
        // Held throughout, so a file reserved by `create` meanwhile is never deleted.
        let live = self.live();
        entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .filter(|p| is_ours(p) && !live.contains_key(p))
            .filter(|p| remove(p))
            .count()
    }
}

/// Writes `png` to `path` (truncating), readable by this user only on Unix: the directory is
/// 0700 and the file 0600, whatever the umask or the modes of leftovers.
fn write_private(dir: &Path, path: &Path, png: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        builder.mode(0o700);
        options.mode(0o600);
    }
    builder.create(dir)?;
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(png)
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
    use std::time::Duration;

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
        assert!(files.live().is_empty());
        std::fs::remove_file(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_deleted_is_logged_and_kept() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = scratch("readonly");
        let files = TempFiles::new(dir.clone());
        let path = files.create(b"png", at(13, 22, 5)).unwrap();
        // Root ignores directory permissions, so the file can't be kept from it.
        if std::fs::metadata(&path).unwrap().uid() == 0 {
            std::fs::remove_dir_all(dir).unwrap();
            return;
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        files.release(&path);
        let kept = path.exists();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(kept);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deadlines_delete_only_what_is_due() {
        let dir = scratch("deadlines");
        let files = TempFiles::new(dir.clone());
        let t0 = Instant::now();
        let soon = files.create(b"a", at(13, 22, 5)).unwrap();
        let later = files.create(b"b", at(13, 22, 5)).unwrap();
        let open = files.create(b"c", at(13, 22, 5)).unwrap();
        files.release_after(&soon, t0 + Duration::from_secs(1));
        files.release_after(&later, t0 + Duration::from_secs(5));
        // Unknown paths are ignored.
        files.release_after(&dir.join("other.png"), t0);
        assert_eq!(files.next_deadline(), Some(t0 + Duration::from_secs(1)));
        assert_eq!(files.release_due(t0), 0);
        assert_eq!(files.release_due(t0 + Duration::from_secs(2)), 1);
        assert!(!soon.exists() && later.exists() && open.exists());
        assert_eq!(files.next_deadline(), Some(t0 + Duration::from_secs(5)));
        assert_eq!(files.release_due(t0 + Duration::from_secs(9)), 1);
        assert!(open.exists(), "still being shared");
        assert_eq!(files.next_deadline(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_sweeper_deletes_files_when_due() {
        let dir = scratch("sweeper");
        let files = std::sync::Arc::new(TempFiles::new(dir.clone()));
        let path = files.create(b"png", at(13, 22, 5)).unwrap();
        let runner = files.clone();
        tauri::async_runtime::spawn(async move { runner.run_deadlines().await });
        // Added while the sweeper is idle: it wakes up for it.
        files.release_after(&path, Instant::now() + Duration::from_millis(30));
        let deadline = Instant::now() + Duration::from_secs(5);
        while path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_name_is_never_reused_before_its_file_is_gone() {
        // Many threads share and release in the same second: each one's file must hold its
        // own bytes until it releases it.
        let dir = scratch("race");
        let files = std::sync::Arc::new(TempFiles::new(dir.clone()));
        let workers: Vec<_> = (0..8u8)
            .map(|i| {
                let files = files.clone();
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        let path = files.create(&[i; 16], at(13, 22, 5)).unwrap();
                        assert_eq!(std::fs::read(&path).unwrap(), [i; 16]);
                        files.release(&path);
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        assert!(files.live().is_empty());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn only_this_user_can_read_the_files() {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = scratch("private");
        // A leftover directory and file that anyone could read.
        std::fs::DirBuilder::new().mode(0o755).create(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let leftover = dir.join(file_name(at(13, 22, 5), 1));
        std::fs::write(&leftover, b"old").unwrap();
        std::fs::set_permissions(&leftover, std::fs::Permissions::from_mode(0o644)).unwrap();
        let files = TempFiles::new(dir.clone());
        let reused = files.create(b"png", at(13, 22, 5)).unwrap();
        assert_eq!(reused, leftover);
        let fresh = files.create(b"png", at(13, 22, 5)).unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&reused), 0o600);
        assert_eq!(mode(&fresh), 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_write_leaves_no_partial_file() {
        let dir = scratch("partial");
        std::fs::create_dir_all(&dir).unwrap();
        // A directory where the file should go: opening it for writing fails.
        let blocked = dir.join(file_name(at(13, 22, 5), 1));
        std::fs::create_dir(&blocked).unwrap();
        let files = TempFiles::new(dir.clone());
        assert!(files.create(b"png", at(13, 22, 5)).is_err());
        assert!(files.live().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unreadable_directory_sweeps_nothing() {
        let dir = scratch("unreadable");
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(&dir, b"a file, not a directory").unwrap();
        assert_eq!(TempFiles::new(dir.clone()).sweep(), 0);
        std::fs::remove_file(dir).unwrap();
    }
}
