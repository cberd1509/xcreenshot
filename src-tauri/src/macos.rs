//! Shared AppKit helpers.

use crate::focus::AppId;
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSScreenSaverWindowLevel,
    NSWindow, NSWorkspace, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::{NSNotification, NSOperationQueue};
use std::ptr::NonNull;
use tauri::{Runtime, WebviewWindow};

/// The overlay's window level: above the menu bar, the Dock and pop-up menus.
pub(crate) const SCREEN_SAVER_LEVEL: isize = NSScreenSaverWindowLevel;

/// The window's `NSWindow`. Only use it on the main thread.
pub(crate) fn ns_window<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<&NSWindow> {
    let ptr = window.ns_window()?;
    // SAFETY: Tauri returns the live NSWindow of this window, which outlives the borrow of
    // `window`; callers are on the main thread.
    Ok(unsafe { &*ptr.cast::<NSWindow>() })
}

/// Runs `f` on the main thread and waits for it, or runs it directly when already there.
pub(crate) fn on_main<R: Runtime, T: Send + 'static>(
    window: &WebviewWindow<R>,
    f: impl FnOnce(&WebviewWindow<R>, MainThreadMarker) -> T + Send + 'static,
) -> Result<T, String> {
    if let Some(mtm) = MainThreadMarker::new() {
        return Ok(f(window, mtm));
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let mtm = MainThreadMarker::new().expect("on the main thread");
            let _ = tx.send(f(&target, mtm));
        })
        .map_err(|e| e.to_string())?;
    rx.recv()
        .map_err(|_| "the main thread did not answer".to_owned())
}

/// The window's `NSWindow`, if this is the main thread; logs why not otherwise.
pub(crate) fn main_ns_window<R: Runtime>(window: &WebviewWindow<R>) -> Option<&NSWindow> {
    if MainThreadMarker::new().is_none() {
        log::error!("{} must be changed on the main thread", window.label());
        return None;
    }
    ns_window(window)
        .inspect_err(|e| log::error!("no NSWindow for {}: {e}", window.label()))
        .ok()
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
        window: None,
    }
}

/// Calls `f` with each app that comes to the front, on the main thread, for as long as the app
/// runs.
pub(crate) fn on_activate(f: impl Fn(AppId) + Send + Sync + 'static) {
    let block = RcBlock::new(move |note: NonNull<NSNotification>| {
        // SAFETY: AppKit passes a notification that lives for the call.
        let note = unsafe { note.as_ref() };
        // SAFETY: an AppKit constant.
        let key = unsafe { NSWorkspaceApplicationKey };
        let app = note
            .userInfo()
            .and_then(|info| info.objectForKey(key))
            .and_then(|app| app.downcast::<NSRunningApplication>().ok());
        if let Some(app) = app {
            f(app_id(&app));
        }
    });
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the main queue runs the block on the main thread; `f` is Send and Sync anyway.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidActivateApplicationNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &block,
        )
    };
    // Never removed: it follows the apps until xcreenshot quits.
    std::mem::forget(observer);
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
