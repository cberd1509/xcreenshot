//! Dev builds only: `XCREENSHOT_DEBUG_SHARE` opens Settings after startup and shares a small
//! test PNG from it, so the share sheet can be checked by hand before the overlay calls it.
//!
//! - `1`: Settings at its normal level.
//! - `overlay`: Settings raised to the overlay's level (screen saver) first.
//! - `cancel`: like `overlay`, then dismisses the sheet after 2 s, which deletes the temp file
//!   and puts Settings back at the screen saver level.

use super::Rect;
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Runtime, WebviewWindow};

const DELAY: Duration = Duration::from_secs(1);
/// Top left of the Settings page, about where a toolbar button would be.
const ANCHOR: Rect = Rect {
    x: 24.0,
    y: 24.0,
    width: 96.0,
    height: 32.0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Overlay,
    Cancel,
}

fn mode(value: Option<String>) -> Option<Mode> {
    match value?.as_str() {
        "1" => Some(Mode::Normal),
        "overlay" => Some(Mode::Overlay),
        "cancel" => Some(Mode::Cancel),
        _ => None,
    }
}

type Open<R> = fn(&AppHandle<R>) -> tauri::Result<WebviewWindow<R>>;

pub fn schedule<R: Runtime>(app: &AppHandle<R>, open: Open<R>) {
    if let Some(mode) = mode(std::env::var("XCREENSHOT_DEBUG_SHARE").ok()) {
        later(app, DELAY, open, mode);
    }
}

fn later<R: Runtime>(
    app: &AppHandle<R>,
    delay: Duration,
    open: Open<R>,
    mode: Mode,
) -> JoinHandle<()> {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || match open(&handle) {
            Ok(window) => share(&window, mode),
            Err(e) => log::error!("debug share: could not open settings: {e}"),
        });
        if mode == Mode::Cancel {
            // The sheet has been open for `2 × delay`.
            std::thread::sleep(delay * 2);
            let _ = app.run_on_main_thread(close_sheets);
        }
    })
}

fn share<R: Runtime>(window: &WebviewWindow<R>, mode: Mode) {
    #[cfg(all(target_os = "macos", not(test)))]
    if mode != Mode::Normal {
        match crate::macos::ns_window(window) {
            Ok(w) => w.setLevel(crate::macos::SCREEN_SAVER_LEVEL),
            Err(e) => log::error!("debug share: {e}"),
        }
    }
    let started = std::time::Instant::now();
    let result = super::share_png(window, &test_png(), ANCHOR, |outcome| {
        log::info!("debug share: {outcome:?}");
    });
    log::info!(
        "debug share ({mode:?}): {result:?}, returned after {:?}",
        started.elapsed()
    );
}

fn close_sheets() {
    #[cfg(all(target_os = "macos", not(test)))]
    super::macos::close_all();
    log::info!("debug share: dismissed the share sheet");
}

/// A 64×64 PNG in the app's accent colors.
fn test_png() -> Vec<u8> {
    let image = image::RgbaImage::from_fn(64, 64, |x, y| {
        if (x / 8 + y / 8) % 2 == 0 {
            image::Rgba([0x4f, 0x46, 0xe5, 0xff])
        } else {
            image::Rgba([0xff, 0xff, 0xff, 0xff])
        }
    });
    crate::capture::encode_png(&image).expect("encoding to memory cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    #[test]
    fn mode_comes_from_the_variable() {
        assert_eq!(mode(Some("1".into())), Some(Mode::Normal));
        assert_eq!(mode(Some("overlay".into())), Some(Mode::Overlay));
        assert_eq!(mode(Some("cancel".into())), Some(Mode::Cancel));
        assert_eq!(mode(Some("yes".into())), None);
        assert_eq!(mode(None), None);
    }

    #[test]
    fn test_png_is_a_png() {
        let png = test_png();
        let image = image::load_from_memory(&png).unwrap();
        assert_eq!((image.width(), image.height()), (64, 64));
    }

    fn open(app: &AppHandle<MockRuntime>) -> tauri::Result<WebviewWindow<MockRuntime>> {
        match app.get_webview_window("settings") {
            Some(w) => Ok(w),
            None => WebviewWindowBuilder::new(app, "settings", WebviewUrl::default()).build(),
        }
    }

    fn fail(_: &AppHandle<MockRuntime>) -> tauri::Result<WebviewWindow<MockRuntime>> {
        Err(tauri::Error::WindowNotFound)
    }

    #[test]
    fn opens_settings_and_shares_in_every_mode() {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        // Without XCREENSHOT_DEBUG_SHARE, nothing is scheduled.
        schedule(app.handle(), open);
        for mode in [Mode::Normal, Mode::Overlay, Mode::Cancel] {
            later(app.handle(), Duration::ZERO, open, mode)
                .join()
                .unwrap();
        }
        later(app.handle(), Duration::ZERO, fail, Mode::Normal)
            .join()
            .unwrap();
        assert!(app.get_webview_window("settings").is_some());
    }
}
