//! `SMAppService.mainApp` glue (macOS 13+). The decisions are in the parent module.

use super::{LoginItem, Status};
use objc2_service_management::SMAppService;
use std::io;
use std::path::{Path, PathBuf};

/// The app itself as a login item, listed by name in System Settings › Login Items.
pub struct MainApp {
    /// The home folder, where the old LaunchAgent is (see [`super::sync`]); `None` if `HOME`
    /// isn't set.
    home: Option<PathBuf>,
}

impl Default for MainApp {
    fn default() -> Self {
        Self {
            home: std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(PathBuf::from),
        }
    }
}

impl MainApp {
    fn launch_agents(&self) -> Result<[PathBuf; 2], String> {
        self.home
            .as_deref()
            .map(launch_agents)
            .ok_or_else(|| "HOME isn't set".to_owned())
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

    fn has_launch_agent(&self) -> Result<bool, String> {
        for path in self.launch_agents()? {
            if path
                .try_exists()
                .map_err(|e| format!("{}: {e}", path.display()))?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn remove_launch_agent(&self) -> Result<(), String> {
        self.launch_agents()?
            .iter()
            .try_for_each(|p| remove_file(p))
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

/// Where tauri-plugin-autostart may have written its LaunchAgent, under `home`: named after the
/// product (what it used), or after the bundle id.
fn launch_agents(home: &Path) -> [PathBuf; 2] {
    let dir = home.join("Library/LaunchAgents");
    [
        dir.join("xcreenshot.plist"),
        dir.join("com.bervex.xcreenshot.plist"),
    ]
}

/// Deletes `path` if it is there, as the plugin's own `disable` did. It isn't unloaded with
/// `launchctl bootout`: when launched at login, xcreenshot runs as that job, so unloading it
/// would quit the app. Without its file, launchd doesn't load it at the next login.
fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(home: &Path) -> MainApp {
        MainApp {
            home: Some(home.to_owned()),
        }
    }

    /// Reads the status only: tests never register, unregister or touch the real LaunchAgent.
    #[test]
    fn reads_the_status() {
        let _ = MainApp::default();
        let home = tempfile::tempdir().unwrap();
        assert!(app(home.path()).status().is_ok());
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
    fn finds_and_deletes_either_old_launch_agent() {
        let home = tempfile::tempdir().unwrap();
        let app = app(home.path());
        let [product, bundle_id] = launch_agents(home.path());
        assert!(product.ends_with("Library/LaunchAgents/xcreenshot.plist"));
        assert!(bundle_id.ends_with("Library/LaunchAgents/com.bervex.xcreenshot.plist"));
        assert_eq!(app.has_launch_agent(), Ok(false));
        assert_eq!(
            app.remove_launch_agent(),
            Ok(()),
            "nothing to delete is fine"
        );
        std::fs::create_dir_all(product.parent().unwrap()).unwrap();
        for path in [&product, &bundle_id] {
            std::fs::write(path, "<plist/>").unwrap();
            assert_eq!(app.has_launch_agent(), Ok(true));
            assert_eq!(app.remove_launch_agent(), Ok(()));
            assert!(!path.exists());
            assert_eq!(app.has_launch_agent(), Ok(false));
        }
        // A folder in its place can't be deleted as a file.
        std::fs::create_dir(&product).unwrap();
        let err = app.remove_launch_agent().unwrap_err();
        assert!(err.contains("xcreenshot.plist"), "{err}");
    }

    #[test]
    fn without_a_home_folder_there_is_no_answer() {
        let app = MainApp { home: None };
        assert!(app.has_launch_agent().is_err());
        assert!(app.remove_launch_agent().is_err());
    }
}
