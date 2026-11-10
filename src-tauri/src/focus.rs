//! Where focus goes when the overlay or a notice closes. [`Focus`] follows the apps that come
//! to the front ([`Focus::activated`], fed by [`watch`]) and keeps the last one that wasn't
//! xcreenshot. A capture or a notice records whether xcreenshot was in front as it began
//! ([`frontmost`]); closing hands focus back to that last app ([`hand_back`]). So a notice
//! answered hours later goes back to the app the user just left, not the one in front when it
//! opened. The decisions are [`handback`] and [`Focus::hand_back`]; the system calls sit behind
//! [`Apps`] so tests can see which ran: AppKit on macOS (`macos.rs`), the foreground window on
//! Windows (`win` below). Elsewhere no [`Focus`] is managed and the system moves focus itself
//! when a window hides.
//!
//! A share is the exception: the service picked in the sheet shows its UI and takes focus, so
//! nothing is handed back. On macOS our other windows are put away instead
//! ([`park_for_share`]), and come back, behind, once another app is in front.
#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows", test)),
    allow(dead_code)
)]

use crate::windows;
use std::sync::{Mutex, MutexGuard, PoisonError};
use tauri::{AppHandle, Manager, Runtime};

/// One launch of an app: a pid can be reused by a later app, a launch date can't.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppId {
    pub pid: i32,
    /// Seconds since 1970; `None` for apps not launched through LaunchServices on macOS, or
    /// when Windows won't say.
    pub launched: Option<f64>,
    /// The window that was in front (an `HWND`), to raise again. Windows only: macOS activates
    /// whole apps.
    pub window: Option<isize>,
}

impl AppId {
    /// Whether `now` is the same launch as this one; its window may differ. Never for an app
    /// with no launch date: its pid may have been reused.
    fn same_launch(&self, now: &AppId) -> bool {
        self.launched.is_some() && self.pid == now.pid && self.launched == now.launched
    }
}

/// Who was in front when the capture started or the notice opened.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Front {
    /// xcreenshot itself (the capture started from Settings, say), when other apps had come to
    /// the front `switches` times.
    Us { switches: u64 },
    /// Another app, or none we know of.
    #[default]
    Other,
}

/// What [`Focus`] has seen of the apps in front.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Seen {
    /// Whether xcreenshot is the app in front now.
    us_in_front: bool,
    /// The last app in front that wasn't xcreenshot: where focus goes back to.
    last: Option<AppId>,
    /// How many times another app came to the front.
    switches: u64,
}

impl Seen {
    /// `app` came to the front; `ours` is our pid.
    fn activated(&mut self, app: AppId, ours: u32) {
        self.us_in_front = is_ours(&app, ours);
        if !self.us_in_front {
            self.last = Some(app);
            self.switches += 1;
        }
    }

    /// Who is in front.
    fn front(&self) -> Front {
        if self.us_in_front {
            Front::Us {
                switches: self.switches,
            }
        } else {
            Front::Other
        }
    }
}

/// No pid (-1) is never ours.
fn is_ours(app: &AppId, ours: u32) -> bool {
    u32::try_from(app.pid).is_ok_and(|p| p == ours)
}

/// What to do with focus once the overlay or a notice is hidden.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Handback {
    /// Leave it with our open window (Settings): the user started there.
    Keep,
    /// Hide xcreenshot, so the system gives focus to the app behind it.
    Hide,
    /// Activate the last app: one of our windows is open, so staying active would bring it to
    /// the front.
    Activate(AppId),
}

/// `window_open` says whether one of our other windows (Settings, a notice) is on screen.
pub fn handback(before: Front, seen: &Seen, window_open: bool) -> Handback {
    if !window_open {
        return Handback::Hide;
    }
    match before {
        // Still where the user started: no other app came up since.
        Front::Us { switches } if switches == seen.switches => Handback::Keep,
        // Hiding beats leaving Settings in front of an app we can't name.
        _ => seen.last.map_or(Handback::Hide, Handback::Activate),
    }
}

/// Whether a share puts our open windows (Settings, a notice) away while the picked service
/// shows its UI. xcreenshot stays active for that UI, so once the overlay goes macOS would make
/// one of them key and bring it forward: focus would land on Settings, not on the service.
/// Not when the capture started in that window ([`Handback::Keep`]): the user was there.
pub fn parks_for_share(before: Front, seen: &Seen, window_open: bool) -> bool {
    window_open && handback(before, seen, true) != Handback::Keep
}

