//! Native share sheet. macOS shows `NSSharingServicePicker`; Windows (#30) is not done yet and
//! returns [`ShareError::Unsupported`]. See `docs/share.md` for what callers must do.
//!
//! The PNG goes to a temp file in the app's cache dir (`share/`), which is deleted when the
//! user cancels or the chosen service reports back, and swept at the next startup otherwise.

#[cfg(debug_assertions)]
pub mod debug;
#[cfg(all(target_os = "macos", not(test)))]
mod macos;
mod temp;

use temp::TempFiles;

use std::fmt;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

/// A rectangle in logical pixels, relative to the window's content area (origin top left,
/// like CSS pixels in the page).
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    /// The same rect in AppKit window coordinates (origin bottom left), given the area the
    /// page fills (`contentLayoutRect`, below the title bar) in those coordinates.
    pub fn in_window(self, content: Rect) -> Self {
        Self {
            x: content.x + self.x,
            y: content.y + content.height - self.y - self.height,
            ..self
        }
    }
}

#[derive(Debug)]
pub enum ShareError {
    /// No share sheet on this platform yet.
    Unsupported,
    /// [`init`] was not called at startup.
    NotReady,
    /// The temp file could not be written.
    Io(std::io::Error),
    /// The window has no native view, or the main thread is gone.
    Window(String),
}

impl ShareError {
    fn kind(&self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::NotReady => "notReady",
            Self::Io(_) => "io",
            Self::Window(_) => "window",
        }
    }
}

impl fmt::Display for ShareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("sharing is not supported on this platform yet"),
            Self::NotReady => f.write_str("sharing was not set up"),
            Self::Io(e) => write!(f, "could not write the file to share: {e}"),
            Self::Window(e) => write!(f, "could not show the share sheet: {e}"),
        }
    }
}

impl std::error::Error for ShareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ShareError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Serializes as `{ "kind": "unsupported", "message": "..." }`, like `CaptureError`.
impl serde::Serialize for ShareError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("ShareError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

/// How sharing ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(all(target_os = "macos", not(test))), allow(dead_code))]
enum Ended {
    /// The sheet was dismissed without a choice.
    Cancelled,
    /// The chosen service reported success or failure.
    Finished,
}

/// Called once when sharing ends, on the main thread.
type Done = Box<dyn FnOnce(Ended) + Send + 'static>;

/// After a service reports back, the file is kept this long: some read it lazily.
#[cfg_attr(not(all(target_os = "macos", not(test))), allow(dead_code))]
const GRACE: Duration = Duration::from_secs(30);

/// Startup: manages the temp file tracker and, off the startup path, deletes files a previous
/// run left behind.
pub fn init<R: Runtime>(app: &AppHandle<R>) {
    let dir = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("com.bervex.xcreenshot"))
        .join("share");
    manage(app, dir);
}

fn manage<R: Runtime>(app: &AppHandle<R>, dir: PathBuf) -> JoinHandle<()> {
    app.manage(TempFiles::new(dir));
    let app = app.clone();
    std::thread::spawn(move || {
        let swept = app.state::<TempFiles>().sweep();
        if swept > 0 {
            log::info!("deleted {swept} leftover share file(s)");
        }
    })
}

/// Shows the share sheet for `png`, anchored to `anchor` in `window`. Returns once the sheet
/// is open; the temp file is deleted when sharing ends. Callable from any thread.
///
/// A window above the pop-up menu level (the overlay, at the screen saver level) would hide
/// the sheet's menu, so it is lowered just below that level until the menu closes.
pub fn share_png<R: Runtime>(
    window: &WebviewWindow<R>,
    png: &[u8],
    anchor: Rect,
) -> Result<(), ShareError> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let now = chrono::Local::now().naive_local();
        share_with(window, png, anchor, now, GRACE, macos::show)
    }
    #[cfg(not(all(target_os = "macos", not(test))))]
    {
        let _ = (window, png, anchor);
        Err(ShareError::Unsupported)
    }
}

