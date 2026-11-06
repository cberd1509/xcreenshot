//! Screen Recording permission (macOS): the window with the steps to allow it.
//!
//! Without the permission, the first launch and the hotkey open this window instead of the
//! overlay. The page re-checks the permission itself while it is open. Other platforms always
//! have permission, so the window never opens there.

use crate::capture;
use crate::windows::show_window;
use tauri::{AppHandle, Runtime, WebviewUrl};

pub const WINDOW: &str = "permission";

/// Opens (or focuses) the steps window.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    let url = WebviewUrl::App("permission.html".into());
    let shown = show_window(app, WINDOW, url, |b| {
        b.title("xcreenshot")
            .inner_size(460.0, 540.0)
            .resizable(false)
            .center()
    });
    if let Err(e) = shown {
        log::error!("could not open the permission window: {e}");
    }
}

/// Whether capture is allowed now. The page polls it while open.
#[tauri::command]
pub fn permission_granted() -> bool {
    capture::has_permission()
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
    use tauri::Manager;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn show_opens_one_window() {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        show(app.handle());
        show(app.handle());
        assert_eq!(app.webview_windows().len(), 1);
        assert!(app.get_webview_window(WINDOW).is_some());
    }
}
