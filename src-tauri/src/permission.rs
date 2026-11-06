//! Screen Recording permission (macOS): the steps window and the re-check behind it.
//!
//! Without the permission, the first launch and the hotkey open the "permission" window
//! instead of the overlay. While it is open, a worker thread checks the permission every second
//! and sends `permission:granted` to the window when it flips, then stops. It also stops when the
//! window closes. Other platforms always have permission, so the window never opens there.

use crate::capture;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

pub const WINDOW: &str = "permission";
/// Sent to the window once the permission is granted. No payload.
pub const GRANTED_EVENT: &str = "permission:granted";
/// Privacy & Security › Screen Recording in System Settings.
pub const SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
const RECHECK: Duration = Duration::from_secs(1);

/// Whether a re-check thread is running for this app (at most one).
#[derive(Default)]
struct Watching(AtomicBool);

/// How the permission is checked: the OS call in the app, a fake in tests.
#[derive(Clone, Copy)]
struct Checker {
    granted: fn() -> bool,
    every: Duration,
}

const OS: Checker = Checker {
    granted: capture::has_permission,
    every: RECHECK,
};

/// Called before a capture and at launch. True if capture may go ahead; otherwise opens the
/// steps window and returns false.
pub fn ensure<R: Runtime>(app: &AppHandle<R>) -> bool {
    ensure_with(app, OS)
}

fn ensure_with<R: Runtime>(app: &AppHandle<R>, checker: Checker) -> bool {
    if (checker.granted)() {
        return true;
    }
    if let Err(e) = show(app, checker) {
        log::error!("could not open the permission window: {e}");
    }
    false
}

fn show<R: Runtime>(app: &AppHandle<R>, checker: Checker) -> tauri::Result<()> {
    let window = match app.get_webview_window(WINDOW) {
        Some(w) => w,
        None => WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("permission.html".into()))
            .title("xcreenshot")
            .inner_size(460.0, 520.0)
            .resizable(false)
            .center()
            .build()?,
    };
    window.unminimize()?;
    window.show()?;
    window.set_focus()?;
    watch(app, checker);
    Ok(())
}

/// Starts the re-check thread unless one is already running.
fn watch<R: Runtime>(app: &AppHandle<R>, checker: Checker) {
    app.manage(Watching::default());
    if app.state::<Watching>().0.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let open = || app.get_webview_window(WINDOW).is_some();
        if recheck(open, checker.granted, checker.every)
            && let Err(e) = app.emit_to(WINDOW, GRANTED_EVENT, ())
        {
            log::error!("could not report the permission: {e}");
        }
        app.state::<Watching>().0.store(false, Ordering::Release);
    });
}

/// Checks every `every` until the permission is granted (true) or the window closes (false).
fn recheck(open: impl Fn() -> bool, granted: impl Fn() -> bool, every: Duration) -> bool {
    loop {
        std::thread::sleep(every);
        if !open() {
            return false;
        }
        if granted() {
            return true;
        }
    }
}

/// Whether capture is allowed now. The window asks once when it loads.
#[tauri::command]
pub fn permission_granted() -> bool {
    capture::has_permission()
}

/// Opens Privacy & Security › Screen Recording. Asking first adds xcreenshot to that list (the
/// first time it also shows the system prompt).
#[tauri::command]
pub fn open_permission_settings() -> Result<(), String> {
    capture::request_permission();
    open::that_detached(SETTINGS_URL).map_err(|e| e.to_string())
}

/// macOS often applies the permission only after the app restarts.
#[tauri::command]
pub fn restart_app<R: Runtime>(app: AppHandle<R>) {
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::time::Instant;
    use tauri::Listener;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    type Handle = AppHandle<MockRuntime>;

    fn app() -> tauri::App<MockRuntime> {
        mock_builder().build(mock_context(noop_assets())).unwrap()
    }

    fn wait_until(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn watching(app: &Handle) -> bool {
        app.state::<Watching>().0.load(Ordering::Acquire)
    }

    fn checker(granted: fn() -> bool) -> Checker {
        Checker {
            granted,
            every: Duration::from_millis(1),
        }
    }

    #[test]
    fn settings_url_opens_screen_recording() {
        assert!(SETTINGS_URL.starts_with("x-apple.systempreferences:"));
        assert!(SETTINGS_URL.ends_with("?Privacy_ScreenCapture"));
    }

    #[test]
    fn the_page_reads_the_os_state() {
        assert_eq!(permission_granted(), capture::has_permission());
    }

    #[test]
    fn recheck_returns_once_granted() {
        let checks = Cell::new(0);
        let granted = || {
            checks.set(checks.get() + 1);
            checks.get() == 3
        };
        assert!(recheck(|| true, granted, Duration::ZERO));
        assert_eq!(checks.get(), 3);
    }

    #[test]
    fn recheck_stops_when_the_window_closes() {
        let looks = Cell::new(0);
        let open = || {
            looks.set(looks.get() + 1);
            looks.get() < 2
        };
        let granted = Cell::new(false);
        assert!(!recheck(open, || granted.replace(true), Duration::ZERO));
        // Closed before the second check, so it never saw the grant.
        assert!(granted.get());
        assert_eq!(looks.get(), 2);
    }

    #[test]
    fn granted_skips_the_window() {
        let app = app();
        assert!(ensure_with(app.handle(), checker(|| true)));
        assert!(app.get_webview_window(WINDOW).is_none());
    }

    #[test]
    fn missing_permission_opens_the_window_and_reports_the_grant() {
        static GRANTED: AtomicBool = AtomicBool::new(false);
        let app = app();
        let handle = app.handle().clone();
        let (tx, rx) = mpsc::channel();
        handle.listen_any(GRANTED_EVENT, move |_| tx.send(()).unwrap());

        let check = checker(|| GRANTED.load(Ordering::Acquire));
        assert!(!ensure_with(&handle, check));
        assert!(handle.get_webview_window(WINDOW).is_some());
        assert!(watching(&handle));

        GRANTED.store(true, Ordering::Release);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        wait_until("the re-check to stop", || !watching(&handle));
        assert!(ensure_with(&handle, check));
    }

    #[test]
    fn pressing_again_reuses_the_window_and_the_re_check() {
        static CHECKS: AtomicUsize = AtomicUsize::new(0);
        static GRANTED: AtomicBool = AtomicBool::new(false);
        let app = app();
        let handle = app.handle().clone();
        let check = checker(|| {
            CHECKS.fetch_add(1, Ordering::Relaxed);
            GRANTED.load(Ordering::Acquire)
        });

        assert!(!ensure_with(&handle, check));
        wait_until("the re-check to run", || CHECKS.load(Ordering::Relaxed) > 2);
        assert!(!ensure_with(&handle, check));
        assert_eq!(handle.webview_windows().len(), 1);
        assert!(watching(&handle));

        GRANTED.store(true, Ordering::Release);
        wait_until("the re-check to stop", || !watching(&handle));
    }
}
