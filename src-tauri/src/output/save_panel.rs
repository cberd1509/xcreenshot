//! The Save dialog on macOS: an app-modal `NSSavePanel`, centred on the capture's display.
//!
//! The dialog plugin can't be used there: with no parent window it attaches the panel as a sheet
//! to the app's main or first window, which during a capture is a full-screen overlay window,
//! so the panel hangs from the top edge of the screen and can't be moved (#119). Windows keeps
//! the plugin: its dialog has no owner window and is a normal movable dialog.

/// A rectangle in AppKit screen coordinates (origin bottom left of the main display).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where a `width` × `height` panel goes to sit centred on `display`. A panel bigger than the
/// display keeps its top left corner on it, so its title bar can still be dragged.
pub fn centred(display: Frame, width: f64, height: f64) -> (f64, f64) {
    let x = display.x + ((display.width - width) / 2.0).max(0.0);
    let top = display.y + display.height;
    let y = (display.y + (display.height - height) / 2.0).min(top - height);
    (x, y)
}

#[cfg(all(target_os = "macos", not(test)))]
pub use os::ask;

// ---- OS calls only below. ----

#[cfg(all(target_os = "macos", not(test)))]
mod os {
    use super::{Frame, centred};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalPanelWindowLevel, NSModalResponseOK, NSSavePanel};
    use objc2_foundation::{NSArray, NSPoint, NSString, NSURL};
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::ptr::NonNull;
    use tauri::{Runtime, WebviewWindow};

    /// Shows the Save dialog in `dir` with `name` filled in, centred on the display `window`
    /// covers, and waits for it. The dialog runs on the main thread.
    pub fn ask<R: Runtime>(
        window: &WebviewWindow<R>,
        dir: Option<PathBuf>,
        name: &str,
    ) -> Result<Option<PathBuf>, String> {
        let name = name.to_owned();
        crate::macos::on_main(window, move |w, mtm| show(w, mtm, dir.as_deref(), &name))?
    }

    fn show<R: Runtime>(
        window: &WebviewWindow<R>,
        mtm: MainThreadMarker,
        dir: Option<&Path>,
        name: &str,
    ) -> Result<Option<PathBuf>, String> {
        let panel = NSSavePanel::savePanel(mtm);
        let png = NSArray::from_retained_slice(&[NSString::from_str("png")]);
        // Adds `.png` to a typed name, as the plugin did.
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&png));
        panel.setCanCreateDirectories(true);
        panel.setNameFieldStringValue(&NSString::from_str(name));
        // From the path's bytes, so a folder whose name isn't UTF-8 still opens.
        if let Some(dir) = dir.and_then(|d| CString::new(d.as_os_str().as_bytes()).ok()) {
            // SAFETY: a NUL-terminated C string that outlives the call.
            let url = unsafe {
                NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                    NonNull::from(dir.as_c_str()).cast(),
                    true,
                    None,
                )
            };
            panel.setDirectoryURL(Some(&url));
        }
        // A normal modal panel. The overlays are hidden while it is open; above them (1000) it
        // would also be above its own menus (101).
        panel.setLevel(NSModalPanelWindowLevel);
        // The overlay window covers the display the capture is on. Without it the dialog still
        // opens, where AppKit puts it, rather than failing the save.
        match crate::macos::ns_window(window) {
            Ok(overlay) => {
                let display = overlay.frame();
                let size = panel.frame().size;
                let display = Frame {
                    x: display.origin.x,
                    y: display.origin.y,
                    width: display.size.width,
                    height: display.size.height,
                };
                let (x, y) = centred(display, size.width, size.height);
                panel.setFrameOrigin(NSPoint::new(x, y));
            }
            Err(e) => log::warn!("could not centre the Save dialog: {e}"),
        }
        if panel.runModal() != NSModalResponseOK {
            return Ok(None);
        }
        let path = panel.URL().and_then(|url| url.path());
        let path = path.ok_or("the Save dialog returned no path")?;
        Ok(Some(PathBuf::from(path.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: Frame = Frame {
        x: 1512.0,
        y: -200.0,
        width: 1920.0,
        height: 1080.0,
    };

    #[test]
    fn the_panel_is_centred_on_the_display() {
        assert_eq!(centred(DISPLAY, 800.0, 500.0), (2072.0, 90.0));
    }

    #[test]
    fn a_panel_bigger_than_the_display_keeps_its_title_bar_on_it() {
        let (x, y) = centred(DISPLAY, 2000.0, 1200.0);
        assert_eq!(x, DISPLAY.x);
        assert_eq!(
            y + 1200.0,
            DISPLAY.y + DISPLAY.height,
            "top edge on the display"
        );
    }
}
