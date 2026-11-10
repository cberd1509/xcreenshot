//! Double-click a window in the overlay to select it.
//!
//! The on-screen windows are listed when the hotkey is pressed, in parallel with the capture,
//! so they match the frozen frame ([`snapshot`]). A double-click asks for the window under the
//! pointer ([`overlay_window_at`]): the topmost one in the list, clipped to the display. On the
//! desktop, or outside the display's work area (the menu bar, the Dock, the taskbar), there
//! is none and the page selects the whole display.
//!
//! Everything here is pure except the listing and the work areas, in `macos.rs` and
//! `windows.rs`.
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
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};
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
/// The visible menu bar and Dock are outside the display's work area instead (see
/// [`work_area`]).
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

/// A display as a double-click sees it: its bounds and its work area, the part windows may
/// cover (without the menu bar and the Dock, or the taskbar), both in OS units. `work` is
/// `None` if the OS did not say.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub id: u32,
    pub bounds: Bounds,
    pub work: Option<Rect>,
}

/// The work area of `display` in its OS units, from the OS's frame and visible (work) rect of
/// that screen. Those two can be in another space with the same scale, with its origin at the
/// bottom-left if `y_up` (AppKit); only the insets between them are used.
pub fn work_area(display: Bounds, frame: Rect, visible: Rect, y_up: bool) -> Rect {
    let top = if y_up {
        (frame.y + frame.height) - (visible.y + visible.height)
    } else {
        visible.y - frame.y
    };
    Rect {
        x: f64::from(display.x) + visible.x - frame.x,
        y: f64::from(display.y) + top,
        width: visible.width,
        height: visible.height,
    }
}