/// Writes the temp file and hands it to `show`, deleting it again if `show` fails.
#[cfg_attr(not(all(target_os = "macos", not(test))), allow(dead_code))]
fn share_with<R: Runtime>(
    window: &WebviewWindow<R>,
    png: &[u8],
    anchor: Rect,
    now: chrono::NaiveDateTime,
    grace: Duration,
    show: impl FnOnce(&WebviewWindow<R>, Rect, &Path, Done) -> Result<(), ShareError>,
) -> Result<(), ShareError> {
    let app = window.app_handle();
    let files = app.try_state::<TempFiles>().ok_or(ShareError::NotReady)?;
    let path = files.create(png, now)?;
    let handle = app.clone();
    let shared = path.clone();
    let done: Done = Box::new(move |ended| {
        let release = move || handle.state::<TempFiles>().release(&shared);
        match ended {
            Ended::Cancelled => release(),
            Ended::Finished => {
                std::thread::spawn(move || {
                    std::thread::sleep(grace);
                    release();
                });
            }
        }
    });
    show(window, anchor, &path, done).inspect_err(|e| {
        log::error!("{e}");
        files.release(&path);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::sync::mpsc;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
    use tauri::{App, WebviewUrl, WebviewWindowBuilder};

    fn app() -> App<MockRuntime> {
        mock_builder().build(mock_context(noop_assets())).unwrap()
    }

    fn window(app: &App<MockRuntime>) -> WebviewWindow<MockRuntime> {
        WebviewWindowBuilder::new(app, "w", WebviewUrl::default())
            .build()
            .unwrap()
    }

    /// Its own directory per test: they run in parallel and share the clock.
    fn setup(app: &App<MockRuntime>, name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xcreenshot-share-mod-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("xcreenshot-leftover.png"), b"old").unwrap();
        manage(app.handle(), dir.clone()).join().unwrap();
        assert!(!dir.join("xcreenshot-leftover.png").exists(), "swept");
        dir
    }

    fn now() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 6)
            .unwrap()
            .and_hms_opt(13, 22, 5)
            .unwrap()
    }

    const ANCHOR: Rect = Rect {
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 40.0,
    };

    #[test]
    fn rect_moves_into_window_coordinates() {
        let content = Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
        };
        assert_eq!(ANCHOR.in_window(content), Rect { y: 40.0, ..ANCHOR });
        // A content area inset from the window's edges shifts the rect with it.
        let inset = Rect {
            x: 5.0,
            y: 7.0,
            ..content
        };
        assert_eq!(
            ANCHOR.in_window(inset),
            Rect {
                x: 15.0,
                y: 47.0,
                ..ANCHOR
            }
        );
        let r: Rect = serde_json::from_str(r#"{"x":1,"y":2.5,"width":3,"height":4}"#).unwrap();
        assert_eq!(r.y, 2.5);
    }

    #[test]
    fn errors_map_to_kind_and_message() {
        let io = std::io::Error::other("disk full");
        let cases = [
            (ShareError::Unsupported, "unsupported", "not supported"),
            (ShareError::NotReady, "notReady", "not set up"),
            (ShareError::from(io), "io", "disk full"),
            (ShareError::Window("no view".into()), "window", "no view"),
        ];
        for (e, kind, text) in cases {
            let json = serde_json::to_value(&e).unwrap();
            assert_eq!(json["kind"], kind);
            assert!(json["message"].as_str().unwrap().contains(text), "{e}");
            assert_eq!(
                std::error::Error::source(&e).is_some(),
                kind == "io",
                "{kind}"
            );
        }
    }

    #[test]
    fn share_png_is_unsupported_without_a_native_sheet() {
        let app = app();
        assert!(matches!(
            share_png(&window(&app), b"png", ANCHOR),
            Err(ShareError::Unsupported)
        ));
    }

    #[test]
    fn share_needs_init() {
        let app = app();
        let w = window(&app);
        let result = share_with(&w, b"png", ANCHOR, now(), Duration::ZERO, |_, _, _, _| {
            Ok(())
        });
        assert!(matches!(result, Err(ShareError::NotReady)));
    }

    /// Shares one file and returns its path and the callback that ends sharing.
    fn shared(app: &App<MockRuntime>, grace: Duration) -> (PathBuf, Done) {
        let (tx, rx) = mpsc::channel();
        share_with(
            &window(app),
            b"png",
            ANCHOR,
            now(),
            grace,
            |_, anchor, path, done| {
                assert_eq!(anchor, ANCHOR);
                tx.send((path.to_path_buf(), done)).unwrap();
                Ok(())
            },
        )
        .unwrap();
        let (path, done) = rx.recv().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"png");
        (path, done)
    }

    #[test]
    fn cancelling_deletes_the_file_at_once() {
        let app = app();
        let dir = setup(&app, "cancel");
        let (path, done) = shared(&app, Duration::from_secs(60));
        assert_eq!(path, dir.join("xcreenshot-2026-11-06-13.22.05.png"));
        done(Ended::Cancelled);
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_finished_share_deletes_the_file_after_the_grace_period() {
        let app = app();
        let dir = setup(&app, "finish");
        let (path, done) = shared(&app, Duration::from_millis(50));
        done(Ended::Finished);
        assert!(path.exists(), "kept for services that read it late");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_show_deletes_the_file() {
        let app = app();
        let dir = setup(&app, "failed");
        let w = window(&app);
        let (tx, rx) = mpsc::channel();
        let result = share_with(
            &w,
            b"png",
            ANCHOR,
            now(),
            Duration::ZERO,
            |_, _, path, _| {
                tx.send(path.to_path_buf()).unwrap();
                Err(ShareError::Window("no view".into()))
            },
        );
        assert!(matches!(result, Err(ShareError::Window(_))));
        assert!(!rx.recv().unwrap().exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
