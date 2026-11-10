//! Double-click a window in the overlay to select it.
//!
//! The on-screen windows are listed when the hotkey is pressed, in parallel with the capture,
//! so they match the frozen frame ([`snapshot`]). A double-click asks for the window under the
//! pointer ([`overlay_window_at`]): the topmost one in the list, clipped to the display. On the
//! desktop, or on a layer that is not a window to select (menu bar, Dock, taskbar), there is
//! none and the page selects the whole display.
//!
//! Everything here is pure except the listing, which is in `macos.rs` and `windows.rs`.
//! Rects are in the displays' OS units (points on macOS, pixels on Windows; see
//! `capture::Display`), the same space as the window bounds the OS reports.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use crate::capture::{Bounds, Display, Rect};
use crate::overlay::display_id;
use crate::region::LogicalRect;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Instant;
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

/// Whether `r` has a size; false for NaN.
fn has_size(r: &Rect) -> bool {
    r.width > 0.0 && r.height > 0.0
}

fn contains(r: &Rect, x: f64, y: f64) -> bool {
    x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height
}

/// An on-screen window, front to back in a list, in OS units. `selectable` is false for a
/// layer that is not a window to select (menu bar, Dock, taskbar, desktop): a double-click on
/// it selects the whole display, and it still hides the windows under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub bounds: Rect,
    pub selectable: bool,
}

/// The macOS window level of the Dock; the menu bar and everything else above windows sit
/// at or above it. Desktop windows are below 0.
const DOCK_LAYER: i32 = 20;

/// A macOS window, from its `CGWindowListCopyWindowInfo` entry: normal, floating and modal
/// windows (layers 0 to 19). `None` for one of ours, an invisible one, one with no size, and
/// the layers above and below windows. Those are left out rather than kept as unselectable:
/// the Dock has a transparent full-screen window above every app, which would hide them all.
/// The menu bar and the Dock cover no window, so a double-click there finds none.
pub fn mac_target(pid: i32, layer: i32, alpha: f64, bounds: Rect) -> Option<Target> {
    let ours = u32::try_from(pid).is_ok_and(|pid| pid == std::process::id());
    let window = (0..DOCK_LAYER).contains(&layer);
    (window && !ours && alpha > 0.0 && has_size(&bounds)).then_some(Target {
        bounds,
        selectable: true,
    })
}

/// Window classes on Windows that are not windows to select: the taskbars and the desktop.
const WINDOWS_SHELL: [&str; 4] = [
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Progman",
    "WorkerW",
];

/// A visible, uncloaked, unminimised window on Windows that is not ours (`EnumWindows` order).
/// `None` for a window that is `passive` (a tool window, such as a floating palette or a tray
/// pop-up, or a click-through overlay) or has no size; the taskbars and the desktop are kept,
/// but not selectable.
pub fn windows_target(class: &str, passive: bool, bounds: Rect) -> Option<Target> {
    let shell = WINDOWS_SHELL.contains(&class);
    if (passive && !shell) || !has_size(&bounds) {
        return None;
    }
    Some(Target {
        bounds,
        selectable: !shell,
    })
}

/// The window a double-click at `point` selects on `display`, whose page is `size` logical
/// px: the topmost one under it, clipped to the display, in the page's logical px. `None` on
/// the desktop or a layer that is not a window to select (the page selects the whole display),
/// or if `size` is not a size.
pub(crate) fn window_at(
    targets: &[Target],
    display: Bounds,
    size: (f64, f64),
    point: (f64, f64),
) -> Option<LogicalRect> {
    if !(size.0.is_finite() && size.1.is_finite() && size.0 > 0.0 && size.1 > 0.0) {
        return None;
    }
    // OS units per logical px: 1 on macOS, the display's scale on Windows.
    let sx = f64::from(display.width) / size.0;
    let sy = f64::from(display.height) / size.1;
    let (left, top) = (f64::from(display.x), f64::from(display.y));
    let (x, y) = (left + point.0 * sx, top + point.1 * sy);
    let hit = targets
        .iter()
        .find(|t| contains(&t.bounds, x, y))
        .filter(|t| t.selectable)?;
    let b = hit.bounds;
    LogicalRect {
        x: (b.x - left) / sx,
        y: (b.y - top) / sy,
        width: b.width / sx,
        height: b.height / sy,
    }
    .clamp_to(size.0, size.1)
}

