//! Opening the app's regular windows (Settings, permission steps): one per label, created on
//! demand and brought to the front when already open.

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

type Builder<'a, R> = WebviewWindowBuilder<'a, R, AppHandle<R>>;

/// Opens a window, like `open_settings` in `lib.rs`; passed to modules that offer to open it.
pub type Open<R> = fn(&AppHandle<R>) -> tauri::Result<WebviewWindow<R>>;

/// Shows the window `label`, building it from `url` and `configure` if it isn't open yet.
pub fn show_window<'a, R: Runtime>(
    app: &'a AppHandle<R>,
    label: &str,
    url: WebviewUrl,
    configure: impl FnOnce(Builder<'a, R>) -> Builder<'a, R>,
) -> tauri::Result<WebviewWindow<R>> {
    let window = match app.get_webview_window(label) {
        Some(w) => w,
        None => configure(WebviewWindowBuilder::new(app, label, url)).build()?,
    };
    window.unminimize()?;
    window.show()?;
    window.set_focus()?;
    Ok(window)
}

/// Gives focus back to the app the user was in (macOS: hides this menu bar app), unless one of
/// its regular windows (Settings, a notice) other than `closing` is on screen. Elsewhere the
/// system does this when a window hides or closes.
pub fn yield_focus<R: Runtime>(app: &AppHandle<R>, closing: Option<&str>) {
    #[cfg(target_os = "macos")]
    if !app.webview_windows().iter().any(|(label, w)| {
        Some(label.as_str()) != closing
            && crate::overlay::display_id(label).is_none()
            && w.is_visible().unwrap_or(false)
    }) {
        let _ = app.hide();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, closing);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn builds_once_then_reuses_the_window() {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        let handle = app.handle();
        let mut built = 0;
        for _ in 0..2 {
            show_window(handle, "w", WebviewUrl::default(), |b| {
                built += 1;
                b.title("W")
            })
            .unwrap();
        }
        assert_eq!(built, 1);
        assert!(handle.get_webview_window("w").is_some());
        assert_eq!(handle.webview_windows().len(), 1);
    }
}
