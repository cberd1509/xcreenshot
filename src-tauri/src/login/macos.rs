//! `SMAppService.mainApp` glue (macOS 13+). The decisions are in the parent module.

use super::{LoginItem, Status};
use objc2_service_management::SMAppService;
use std::io;
use std::path::{Path, PathBuf};

/// The app itself as a login item, listed by name in System Settings › Login Items.
pub struct MainApp {
    /// The old LaunchAgent to remove (see [`super::sync`]).
    launch_agent: Option<PathBuf>,
}

impl Default for MainApp {
    fn default() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self {
            launch_agent: home.as_deref().map(launch_agent),
        }
    }
}

impl LoginItem for MainApp {
    fn status(&self) -> Result<Status, String> {
        // SAFETY: plain SMAppService calls on any thread.
        let raw = unsafe { SMAppService::mainAppService().status() };
        Ok(from_raw(raw.0))
    }

    fn set_enabled(&self, on: bool) -> Result<(), String> {
        // SAFETY: as above.
        unsafe {
            let app = SMAppService::mainAppService();
            if on {
                app.registerAndReturnError()
            } else {
                app.unregisterAndReturnError()
            }
        }
        .map_err(|e| e.to_string())
    }

    fn remove_launch_agent(&self) -> Result<bool, String> {
        self.launch_agent.as_deref().map_or(Ok(false), remove_file)
    }
}

pub fn open_login_items() {
    // SAFETY: opens System Settings; no arguments.
    unsafe { SMAppService::openSystemSettingsLoginItems() }
}

/// From `SMAppServiceStatus`. An unknown value counts as [`Status::NotFound`].
fn from_raw(raw: isize) -> Status {
    match raw {
        0 => Status::NotRegistered,
        1 => Status::Enabled,
        2 => Status::RequiresApproval,
        _ => Status::NotFound,
    }
}

/// The LaunchAgent tauri-plugin-autostart wrote, under `home`.
fn launch_agent(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents/xcreenshot.plist")
}

/// Deletes `path` if it is there, as the plugin's own `disable` did. It isn't unloaded with
/// `launchctl bootout`: when launched at login, xcreenshot runs as that job, so unloading it
/// would quit the app. Without its file, launchd doesn't load it at the next login.
fn remove_file(path: &Path) -> Result<bool, String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the status only: tests never register, unregister or touch the real LaunchAgent.
    #[test]
    fn reads_the_status_and_removes_only_its_own_launch_agent() {
        let _ = MainApp::default();
        let home = tempfile::tempdir().unwrap();
        let app = MainApp {
            launch_agent: Some(launch_agent(home.path())),
        };
        assert!(app.status().is_ok());
        assert_eq!(app.remove_launch_agent(), Ok(false));
        let none = MainApp { launch_agent: None };
        assert_eq!(none.remove_launch_agent(), Ok(false));
    }

    #[test]
    fn status_follows_smappservice() {
        use Status::*;
        let all = [NotRegistered, Enabled, RequiresApproval, NotFound];
        for (raw, status) in (0..).zip(all) {
            assert_eq!(from_raw(raw), status);
        }
        assert_eq!(from_raw(7), NotFound);
        assert_eq!(from_raw(-1), NotFound);
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
}