/// The platform calls [`Focus`] makes.
pub trait Apps: Send + Sync {
    fn frontmost(&self) -> Option<AppId>;
    /// The app running as `pid` now, if any.
    fn running(&self, pid: i32) -> Option<AppId>;
    /// Asks `app` to come to the front; `false` if that launch is gone or the system refused.
    fn activate(&self, app: &AppId) -> bool;
    /// Hides xcreenshot.
    fn hide(&self);
}

/// Follows the app in front and hands focus back after a capture or a notice; managed by the
/// app on macOS and Windows.
pub struct Focus {
    apps: Box<dyn Apps>,
    seen: Mutex<Seen>,
    /// The windows a share put away, by label; `Some` (maybe empty: the notices hidden for the
    /// capture stay hidden too) until another app comes to the front.
    parked: Mutex<Option<Vec<String>>>,
}

impl Focus {
    /// Starts from the app in front now.
    pub fn new(apps: impl Apps + 'static) -> Self {
        let focus = Self {
            apps: Box::new(apps),
            seen: Mutex::default(),
            parked: Mutex::default(),
        };
        if let Some(app) = focus.apps.frontmost() {
            focus.activated(app);
        }
        focus
    }

    /// AppKit on macOS, the foreground window on Windows.
    #[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
    pub fn os() -> Self {
        Self::new(Os)
    }

    /// `app` came to the front. Once it isn't xcreenshot, returns what a share put away, to
    /// bring back ([`unpark`]).
    pub fn activated(&self, app: AppId) -> Option<Vec<String>> {
        let mut seen = self.seen();
        seen.activated(app, std::process::id());
        if seen.us_in_front {
            return None;
        }
        drop(seen);
        self.parked().take()
    }

    fn seen(&self) -> MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn parked(&self) -> MutexGuard<'_, Option<Vec<String>>> {
        self.parked.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A share put `labels` away; added to any still away from an earlier share.
    fn park(&self, labels: Vec<String>) {
        self.parked().get_or_insert_default().extend(labels);
    }

    fn hand_back(&self, before: Front, window_open: bool) {
        // Copied, so the lock isn't held while another app activates.
        let seen = *self.seen();
        let app = match handback(before, &seen, window_open) {
            Handback::Keep => return,
            Handback::Hide => return self.apps.hide(),
            Handback::Activate(app) => app,
        };
        // Only the same launch: its pid may now belong to another app.
        let same = self
            .apps
            .running(app.pid)
            .is_some_and(|now| app.same_launch(&now));
        if !same {
            log::info!("the last app the user was in has quit");
            self.apps.hide();
        } else if !self.apps.activate(&app) {
            log::info!("the system did not activate the last app the user was in");
            self.apps.hide();
        }
    }
}

/// Who is in front now, as last seen: no system call, so it costs nothing on the hotkey. Read
/// before the capture's or the notice's window takes focus. Main thread only.
pub fn frontmost<R: Runtime>(app: &AppHandle<R>) -> Front {
    app.try_state::<Focus>()
        .map_or(Front::Other, |focus| focus.seen().front())
}

/// Gives focus back after a capture or a notice, once its windows are hidden; `closing` is a
/// window still closing, which doesn't count as open. Main thread only.
pub fn hand_back<R: Runtime>(app: &AppHandle<R>, before: Front, closing: Option<&str>) {
    if let Some(focus) = app.try_state::<Focus>() {
        focus.hand_back(before, windows::window_open(app, closing));
    }
}

/// A service was picked in the share sheet and the overlay is about to hide (main thread):
/// hides our open windows if [`parks_for_share`] says so, before macOS can make one key. The
/// notices hidden for the capture count as open, and stay hidden. Returns whether it parked.
/// macOS only: Windows activates the window behind the overlay, and the picked app takes the
/// foreground.
pub fn park_for_share<R: Runtime>(app: &AppHandle<R>, before: Front) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let Some(focus) = app.try_state::<Focus>() else {
        return false;
    };
    let open: Vec<_> = windows::open_windows(app, None).collect();
    let window_open = !open.is_empty() || crate::notice::hidden_for_capture(app);
    let seen = *focus.seen();
    if !parks_for_share(before, &seen, window_open) {
        return false;
    }
    let mut parked = Vec::new();
    for window in open {
        match window.hide() {
            Ok(()) => parked.push(window.label().to_owned()),
            Err(e) => log::warn!("could not hide {} for the share: {e}", window.label()),
        }
    }
    log::info!("hid {parked:?} while the shared service shows");
    focus.park(parked);
    true
}

