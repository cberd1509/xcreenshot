//! `SMAppService.mainApp` glue (macOS 13+). The decisions are in the parent module.

use super::{LoginItem, Status};
use objc2_service_management::SMAppService;
use std::path::PathBuf;

/// The app itself as a login item, listed by name in System Settings › Login Items.
pub struct MainApp {
    /// The old LaunchAgent to remove (see [`super::sync`]).
    launch_agent: Option<PathBuf>,
}

impl Default for MainApp {
    fn default() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self {
            launch_agent: home.as_deref().map(super::launch_agent),
        }
    }
}

impl LoginItem for MainApp {
    fn status(&self) -> Result<Status, String> {
        // SAFETY: plain SMAppService calls on any thread.
        let raw = unsafe { SMAppService::mainAppService().status() };
        Ok(Status::from_raw(raw.0))
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
        self.launch_agent
            .as_deref()
            .map_or(Ok(false), super::remove_file)
    }
}

pub fn open_login_items() {
    // SAFETY: opens System Settings; no arguments.
    unsafe { SMAppService::openSystemSettingsLoginItems() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the status only: tests never register, unregister or touch the real LaunchAgent.
    #[test]
    fn reads_the_status_and_removes_only_its_own_launch_agent() {
        assert!(MainApp::default().launch_agent.is_some());
        let home = tempfile::tempdir().unwrap();
        let app = MainApp {
            launch_agent: Some(super::super::launch_agent(home.path())),
        };
        assert!(app.status().is_ok());
        assert_eq!(app.remove_launch_agent(), Ok(false));
        let none = MainApp { launch_agent: None };
        assert_eq!(none.remove_launch_agent(), Ok(false));
    }
}
