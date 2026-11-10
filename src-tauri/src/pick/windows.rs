//! The on-screen windows on Windows, from `EnumWindows` (top to bottom) and their DWM frames
//! (without the invisible resize borders). The app is Per-Monitor DPI aware, so the frames are
//! in physical pixels, as display bounds.

use super::{Target, windows_target};
use crate::capture::Rect;
use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWINDOWATTRIBUTE, DwmGetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetWindowLongW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

pub(super) fn list() -> Vec<Target> {
    let mut targets: Vec<Target> = Vec::new();
    // SAFETY: `each` only runs during the call, while `targets` is borrowed.
    let listed = unsafe { EnumWindows(Some(each), LPARAM(&raw mut targets as isize)) };
    if let Err(e) = listed {
        log::warn!("could not list the windows: {e}");
    }
    targets
}

unsafe extern "system" fn each(hwnd: HWND, targets: LPARAM) -> BOOL {
    // SAFETY: `list` passes its `Vec<Target>`, borrowed for the whole enumeration.
    let targets = unsafe { &mut *(targets.0 as *mut Vec<Target>) };
    if let Some(t) = target(hwnd) {
        targets.push(t);
    }
    true.into()
}

/// Reads one DWM attribute of `hwnd` into a `T`.
fn attribute<T: Default>(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE) -> Option<T> {
    let mut value = T::default();
    let size = u32::try_from(size_of::<T>()).ok()?;
    // SAFETY: `value` is a `T` of `size` bytes, as the attribute expects.
    unsafe { DwmGetWindowAttribute(hwnd, attribute, (&raw mut value).cast::<c_void>(), size) }
        .ok()?;
    Some(value)
}

fn target(hwnd: HWND) -> Option<Target> {
    // SAFETY: `hwnd` comes from `EnumWindows`; these calls only read it.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
        if pid == std::process::id() || attribute::<u32>(hwnd, DWMWA_CLOAKED).unwrap_or(0) != 0 {
            return None;
        }
        let r: RECT = attribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS)?;
        let mut class = [0u16; 64];
        let len = usize::try_from(GetClassNameW(hwnd, &mut class)).unwrap_or(0);
        // Tool windows (palettes, tray pop-ups) and click-through overlays.
        let passive = WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0;
        let passive = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & passive != 0;
        windows_target(
            &String::from_utf16_lossy(&class[..len]),
            passive,
            Rect {
                x: f64::from(r.left),
                y: f64::from(r.top),
                width: f64::from(r.right - r.left),
                height: f64::from(r.bottom - r.top),
            },
        )
    }
}