/// Another app is in front: brings back the windows a share put away (see
/// [`Focus::activated`]), behind it and without focus, then the notices it kept hidden. A
/// window shown again meanwhile (Settings from the menu) is left alone, and so are the notices
/// while a capture is open: it shows them when it closes. Main thread only.
pub fn unpark<R: Runtime>(app: &AppHandle<R>, labels: &[String]) {
    for label in labels {
        if let Some(window) = app.get_webview_window(label)
            && !window.is_visible().unwrap_or(true)
        {
            order_back(&window);
        }
    }
    if !crate::overlay::is_busy(app) {
        crate::notice::show_after_capture(app);
    }
}

/// Puts `window` on screen behind the others, without focus.
fn order_back<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    #[cfg(all(target_os = "macos", not(test)))]
    crate::macos::order_back(window);
    // Never parked elsewhere; the mock runtime has no native window.
    #[cfg(not(all(target_os = "macos", not(test))))]
    if let Err(e) = window.show() {
        log::warn!("could not show {} again: {e}", window.label());
    }
}

/// Starts following the apps that come to the front. Main thread, once, after [`Focus`] is
/// managed.
#[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
pub fn watch<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let activated = move |id| {
        if let Some(parked) = handle.state::<Focus>().activated(id) {
            unpark(&handle, &parked);
        }
    };
    #[cfg(target_os = "macos")]
    crate::macos::on_activate(activated);
    #[cfg(target_os = "windows")]
    win::on_activate(activated);
}

/// Seconds since 1970 from a Windows `FILETIME`: 100 ns ticks since 1601.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn unix_seconds(filetime: u64) -> f64 {
    const TICKS_TO_1970: u64 = 116_444_736_000_000_000;
    filetime.saturating_sub(TICKS_TO_1970) as f64 / 1e7
}

/// Windows window classes the shell brings to the front for a moment: the Alt-Tab switcher and
/// the tray overflow. With the taskbars and the desktop, focus never goes back to them.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WINDOWS_PASSING: [&str; 4] = [
    "MultitaskingViewFrame",
    "XamlExplorerHostIslandWindow",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
];

/// Whether focus may go back to a Windows window of `class`: not the shell's, and not a tool
/// window (a palette or a tray pop-up).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn returns_to(class: &str, tool_window: bool) -> bool {
    !tool_window
        && !crate::pick::WINDOWS_SHELL.contains(&class)
        && !WINDOWS_PASSING.contains(&class)
}

#[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
struct Os;

#[cfg(all(target_os = "macos", not(test)))]
impl Apps for Os {
    fn frontmost(&self) -> Option<AppId> {
        crate::macos::frontmost()
    }
    fn running(&self, pid: i32) -> Option<AppId> {
        crate::macos::running(pid)
    }
    fn activate(&self, app: &AppId) -> bool {
        crate::macos::activate(app)
    }
    fn hide(&self) {
        crate::macos::hide()
    }
}

#[cfg(all(target_os = "windows", not(test)))]
impl Apps for Os {
    fn frontmost(&self) -> Option<AppId> {
        win::frontmost()
    }
    fn running(&self, pid: i32) -> Option<AppId> {
        win::running(pid)
    }
    fn activate(&self, app: &AppId) -> bool {
        win::activate(app)
    }
    /// Nothing to do: Windows gives focus to the window behind once ours hide.
    fn hide(&self) {}
}

/// The foreground window and its process, on Windows.
#[cfg(all(target_os = "windows", not(test)))]
mod win {
    use super::{AppId, returns_to, unix_seconds};
    use ::windows::Win32::Foundation::{CloseHandle, FILETIME, HWND};
    use ::windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use ::windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
    use ::windows::Win32::UI::WindowsAndMessaging::{
        EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindowLongW,
        GetWindowThreadProcessId, OBJID_WINDOW, SetForegroundWindow, WINEVENT_OUTOFCONTEXT,
        WS_EX_TOOLWINDOW,
    };
    use std::ffi::c_void;
    use std::sync::OnceLock;

    type Activated = Box<dyn Fn(AppId) + Send + Sync>;

