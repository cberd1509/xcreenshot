//! The on-screen windows on macOS, from `CGWindowListCopyWindowInfo`. Bounds need no
//! permission; they are in points, from the top-left of the main display, as display bounds.

use super::{Target, mac_target};
use crate::capture::Rect;
use objc2_core_foundation::{
    CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGRect,
};
use objc2_core_graphics::{
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
    kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowOwnerPID,
};

type Info = CFDictionary<CFString, CFType>;

/// Front to back; the desktop's own windows (wallpaper, icons) are left out by the OS.
pub(super) fn list() -> Vec<Target> {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(windows) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        return Vec::new();
    };
    // SAFETY: the list is an array of window info dictionaries with string keys.
    let windows = unsafe { CFRetained::cast_unchecked::<CFArray<Info>>(windows) };
    windows.iter().filter_map(|info| target(&info)).collect()
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
    let mut rect = CGRect::default();
    // SAFETY: `bounds` is the window's bounds dictionary and `rect` a valid pointer.
    if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(&bounds), &mut rect) } {
        return None;
    }
    mac_target(
        number(pid)?.as_i32()?,
        number(layer)?.as_i32()?,
        number(alpha).and_then(|a| a.as_f64()).unwrap_or(1.0),
        Rect {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        },
    )
}