/// One capture's window list. It is listed on its own thread; asks wait for it.
struct Snapshot {
    session: u64,
    displays: Vec<Display>,
    targets: Arc<OnceLock<Vec<Target>>>,
}

/// The window list of the latest capture, managed by the app.
#[derive(Default)]
pub struct Windows(Mutex<Option<Snapshot>>);

impl Windows {
    /// Lists the windows for `session` with `list` on a new thread, replacing the previous
    /// capture's list. A `list` that panics lists nothing.
    pub fn begin(
        &self,
        session: u64,
        displays: &[Display],
        list: impl FnOnce() -> Vec<Target> + Send + 'static,
    ) {
        let targets = Arc::new(OnceLock::new());
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Snapshot {
            session,
            displays: displays.to_vec(),
            targets: Arc::clone(&targets),
        });
        std::thread::spawn(move || {
            let started = Instant::now();
            let listed = panic::catch_unwind(AssertUnwindSafe(list)).unwrap_or_default();
            log::info!(
                "listed {} windows in {:.1} ms",
                listed.len(),
                started.elapsed().as_secs_f64() * 1000.0
            );
            let _ = targets.set(listed);
        });
    }

    /// The bounds of `display` in `session` and that capture's list, which may still be in
    /// the making. `None` for another session or display.
    fn find(&self, session: u64, display: u32) -> Option<(Bounds, Arc<OnceLock<Vec<Target>>>)> {
        let snapshot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let s = snapshot.as_ref().filter(|s| s.session == session)?;
        let d = s.displays.iter().find(|d| d.id == display)?;
        Some((d.bounds, Arc::clone(&s.targets)))
    }
}

/// The on-screen windows, front to back, ours left out.
pub fn list() -> Vec<Target> {
    #[cfg(target_os = "macos")]
    return macos::list();
    #[cfg(target_os = "windows")]
    return windows::list();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Vec::new()
}

/// Lists the windows for a capture that just started on `displays`, while it runs.
pub(crate) fn snapshot<R: Runtime>(app: &AppHandle<R>, session: u64, displays: &[Display]) {
    if let Some(windows) = app.try_state::<Windows>() {
        windows.begin(session, displays, list);
    }
}