    /// Called with each window that comes to the front.
    static ACTIVATED: OnceLock<Activated> = OnceLock::new();

    pub(super) fn frontmost() -> Option<AppId> {
        app_of(unsafe { GetForegroundWindow() })
    }

    /// The process that owns `hwnd`, with `hwnd` to raise again; `None` for a shell window
    /// focus shouldn't go back to (see [`returns_to`]).
    fn app_of(hwnd: HWND) -> Option<AppId> {
        if hwnd.is_invalid() {
            return None;
        }
        let pid = owner(hwnd)?;
        let window = Some(hwnd.0 as isize);
        // Our own windows only need the pid: no process lookup on every overlay focus.
        if u32::try_from(pid).is_ok_and(|p| p == std::process::id()) {
            return Some(AppId {
                pid,
                launched: None,
                window,
            });
        }
        let mut class = [0u16; 64];
        // SAFETY: these calls only read `hwnd`; `class` outlives the call.
        let (len, ex_style) = unsafe {
            let len = GetClassNameW(hwnd, &mut class);
            (len, GetWindowLongW(hwnd, GWL_EXSTYLE) as u32)
        };
        let class = String::from_utf16_lossy(&class[..usize::try_from(len).unwrap_or(0)]);
        if !returns_to(&class, ex_style & WS_EX_TOOLWINDOW.0 != 0) {
            return None;
        }
        Some(AppId {
            window,
            ..running(pid)?
        })
    }

    /// The pid of the process that owns `hwnd`.
    fn owner(hwnd: HWND) -> Option<i32> {
        let mut pid = 0u32;
        // SAFETY: `pid` outlives the call; a stale `hwnd` only makes it return 0.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        i32::try_from(pid).ok().filter(|&p| p != 0)
    }

    /// `pid`'s app, with no launch date if Windows won't open the process (a protected one, or
    /// one that quit): still an app the user went to, but never one to activate.
    pub(super) fn running(pid: i32) -> Option<AppId> {
        let id = u32::try_from(pid).ok()?;
        Some(AppId {
            pid,
            launched: launched(id),
            window: None,
        })
    }

