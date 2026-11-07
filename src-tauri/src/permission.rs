//! Screen Recording permission (macOS): the commands behind the onboarding window's steps.
//!
//! Without the permission, the first launch and the hotkey open that window (see `onboarding`)
//! instead of the overlay. The page re-checks the permission itself while it is open. Other
//! platforms always have permission.

use crate::capture;
use tauri::{AppHandle, Runtime};

/// Whether capture is allowed now. The page polls it while open.
#[tauri::command]
pub fn permission_granted() -> bool {
    capture::has_permission()
}

/// The System Settings pane's name, which macOS 15 renamed.
fn pane_name(macos_major: isize) -> &'static str {
    if macos_major >= 15 {
        "Screen & System Audio Recording"
    } else {
        "Screen Recording"
    }
}

/// The running macOS's major version (0 elsewhere).
fn macos_major() -> isize {
    #[cfg(target_os = "macos")]
    return objc2_foundation::NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion;
    #[cfg(not(target_os = "macos"))]
    0
}

/// The name of the Privacy & Security pane to turn xcreenshot on in, for the steps.
#[tauri::command]
pub fn permission_pane() -> &'static str {
    pane_name(macos_major())
}

/// Privacy & Security › Screen Recording in System Settings.
#[cfg(target_os = "macos")]
const SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";

/// Opens Privacy & Security › Screen Recording. Asking first adds xcreenshot to that list (the
/// first time it also shows the system prompt).
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn open_permission_settings() -> Result<(), String> {
    capture::request_permission();
    open::that_detached(SETTINGS_URL).map_err(|e| e.to_string())
}

/// macOS often applies the permission only after the app restarts. Exits through the normal
/// path, so the single-instance lock is released before the new process starts.
#[tauri::command]
pub fn restart_app<R: Runtime>(app: AppHandle<R>) {
    app.request_restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_sees_the_real_permission() {
        assert_eq!(permission_granted(), capture::has_permission());
    }

    #[test]
    fn names_the_pane_by_macos_version() {
        assert_eq!(pane_name(14), "Screen Recording");
        assert_eq!(pane_name(15), "Screen & System Audio Recording");
        assert_eq!(pane_name(26), "Screen & System Audio Recording");
    }

    /// The command's answer matches the version `sw_vers` reports for this Mac.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_command_names_this_macs_pane() {
        let out = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .unwrap();
        let version = String::from_utf8(out.stdout).unwrap();
        let major: isize = version.trim().split('.').next().unwrap().parse().unwrap();
        assert_eq!(macos_major(), major);
        assert_eq!(permission_pane(), pane_name(major));
    }
}
