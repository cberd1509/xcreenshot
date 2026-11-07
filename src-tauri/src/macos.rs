//! Shared AppKit helpers. `overlay/window.rs` (`macos::raise`) still has its own copy of the
//! pointer cast and the level; it should switch to these.

use crate::focus::AppId;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSScreenSaverWindowLevel,
    NSWindow, NSWorkspace,
};
use tauri::{Runtime, WebviewWindow};

/// The overlay's window level: above the menu bar, the Dock and pop-up menus. Only the share
/// debug path uses it until the overlay switches over.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) const SCREEN_SAVER_LEVEL: isize = NSScreenSaverWindowLevel;

/// The window's `NSWindow`. Only use it on the main thread.
pub(crate) fn ns_window<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<&NSWindow> {
    let ptr = window.ns_window()?;
    // SAFETY: Tauri returns the live NSWindow of this window, which outlives the borrow of
    // `window`; callers are on the main thread.
    Ok(unsafe { &*ptr.cast::<NSWindow>() })
}

/// The app in front, as AppKit last saw it.
pub(crate) fn frontmost() -> Option<AppId> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app_id(&app))
}

/// The app running as `pid` now, if any.
pub(crate) fn running(pid: i32) -> Option<AppId> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid).map(|app| app_id(&app))
}

fn app_id(app: &NSRunningApplication) -> AppId {
    AppId {
        pid: app.processIdentifier(),
        launched: app.launchDate().map(|d| d.timeIntervalSince1970()),
    }
}

/// Brings `app` to the front, yielding our activation to it (macOS 14 activation is
/// cooperative). Returns whether macOS took the request; `false` if its pid now runs another
/// launch.
pub(crate) fn activate(app: &AppId) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        log::error!("apps must be activated on the main thread");
        return false;
    };
    let target = NSRunningApplication::runningApplicationWithProcessIdentifier(app.pid);
    let Some(target) = target.filter(|t| app_id(t) == *app) else {
        return false;
    };
    NSApplication::sharedApplication(mtm).yieldActivationToApplication(&target);
    target.activateFromApplication_options(
        &NSRunningApplication::currentApplication(),
        NSApplicationActivationOptions::empty(),
    ) || target.activateWithOptions(NSApplicationActivationOptions::empty())
}

/// Hides xcreenshot, so macOS gives focus to the app behind it.
pub(crate) fn hide() {
    match MainThreadMarker::new() {
        Some(mtm) => NSApplication::sharedApplication(mtm).hide(None),
        None => log::error!("the app must be hidden on the main thread"),
    }
}