/// The window a double-click at `point` selects on `screen`, whose page is `size` logical px:
/// the topmost one under it, clipped to the display, in the page's logical px. `None` (the
/// page selects the whole display) on the desktop, a layer that is not a window to select, or
/// outside the work area; and if `size` is not a size.
pub(crate) fn window_at(
    targets: &[Target],
    screen: &Screen,
    size: (f64, f64),
    point: (f64, f64),
) -> Option<LogicalRect> {
    if !(size.0.is_finite() && size.1.is_finite() && size.0 > 0.0 && size.1 > 0.0) {
        return None;
    }
    let display = screen.bounds;
    // OS units per logical px: 1 on macOS, the display's scale on Windows.
    let sx = f64::from(display.width) / size.0;
    let sy = f64::from(display.height) / size.1;
    let (left, top) = (f64::from(display.x), f64::from(display.y));
    let (x, y) = (left + point.0 * sx, top + point.1 * sy);
    // On the menu bar, the Dock or the taskbar.
    if screen.work.is_some_and(|w| !contains(&w, x, y)) {
        return None;
    }
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

/// A window list being made on another thread.
#[derive(Default)]
struct Listed {
    targets: Mutex<Option<Vec<Target>>>,
    ready: Condvar,
}

impl Listed {
    fn set(&self, targets: Vec<Target>) {
        *self.targets.lock().unwrap_or_else(PoisonError::into_inner) = Some(targets);
        self.ready.notify_all();
    }

    /// `pick` on the list, waiting up to `timeout` for it; `None` if it is not ready by then.
    fn with<T>(&self, timeout: Duration, pick: impl FnOnce(&[Target]) -> T) -> Option<T> {
        let targets = self.targets.lock().unwrap_or_else(PoisonError::into_inner);
        let (targets, _) = self
            .ready
            .wait_timeout_while(targets, timeout, |t| t.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        targets.as_deref().map(pick)
    }
}

/// One capture's window list. It is listed on its own thread; asks wait for it.
struct Snapshot {
    session: u64,
    screens: Vec<Screen>,
    targets: Arc<Listed>,
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
        screens: Vec<Screen>,
        list: impl FnOnce() -> Vec<Target> + Send + 'static,
    ) {
        let targets = Arc::new(Listed::default());
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Snapshot {
            session,
            screens,
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
            targets.set(listed);
        });
    }

    /// `display` in `session` and that capture's list, which may still be in the making.
    /// `None` for another session or display.
    fn find(&self, session: u64, display: u32) -> Option<(Screen, Arc<Listed>)> {
        let snapshot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let s = snapshot.as_ref().filter(|s| s.session == session)?;
        let screen = s.screens.iter().find(|d| d.id == display)?;
        Some((*screen, Arc::clone(&s.targets)))
    }

    /// The window a double-click at `point` on `display` selects in `session` (see
    /// [`window_at`]). A list still in the making is waited for off the async runtime's
    /// workers, for up to `deadline`; after that, or for another session, `None`.
    async fn window_at(
        &self,
        session: u64,
        display: u32,
        size: (f64, f64),
        point: (f64, f64),
        deadline: Duration,
    ) -> Option<LogicalRect> {
        let (screen, targets) = self.find(session, display)?;
        let pick = move |targets: &[Target]| window_at(targets, &screen, size, point);
        if let Some(picked) = targets.with(Duration::ZERO, pick) {
            return picked;
        }
        let waited = tauri::async_runtime::spawn_blocking(move || targets.with(deadline, pick));
        match waited.await {
            Ok(Some(picked)) => picked,
            Ok(None) => {
                log::warn!("the window list was not ready in {deadline:?}: whole display");
                None
            }
            Err(e) => {
                log::error!("could not wait for the window list: {e}");
                None
            }
        }
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

/// Each display's work area, as the OS reports it now (see [`Screen`]).
fn screens(displays: &[Display]) -> Vec<Screen> {
    #[cfg(target_os = "macos")]
    let work = macos::work_areas(displays);
    #[cfg(target_os = "windows")]
    let work = windows::work_areas(displays);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let work = vec![None; displays.len()];
    displays
        .iter()
        .zip(work)
        .map(|(d, work)| Screen {
            id: d.id,
            bounds: d.bounds,
            work,
        })
        .collect()
}

/// Lists the windows for a capture that just started on `displays`, while it runs. Called
/// from the hotkey, on the main thread, where macOS answers for its screens.
pub(crate) fn snapshot<R: Runtime>(app: &AppHandle<R>, session: u64, displays: &[Display]) {
    if let Some(windows) = app.try_state::<Windows>() {
        windows.begin(session, screens(displays), list);
    }
}

/// How long a double-click waits for a window list still being made, before it selects the
/// whole display. The list takes about a millisecond.
const LIST_DEADLINE: Duration = Duration::from_millis(300);

/// The window a double-click at (`x`, `y`) selects on the calling overlay's display, in its
/// logical px; the page is `width` x `height` of them. `None`: the whole display.
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
    let Some(display) = display_id(window.label()) else {
        return Ok(None);
    };
    Ok(state
        .window_at(session, display, (width, height), (x, y), LIST_DEADLINE)
        .await)
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

    /// `bounds` as a screen with no work area reported.
    fn on(bounds: Bounds) -> Screen {
        Screen {
            id: 1,
            bounds,
            work: None,
        }
    }

    #[test]
    fn the_menu_bar_and_the_dock_are_outside_the_work_area() {
        // AppKit: the main screen is 1440 x 900 with a 25 pt menu bar and an 80 pt Dock below;
        // the frame starts at the bottom-left.
        let work = work_area(
            MAIN,
            rect(0.0, 0.0, 1440.0, 900.0),
            rect(0.0, 80.0, 1440.0, 795.0),
            true,
        );
        assert_eq!(work, rect(0.0, 25.0, 1440.0, 795.0));
        // A window under the menu bar and the Dock, and the Dock's invisible full-screen
        // window already left out.
        let screen = Screen {
            work: Some(work),
            ..on(MAIN)
        };
        let targets = [window(0.0, 0.0, 1440.0, 900.0)];
        let page = Some(area(0.0, 0.0, 1440.0, 900.0));
        assert_eq!(
            window_at(&targets, &screen, PAGE, (700.0, 10.0)),
            None,
            "menu bar"
        );
        assert_eq!(
            window_at(&targets, &screen, PAGE, (700.0, 860.0)),
            None,
            "Dock"
        );
        assert_eq!(
            window_at(&targets, &screen, PAGE, (700.0, 400.0)),
            page,
            "inside"
        );
    }

    #[test]
    fn the_taskbar_is_outside_the_work_area_of_any_display() {
        // Windows: a 150% display to the right, a 72 px taskbar at its bottom, frames in the
        // same space as the display bounds.
        let display = Bounds {
            x: 2880,
            y: 0,
            width: 2880,
            height: 1620,
        };
        let work = work_area(
            display,
            rect(2880.0, 0.0, 2880.0, 1620.0),
            rect(2880.0, 0.0, 2880.0, 1548.0),
            false,
        );
        assert_eq!(work, rect(2880.0, 0.0, 2880.0, 1548.0));
        let screen = Screen {
            work: Some(work),
            ..on(display)
        };
        let targets = [window(2880.0, 0.0, 2880.0, 1620.0)];
        let size = (1920.0, 1080.0);
        assert_eq!(window_at(&targets, &screen, size, (100.0, 1070.0)), None);
        assert!(window_at(&targets, &screen, size, (100.0, 1000.0)).is_some());
    }

    #[test]
    fn picks_the_topmost_window_under_the_point() {
        let targets = [
            window(100.0, 100.0, 300.0, 200.0),
            window(0.0, 0.0, 800.0, 600.0),
        ];
        assert_eq!(
            window_at(&targets, &on(MAIN), PAGE, (150.0, 150.0)),
            Some(area(100.0, 100.0, 300.0, 200.0)),
            "the front window"
        );
        assert_eq!(
            window_at(&targets, &on(MAIN), PAGE, (500.0, 500.0)),
            Some(area(0.0, 0.0, 800.0, 600.0)),
            "the one behind, where the front one is not"
        );
        // The right and bottom edges are outside.
        assert_eq!(
            window_at(&targets, &on(MAIN), PAGE, (400.0, 150.0)),
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
            window_at(&targets, &on(MAIN), PAGE, (10.0, 10.0)),
            None,
            "menu bar over a window"
        );
        assert_eq!(
            window_at(&targets, &on(MAIN), PAGE, (1000.0, 700.0)),
            None,
            "desktop"
        );
        assert_eq!(
            window_at(&[], &on(MAIN), PAGE, (10.0, 10.0)),
            None,
            "no windows"
        );
    }

    #[test]
    fn a_window_is_clipped_to_the_display() {
        let targets = [window(-50.0, 800.0, 400.0, 300.0)];
        assert_eq!(
            window_at(&targets, &on(MAIN), PAGE, (10.0, 850.0)),
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
            window_at(&targets, &on(left), (1920.0, 1080.0), (1600.0, 300.0)),
            Some(area(1420.0, 200.0, 500.0, 400.0))
        );
        assert_eq!(
            window_at(&targets, &on(MAIN), PAGE, (10.0, 10.0)),
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
            window_at(&targets, &on(display), (1920.0, 1080.0), (150.0, 150.0)),
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
            assert_eq!(
                window_at(&[], &on(MAIN), size, (1.0, 1.0)),
                None,
                "{size:?}"
            );
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

    fn screen(id: u32) -> Screen {
        Screen { id, ..on(MAIN) }
    }

    type Mock = tauri::test::MockRuntime;

    fn app() -> tauri::App<Mock> {
        mock_builder()
            .manage(Windows::default())
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    /// The answer for a double-click at (1, 1) on display 1.
    fn ask(
        windows: &Windows,
        session: u64,
        display: u32,
        deadline: Duration,
    ) -> Option<LogicalRect> {
        tauri::async_runtime::block_on(windows.window_at(
            session,
            display,
            PAGE,
            (1.0, 1.0),
            deadline,
        ))
    }

    const LONG: Duration = Duration::from_secs(5);

    #[test]
    fn a_double_click_waits_for_its_session_s_list() {
        let windows = Arc::new(Windows::default());
        assert_eq!(ask(&windows, 2, 1, LONG), None, "nothing listed");
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        windows.begin(2, vec![screen(1)], move || {
            rx.recv().unwrap();
            vec![window(0.0, 0.0, 10.0, 10.0)]
        });
        assert_eq!(ask(&windows, 1, 1, LONG), None, "older session");
        assert_eq!(ask(&windows, 2, 9, LONG), None, "unknown display");
        // Answered once the list is in, and at once after that.
        let asked = std::thread::spawn({
            let windows = Arc::clone(&windows);
            move || ask(&windows, 2, 1, LONG)
        });
        tx.send(()).unwrap();
        let picked = Some(area(0.0, 0.0, 10.0, 10.0));
        assert_eq!(asked.join().unwrap(), picked);
        assert_eq!(ask(&windows, 2, 1, Duration::ZERO), picked);
    }

    #[test]
    fn a_list_late_past_the_deadline_selects_the_whole_display() {
        let windows = Windows::default();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        windows.begin(1, vec![screen(1)], move || {
            let _ = rx.recv();
            vec![window(0.0, 0.0, 10.0, 10.0)]
        });
        let started = Instant::now();
        assert_eq!(ask(&windows, 1, 1, Duration::from_millis(20)), None);
        assert!(started.elapsed() < LONG, "gave up at the deadline");
        drop(tx);
    }

    #[test]
    fn a_failed_list_finds_no_window() {
        let windows = Windows::default();
        windows.begin(1, vec![screen(1)], || panic!("no windows"));
        assert_eq!(ask(&windows, 1, 1, LONG), None);
    }

    #[test]
    fn the_command_answers_for_the_calling_overlay_only() {
        let app = app();
        let handle = app.handle();
        handle
            .state::<Windows>()
            .begin(3, vec![screen(1)], || vec![window(0.0, 0.0, 10.0, 10.0)]);
        let call = |label: &str| {
            let w = tauri::WebviewWindowBuilder::new(handle, label, Default::default())
                .build()
                .unwrap();
            tauri::async_runtime::block_on(overlay_window_at(
                w,
                handle.state::<Windows>(),
                3,
                1.0,
                1.0,
                1440.0,
                900.0,
            ))
            .unwrap()
        };
        assert_eq!(call("overlay-1"), Some(area(0.0, 0.0, 10.0, 10.0)));
        assert_eq!(call("settings"), None, "not an overlay");
    }

    #[test]
    fn a_capture_lists_this_machine_s_windows_for_its_displays() {
        let app = app();
        let display = Display {
            id: 4,
            bounds: MAIN,
        };
        snapshot(app.handle(), 3, &[display]);
        let windows = app.state::<Windows>();
        let (screen, targets) = windows.find(3, 4).unwrap();
        assert_eq!((screen.id, screen.bounds), (4, MAIN));
        // Whatever is on screen, the list is made, and every window in it has a size.
        let listed = targets.with(LONG, |t| t.iter().all(|t| has_size(&t.bounds)));
        assert_eq!(listed, Some(true));
        // Without the state, a capture lists nothing and never fails.
        let bare = mock_builder().build(mock_context(noop_assets())).unwrap();
        snapshot(bare.handle(), 1, &[display]);
    }
}