/// The window a double-click at (`x`, `y`) selects on the calling overlay's display, in its
/// logical px; the page is `width` x `height` of them. `None`: the whole display. A list still
/// in the making is waited for off the async runtime's workers.
#[tauri::command]
pub(crate) async fn overlay_window_at<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Windows>,
    session: u64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<Option<LogicalRect>, ()> {
    let Some((bounds, targets)) = display_id(window.label()).and_then(|d| state.find(session, d))
    else {
        return Ok(None);
    };
    let pick = move |targets: &[Target]| window_at(targets, bounds, (width, height), (x, y));
    if let Some(listed) = targets.get() {
        return Ok(pick(listed));
    }
    tauri::async_runtime::spawn_blocking(move || pick(targets.wait()))
        .await
        .map_err(|e| log::error!("could not wait for the window list: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    fn area(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
        LogicalRect {
            x,
            y,
            width,
            height,
        }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn window(x: f64, y: f64, width: f64, height: f64) -> Target {
        Target {
            bounds: rect(x, y, width, height),
            selectable: true,
        }
    }

    fn shell(x: f64, y: f64, width: f64, height: f64) -> Target {
        Target {
            selectable: false,
            ..window(x, y, width, height)
        }
    }

    const MAIN: Bounds = Bounds {
        x: 0,
        y: 0,
        width: 1440,
        height: 900,
    };
    const PAGE: (f64, f64) = (1440.0, 900.0);

    #[test]
    fn picks_the_topmost_window_under_the_point() {
        let targets = [
            window(100.0, 100.0, 300.0, 200.0),
            window(0.0, 0.0, 800.0, 600.0),
        ];
        assert_eq!(
            window_at(&targets, MAIN, PAGE, (150.0, 150.0)),
            Some(area(100.0, 100.0, 300.0, 200.0)),
            "the front window"
        );
        assert_eq!(
            window_at(&targets, MAIN, PAGE, (500.0, 500.0)),
            Some(area(0.0, 0.0, 800.0, 600.0)),
            "the one behind, where the front one is not"
        );
        // The right and bottom edges are outside.
        assert_eq!(
            window_at(&targets, MAIN, PAGE, (400.0, 150.0)),
            Some(area(0.0, 0.0, 800.0, 600.0))
        );
    }

    #[test]
    fn the_desktop_and_excluded_layers_have_no_window() {
        let targets = [
            shell(0.0, 0.0, 1440.0, 25.0),
            window(0.0, 0.0, 800.0, 600.0),
        ];
        assert_eq!(
            window_at(&targets, MAIN, PAGE, (10.0, 10.0)),
            None,
            "menu bar over a window"
        );
        assert_eq!(
            window_at(&targets, MAIN, PAGE, (1000.0, 700.0)),
            None,
            "desktop"
        );
        assert_eq!(window_at(&[], MAIN, PAGE, (10.0, 10.0)), None, "no windows");
    }

    #[test]
    fn a_window_is_clipped_to_the_display() {
        let targets = [window(-50.0, 800.0, 400.0, 300.0)];
        assert_eq!(
            window_at(&targets, MAIN, PAGE, (10.0, 850.0)),
            Some(area(0.0, 800.0, 350.0, 100.0))
        );
    }

    #[test]
    fn other_displays_are_offset_and_clip_their_part() {
        // A display to the left of the main one, and a window across both.
        let left = Bounds {
            x: -1920,
            y: -200,
            width: 1920,
            height: 1080,
        };
        let targets = [window(-500.0, 0.0, 1000.0, 400.0)];
        assert_eq!(
            window_at(&targets, left, (1920.0, 1080.0), (1600.0, 300.0)),
            Some(area(1420.0, 200.0, 500.0, 400.0))
        );
        assert_eq!(
            window_at(&targets, MAIN, PAGE, (10.0, 10.0)),
            Some(area(0.0, 0.0, 500.0, 400.0))
        );
    }

    #[test]
    fn pixels_convert_to_the_page_logical_px() {
        // Windows at 150%: the display is 2880 px wide, the page 1920 logical px.
        let display = Bounds {
            x: 2880,
            y: 0,
            width: 2880,
            height: 1620,
        };
        let targets = [window(3030.0, 150.0, 600.0, 300.0)];
        assert_eq!(
            window_at(&targets, display, (1920.0, 1080.0), (150.0, 150.0)),
            Some(area(100.0, 100.0, 400.0, 200.0))
        );
    }

    #[test]
    fn no_size_no_answer() {
        for size in [
            (0.0, 900.0),
            (1440.0, -1.0),
            (f64::NAN, 900.0),
            (f64::INFINITY, 900.0),
        ] {
            assert_eq!(window_at(&[], MAIN, size, (1.0, 1.0)), None, "{size:?}");
        }
    }

    #[test]
    fn mac_keeps_others_visible_windows_and_leaves_out_high_and_desktop_layers() {
        let b = rect(0.0, 0.0, 10.0, 10.0);
        let selectable = |layer| mac_target(1, layer, 1.0, b).map(|t| t.selectable);
        assert_eq!(selectable(0), Some(true), "normal");
        assert_eq!(selectable(3), Some(true), "floating");
        assert_eq!(selectable(8), Some(true), "modal panel");
        // The Dock's full-screen window must not hide the windows under it.
        assert_eq!(selectable(20), None, "Dock");
        assert_eq!(selectable(24), None, "menu bar");
        assert_eq!(selectable(-2_147_483_628), None, "desktop");
        let ours = i32::try_from(std::process::id()).unwrap();
        assert_eq!(mac_target(ours, 0, 1.0, b), None, "ours");
        assert_eq!(mac_target(1, 0, 0.0, b), None, "invisible");
        assert_eq!(
            mac_target(1, 0, 1.0, rect(0.0, 0.0, 0.0, 10.0)),
            None,
            "no size"
        );
    }

    #[test]
    fn windows_skips_passive_windows_and_excludes_the_shell() {
        let b = rect(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            windows_target("Notepad", false, b).map(|t| t.selectable),
            Some(true)
        );
        assert_eq!(
            windows_target("Palette", true, b),
            None,
            "tool or click-through"
        );
        for class in WINDOWS_SHELL {
            for tool in [false, true] {
                let t = windows_target(class, tool, b);
                assert_eq!(t.map(|t| t.selectable), Some(false), "{class}");
            }
        }
        assert_eq!(
            windows_target("Notepad", false, rect(0.0, 0.0, 10.0, 0.0)),
            None
        );
    }

    fn display(id: u32) -> Display {
        Display { id, bounds: MAIN }
    }

    #[test]
    fn listed_windows_have_a_size() {
        assert!(list().iter().all(|t| has_size(&t.bounds)));
    }

    type Mock = tauri::test::MockRuntime;

    fn app() -> tauri::App<Mock> {
        mock_builder()
            .manage(Windows::default())
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    fn page(app: &tauri::App<Mock>, label: &str) -> WebviewWindow<Mock> {
        tauri::WebviewWindowBuilder::new(app.handle(), label, Default::default())
            .build()
            .unwrap()
    }

    /// The calling page's answer for a double-click at (1, 1).
    fn ask(handle: &AppHandle<Mock>, w: WebviewWindow<Mock>, session: u64) -> Option<LogicalRect> {
        let state = handle.state::<Windows>();
        tauri::async_runtime::block_on(overlay_window_at(
            w, state, session, 1.0, 1.0, 1440.0, 900.0,
        ))
        .unwrap()
    }

    #[test]
    fn a_double_click_waits_for_its_session_s_list() {
        let app = app();
        let handle = app.handle().clone();
        let overlay = page(&app, "overlay-1");
        assert_eq!(ask(&handle, overlay.clone(), 2), None, "nothing listed");
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        handle.state::<Windows>().begin(2, &[display(1)], move || {
            rx.recv().unwrap();
            vec![window(0.0, 0.0, 10.0, 10.0)]
        });
        assert_eq!(ask(&handle, overlay.clone(), 1), None, "older session");
        assert_eq!(
            ask(&handle, page(&app, "overlay-9"), 2),
            None,
            "unknown display"
        );
        assert_eq!(
            ask(&handle, page(&app, "settings"), 2),
            None,
            "not an overlay"
        );
        // Answered once the list is in, and at once after that.
        let asked = std::thread::spawn({
            let (handle, overlay) = (handle.clone(), overlay.clone());
            move || ask(&handle, overlay, 2)
        });
        tx.send(()).unwrap();
        let picked = Some(area(0.0, 0.0, 10.0, 10.0));
        assert_eq!(asked.join().unwrap(), picked);
        assert_eq!(ask(&handle, overlay, 2), picked);
    }

    #[test]
    fn a_failed_list_finds_no_window() {
        let app = app();
        app.state::<Windows>()
            .begin(1, &[display(1)], || panic!("no windows"));
        assert_eq!(ask(app.handle(), page(&app, "overlay-1"), 1), None);
    }

    #[test]
    fn a_capture_lists_the_windows_if_the_app_keeps_them() {
        let app = app();
        snapshot(app.handle(), 3, &[display(1)]);
        assert!(app.state::<Windows>().find(3, 1).is_some());
        // Without the state, a capture lists nothing and never fails.
        let bare = mock_builder().build(mock_context(noop_assets())).unwrap();
        snapshot(bare.handle(), 1, &[display(1)]);
    }
}
