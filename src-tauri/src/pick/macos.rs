//! The on-screen windows on macOS, from `CGWindowListCopyWindowInfo`, and each screen's
//! visible frame. Bounds need no permission; they are in points, from the top-left of the main
//! display, as display bounds.

use super::{Target, mac_target, work_area};
use crate::capture::{Display, Rect};
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_foundation::{
    CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGRect,
};
use objc2_core_graphics::{
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
    kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowOwnerPID,
};
use objc2_foundation::{NSNumber, NSString};

type Info = CFDictionary<CFString, CFType>;

/// Front to back; the desktop's own windows (wallpaper, icons) are left out by the OS.
pub(super) fn list() -> Vec<Target> {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(windows) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        log::warn!("could not list the windows: a double-click selects the whole display");
        return Vec::new();
    };
    // SAFETY: the list is an array of window info dictionaries with string keys.
    let windows = unsafe { CFRetained::cast_unchecked::<CFArray<Info>>(windows) };
    windows.iter().filter_map(|info| target(&info)).collect()
}

fn rect(r: CGRect) -> Rect {
    Rect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}

fn target(info: &Info) -> Option<Target> {
    // SAFETY: the keys are the CGWindow constants, which live for the whole process.
    let (pid, layer, alpha, bounds) = unsafe {
        (
            kCGWindowOwnerPID,
            kCGWindowLayer,
            kCGWindowAlpha,
            kCGWindowBounds,
        )
    };
    let number = |key: &CFString| info.get(key)?.downcast::<CFNumber>().ok();
    let bounds = info.get(bounds)?.downcast::<CFDictionary>().ok()?;
    let mut bounds_rect = CGRect::default();
    // SAFETY: `bounds` is the window's bounds dictionary and `bounds_rect` a valid pointer.
    if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(&bounds), &mut bounds_rect) } {
        return None;
    }
    mac_target(
        number(pid)?.as_i32()?,
        number(layer)?.as_i32()?,
        number(alpha).and_then(|a| a.as_f64()).unwrap_or(1.0),
        rect(bounds_rect),
    )
}

/// Each display's work area, from its screen's visible frame (without the menu bar and the
/// Dock). AppKit answers on the main thread only; elsewhere, none.
pub(super) fn work_areas(displays: &[Display]) -> Vec<Option<Rect>> {
    let Some(mtm) = MainThreadMarker::new() else {
        return vec![None; displays.len()];
    };
    let screens = NSScreen::screens(mtm);
    let key = NSString::from_str("NSScreenNumber");
    let id = |screen: &NSScreen| {
        let number = screen.deviceDescription().objectForKey(&key)?;
        Some(number.downcast::<NSNumber>().ok()?.unsignedIntValue())
    };
    displays
        .iter()
        .map(|d| {
            let screen = screens.iter().find(|s| id(s) == Some(d.id))?;
            Some(work_area(
                d.bounds,
                rect(screen.frame()),
                rect(screen.visibleFrame()),
                true,
            ))
        })
        .collect()
}
