//! Opening the app's regular windows (Settings, permission steps): one per label, created on
//! demand and brought to the front when already open.

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

type Builder<'a, R> = WebviewWindowBuilder<'a, R, AppHandle<R>>;

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
