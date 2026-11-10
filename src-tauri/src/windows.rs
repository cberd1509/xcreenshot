//! Opening the app's regular windows (Settings, onboarding): one per label, created on
//! demand and brought to the front when already open.

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

type Builder<'a, R> = WebviewWindowBuilder<'a, R, AppHandle<R>>;

/// Opens a window, like `open_settings` in `lib.rs`; passed to modules that offer to open it.
pub type Open<R> = fn(&AppHandle<R>) -> tauri::Result<WebviewWindow<R>>;

/// Why Settings opens; logged, so an unexpected open shows up in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Menu,
    SecondLaunch,
    /// macOS reopened the app (Finder, Spotlight).
    #[cfg(target_os = "macos")]
    Reopen {
        visible_windows: bool,
    },
    PrintScreenNotice,
}

/// Opens Settings with `open`, logging `source` and any failure.
pub fn open_from<R: Runtime>(
    app: &AppHandle<R>,
    source: Source,
    open: Open<R>,
) -> tauri::Result<WebviewWindow<R>> {
    log::info!("opening Settings: {source:?}");
    open(app).inspect_err(|e| log::error!("could not open settings: {e}"))
}

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

/// Whether one of the app's regular windows (Settings, a notice) other than `closing` is on
/// screen. Overlay windows and the toasts don't count.
pub fn window_open<R: Runtime>(app: &AppHandle<R>, closing: Option<&str>) -> bool {
    open_windows(app, closing).next().is_some()
}

/// The windows [`window_open`] counts.
pub fn open_windows<'a, R: Runtime>(
    app: &AppHandle<R>,
    closing: Option<&'a str>,
) -> impl Iterator<Item = WebviewWindow<R>> + 'a {
    app.webview_windows()
        .into_iter()
        .filter(move |(label, w)| {
            Some(label.as_str()) != closing
                && crate::overlay::display_id(label).is_none()
                && label != crate::toast::LABEL
                && w.is_visible().unwrap_or(false)
        })
        .map(|(_, w)| w)
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

    #[test]
    fn opening_from_a_source_returns_the_window_or_the_error() {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        let h = app.handle();
        let window = open_from(h, Source::Menu, |app| {
            show_window(app, "settings", WebviewUrl::default(), |b| b)
        });
        assert_eq!(window.unwrap().label(), "settings");
        let failed = open_from(h, Source::Menu, |app| {
            show_window(app, "settings", WebviewUrl::default(), |b| b)?;
            Err(tauri::Error::WindowNotFound)
        });
        assert!(failed.is_err());
    }

    #[test]
    fn only_regular_windows_other_than_the_closing_one_count_as_open() {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        let h = app.handle();
        assert!(!window_open(h, None));
        WebviewWindowBuilder::new(h, "overlay-1", WebviewUrl::default())
            .build()
            .unwrap();
        assert!(!window_open(h, None), "overlays don't count");
        WebviewWindowBuilder::new(h, crate::toast::LABEL, WebviewUrl::default())
            .build()
            .unwrap();
        assert!(!window_open(h, None), "the toasts don't count");
        WebviewWindowBuilder::new(h, "settings", WebviewUrl::default())
            .build()
            .unwrap();
        assert!(window_open(h, None));
        assert!(!window_open(h, Some("settings")));
    }
}
