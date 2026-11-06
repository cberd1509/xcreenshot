//! Shared AppKit helpers. `overlay/window.rs` (`macos::raise`) still has its own copy of the
//! pointer cast and the level; it should switch to these.

use objc2_app_kit::{NSScreenSaverWindowLevel, NSWindow};
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