    /// When process `pid` started, in seconds since 1970.
    fn launched(pid: u32) -> Option<f64> {
        // SAFETY: plain calls on a handle we own and close.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let [mut created, mut exited, mut kernel, mut user] = [FILETIME::default(); 4];
            let ok =
                GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user).is_ok();
            let _ = CloseHandle(process);
            ok.then(|| {
                let ticks =
                    (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
                unix_seconds(ticks)
            })
        }
    }

    /// Raises `app`'s window, if it is still that app's.
    pub(super) fn activate(app: &AppId) -> bool {
        let Some(window) = app.window else {
            return false;
        };
        let hwnd = HWND(window as *mut c_void);
        // A closed window's handle can be reused by another app's.
        if owner(hwnd) != Some(app.pid) {
            return false;
        }
        // SAFETY: a plain call; Windows checks the handle.
        unsafe { SetForegroundWindow(hwnd) }.as_bool()
    }

    /// Calls `f` with each window that comes to the front, on this thread's message loop
    /// (the main thread's), for as long as the app runs.
    pub(super) fn on_activate(f: impl Fn(AppId) + Send + Sync + 'static) {
        if ACTIVATED.set(Box::new(f)).is_err() {
            return log::error!("already following the foreground window");
        }
        // SAFETY: the hook is never removed and calls back on this thread only.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            log::error!("could not follow the foreground window");
        }
    }

    unsafe extern "system" fn foreground_changed(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        if object != OBJID_WINDOW.0 {
            return;
        }
        if let (Some(f), Some(app)) = (ACTIVATED.get(), app_of(hwnd)) {
            f(app);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    pub(crate) const SAFARI: AppId = AppId {
        pid: 7,
        launched: Some(1.0),
        window: None,
    };

    pub(crate) const MAIL: AppId = AppId {
        pid: 8,
        launched: Some(2.0),
        window: None,
    };

    /// xcreenshot itself.
    pub(crate) fn us() -> AppId {
        AppId {
            pid: std::process::id() as i32,
            ..SAFARI
        }
    }

    /// Fake apps: `front` is in front, `running` are running, and activation works if
    /// `activates`. Records each call it gets.
    #[derive(Clone)]
    pub(crate) struct Fake {
        pub front: Option<AppId>,
        pub running: Vec<AppId>,
        pub activates: bool,
        pub calls: Arc<Mutex<Vec<String>>>,
    }

    impl Fake {
        pub(crate) fn new(front: Option<AppId>) -> Self {
            Self {
                front,
                running: front.into_iter().collect(),
                activates: true,
                calls: Arc::default(),
            }
        }

        pub(crate) fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn call(&self, what: String) {
            self.calls.lock().unwrap().push(what);
        }
    }

    impl Apps for Fake {
        fn frontmost(&self) -> Option<AppId> {
            self.front
        }
        fn running(&self, pid: i32) -> Option<AppId> {
            self.running.iter().copied().find(|a| a.pid == pid)
        }
        fn activate(&self, app: &AppId) -> bool {
            self.call(format!("activate {}", app.pid));
            self.activates
        }
        fn hide(&self) {
            self.call("hide".into());
        }
    }

    fn hand_back_with(fake: &Fake, before: Front, window_open: bool) -> Vec<String> {
        Focus::new(fake.clone()).hand_back(before, window_open);
        fake.calls()
    }

    /// What was seen after `apps` came to the front in turn.
    fn seen(apps: &[AppId]) -> Seen {
        let mut seen = Seen::default();
        for &app in apps {
            seen.activated(app, 42);
        }
        seen
    }

    #[test]
    fn our_own_pid_is_us() {
        let ours = AppId { pid: 42, ..SAFARI };
        assert_eq!(seen(&[]).front(), Front::Other);
        assert_eq!(seen(&[SAFARI]).front(), Front::Other);
        assert_eq!(seen(&[SAFARI, ours]).front(), Front::Us { switches: 1 });
        // No pid (-1) is never us.
        let no_pid = AppId { pid: -1, ..SAFARI };
        assert!(!is_ours(&no_pid, u32::MAX));
    }

    #[test]
    fn the_last_app_is_the_last_one_that_was_not_us() {
        let ours = AppId { pid: 42, ..SAFARI };
        let seen = seen(&[SAFARI, ours, MAIL, ours]);
        assert_eq!(seen.last, Some(MAIL));
        assert_eq!(seen.switches, 2);
    }

    #[test]
    fn with_no_window_open_xcreenshot_hides() {
        let us = Front::Us { switches: 0 };
        for before in [Front::Other, us] {
            assert_eq!(handback(before, &seen(&[SAFARI]), false), Handback::Hide);
            assert_eq!(hand_back_with(&Fake::new(None), before, false), ["hide"]);
        }
    }

    #[test]
    fn with_settings_open_focus_returns_to_the_last_app() {
        assert_eq!(
            handback(Front::Other, &seen(&[SAFARI, MAIL]), true),
            Handback::Activate(MAIL)
        );
        let fake = Fake::new(Some(SAFARI));
        assert_eq!(hand_back_with(&fake, Front::Other, true), ["activate 7"]);
    }

    #[test]
    fn started_from_settings_keeps_settings_focused_until_another_app_comes_up() {
        let started = Front::Us { switches: 1 };
        assert_eq!(handback(started, &seen(&[SAFARI]), true), Handback::Keep);
        // The user went to Mail and back to answer: Mail is where they were.
        assert_eq!(
            handback(started, &seen(&[SAFARI, MAIL]), true),
            Handback::Activate(MAIL)
        );
        let fake = Fake::new(Some(us()));
        assert!(hand_back_with(&fake, Front::Us { switches: 0 }, true).is_empty());
    }

    #[test]
    fn an_unknown_app_hides_instead_of_keeping_settings_in_front() {
        assert_eq!(
            hand_back_with(&Fake::new(None), Front::Other, true),
            ["hide"]
        );
    }

    #[test]
    fn a_refused_activation_hides() {
        let fake = Fake {
            activates: false,
            ..Fake::new(Some(SAFARI))
        };
        assert_eq!(
            hand_back_with(&fake, Front::Other, true),
            ["activate 7", "hide"]
        );
    }

    #[test]
    fn an_app_that_quit_or_lost_its_pid_is_not_activated() {
        let reused = AppId {
            launched: Some(2.0),
            ..SAFARI
        };
        let undated = AppId {
            launched: None,
            ..SAFARI
        };
        for (running, last) in [
            (vec![], SAFARI),
            (vec![reused], SAFARI),
            (vec![undated], undated),
        ] {
            let fake = Fake {
                running,
                ..Fake::new(Some(last))
            };
            assert_eq!(hand_back_with(&fake, Front::Other, true), ["hide"]);
        }
    }

    #[test]
    fn the_same_launch_may_have_another_window_in_front() {
        let other_window = AppId {
            window: Some(3),
            ..SAFARI
        };
        assert!(other_window.same_launch(&SAFARI));
        assert!(!SAFARI.same_launch(&MAIL));
    }

    #[test]
    fn focus_follows_the_apps_that_come_to_the_front() {
        let fake = Fake {
            running: vec![SAFARI, MAIL],
            ..Fake::new(Some(SAFARI))
        };
        let focus = Focus::new(fake.clone());
        focus.activated(us());
        focus.activated(MAIL);
        focus.activated(us());
        focus.hand_back(Front::Other, true);
        assert_eq!(fake.calls(), ["activate 8"]);
    }

    #[test]
    fn a_share_parks_our_windows_unless_the_capture_started_in_them() {
        let open = seen(&[SAFARI]);
        assert!(parks_for_share(Front::Other, &open, true));
        assert!(
            !parks_for_share(Front::Other, &open, false),
            "nothing to park"
        );
        let started_here = Front::Us { switches: 1 };
        assert!(!parks_for_share(started_here, &open, true));
        // The user went to Mail meanwhile: Settings is no longer where they are.
        assert!(parks_for_share(started_here, &seen(&[SAFARI, MAIL]), true));
        // Hiding beats Settings in front of an app we can't name.
        assert!(parks_for_share(Front::Other, &Seen::default(), true));
    }

    #[test]
    fn parked_windows_come_back_once_another_app_is_in_front() {
        let focus = Focus::new(Fake::new(Some(SAFARI)));
        assert_eq!(focus.activated(MAIL), None, "nothing parked");
        focus.park(vec!["settings".into()]);
        focus.park(vec!["notice-1".into()]);
        assert_eq!(focus.activated(us()), None, "the service's UI is ours");
        assert_eq!(
            focus.activated(SAFARI),
            Some(vec!["settings".into(), "notice-1".into()])
        );
        assert_eq!(focus.activated(MAIL), None, "once");
        focus.park(Vec::new());
        assert_eq!(focus.activated(SAFARI), Some(Vec::new()), "for the notices");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_share_hides_settings_without_handing_focus_back() {
        let fake = Fake::new(Some(SAFARI));
        let app = crate::overlay::window::tests::app();
        let h = app.handle();
        app.manage(Focus::new(fake.clone()));
        tauri::WebviewWindowBuilder::new(h, "settings", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        assert!(park_for_share(h, Front::Other));
        assert!(fake.calls().is_empty(), "the service takes focus");
        let parked = h.state::<Focus>().activated(SAFARI).unwrap();
        assert_eq!(parked, ["settings"]);
        unpark(h, &parked);
        // Started in Settings: it stays.
        h.state::<Focus>().activated(us());
        assert!(!park_for_share(h, frontmost(h)));
        assert_eq!(h.state::<Focus>().activated(SAFARI), None);
    }

    #[test]
    fn a_share_parks_nothing_without_focus_managed_or_off_macos() {
        let app = crate::overlay::window::tests::app();
        assert!(!park_for_share(app.handle(), Front::Other));
        app.manage(Focus::new(Fake::new(Some(SAFARI))));
        tauri::WebviewWindowBuilder::new(app.handle(), "settings", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        let parks = park_for_share(app.handle(), Front::Other);
        assert_eq!(parks, cfg!(target_os = "macos"));
    }

    #[test]
    fn windows_file_times_become_unix_seconds() {
        assert_eq!(unix_seconds(116_444_736_000_000_000), 0.0);
        assert_eq!(unix_seconds(116_444_736_015_000_000), 1.5);
        assert_eq!(unix_seconds(0), 0.0, "before 1970 clamps");
    }

    #[test]
    fn focus_never_goes_back_to_the_windows_shell_or_a_tool_window() {
        assert!(returns_to("Notepad", false));
        assert!(!returns_to("Notepad", true));
        for class in crate::pick::WINDOWS_SHELL.iter().chain(&WINDOWS_PASSING) {
            assert!(!returns_to(class, false), "{class}");
        }
    }

    #[test]
    fn without_focus_managed_nothing_is_read() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        assert_eq!(frontmost(app.handle()), Front::Other);
    }
}
