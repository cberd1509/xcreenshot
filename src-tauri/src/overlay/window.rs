//! Tauri glue for the overlay.
//!
//! Hotkey (main thread): claim the [`Phase`], take Esc, start the capture on a worker
//! thread and place the hidden windows while it runs. Capture done (worker): open the session
//! and send `overlay:show` with each frame's size. Each window fetches its frame
//! (`overlay_frame`: a copy of the raw RGBA buffer), draws it, waits until it is rendered and
//! calls `overlay_painted`, which shows that window. Esc goes to every page (`overlay:escape`
//! with a token); each answers `overlay_escaped`, saying whether it undid something (a size
//! edit, a selection).
//! Rust closes when every shown page had nothing to undo, or when no page answers in time.
//! Copy and save are in `export.rs`. `close_overlay`, closing a window or the watchdog hide
//! everything and tell the pages to free their frames (`overlay:closed`); calls from a page
//! only ever close their own session.
//! Rust holds Esc exactly when [`Phase::wants_escape`] says so: [`sync_escape`] runs after every
//! phase change. Window and shortcut calls stay on the main thread.

use super::{
    FrameInfo, Phase, captured_all, diff, display_at, display_id, label, ms_since, paint_message,
};
use crate::capture::{self, Bounds, CaptureError, Display, DisplayFrame};
use crate::focus;
use crate::region::{self, Start};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tauri::ipc::Response;
use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcut, ShortcutState};

const SHOW_EVENT: &str = "overlay:show";
/// The overlay closed: the pages free their frames and selections.
const CLOSED_EVENT: &str = "overlay:closed";
/// Sent to every page when a capture ends (copy, save, share or Esc).
pub const CAPTURE_CLOSED: &str = "capture:closed";
/// Esc, forwarded to every overlay page.
const ESCAPE_EVENT: &str = "overlay:escape";
const ESCAPE: &str = "Escape";
/// How long the pages get to handle Esc before Rust closes the overlay itself.
const ESCAPE_TIMEOUT: Duration = Duration::from_millis(300);
/// A window whose page hasn't reported its paint this long after the capture is shown anyway
/// ([`reveal_late`]): the page's own fallback (`PAINT_TIMEOUT_MS`, 200 ms) reports by then, so
/// a later report may never come. It may flash, but the capture isn't lost.
const REVEAL_AFTER: Duration = Duration::from_millis(300);
/// [`REVEAL_AFTER`] for a capture on a window that hasn't painted yet (see [`is_cold`]): a cold
/// webview can take that long to render its first frame, and showing it before then flashes
/// black. A late overlay beats a black flash. Well after the page's own first wait
/// (`FIRST_PAINT_TIMEOUT_MS`, 600 ms), which starts only once the frame is fetched and drawn.
const FIRST_REVEAL_AFTER: Duration = Duration::from_millis(1000);
/// If no window paints within this time the overlay page is broken: close everything, so a
/// hung page never locks the user out.
const WATCHDOG: Duration = Duration::from_secs(2);

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
static NEXT_ESCAPE: AtomicU64 = AtomicU64::new(1);

/// When the overlay was prewarmed (launch), until the first capture paints and logs it.
static LAUNCHED: Mutex<Option<Instant>> = Mutex::new(None);
/// Captures shown and closed since launch, for the memory log line.
static CLOSED: AtomicU32 = AtomicU32::new(0);
/// How long after a capture closes the app's memory is logged: once the frames are freed.
/// The allocator may keep some for up to a minute (see `docs/spikes/capture.md`).
const MEMORY_AFTER: Duration = Duration::from_secs(5);

/// Overlay state, managed by the app.
#[derive(Default)]
pub struct Overlay {
    phase: Mutex<Phase>,
    /// The displays whose overlay window is on screen, as Rust last showed or hid them.
    shown: Mutex<Vec<u32>>,
    /// The displays whose overlay window is transparent and click-through, so it must be
    /// unstaged before it shows: staged for its paint ([`stage`]), on screen while it renders
    /// its frame or hidden since, or concealed as the overlay closed ([`conceal`]).
    staged: Mutex<Vec<u32>>,
    /// The displays whose overlay window has reported a paint since it was built.
    warm: Mutex<Vec<u32>>,
    /// Fired when the phase returns to Idle; the update restart waits for it.
    pub(crate) idle: tokio::sync::Notify,
}

impl Overlay {
    pub(super) fn phase(&self) -> MutexGuard<'_, Phase> {
        lock(&self.phase)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn phase<R: Runtime>(app: &AppHandle<R>) -> MutexGuard<'_, Phase> {
    app.state::<Overlay>().inner().phase()
}

/// Shows or hides an overlay window and records which are on screen ([`shown`]). A staged
/// window that can't be made opaque and clickable is not shown (see [`unstage_steps`]).
fn set_shown<R: Runtime>(window: &WebviewWindow<R>, on: bool) -> Result<(), String> {
    if on {
        unstage(window)?;
        window.show()
    } else {
        window.hide()
    }
    .map_err(|e| e.to_string())?;
    if let Some(display) = display_id(window.label()) {
        let overlay = window.state::<Overlay>();
        let mut shown = lock(&overlay.shown);
        shown.retain(|&d| d != display);
        if on {
            shown.push(display);
        }
    }
    Ok(())
}

/// The displays whose overlay window is on screen. Tests check it; the mock runtime has no
/// visibility of its own.
#[cfg(test)]
pub(super) fn shown<R: Runtime>(app: &AppHandle<R>) -> Vec<u32> {
    sorted(&app.state::<Overlay>().shown)
}

/// The displays whose overlay window is staged ([`stage`]).
#[cfg(test)]
pub(super) fn staged<R: Runtime>(app: &AppHandle<R>) -> Vec<u32> {
    sorted(&app.state::<Overlay>().staged)
}

#[cfg(test)]
fn sorted(displays: &Mutex<Vec<u32>>) -> Vec<u32> {
    let mut displays = lock(displays).clone();
    displays.sort_unstable();
    displays
}

/// The OS side of a capture: permission, listing displays and grabbing them. Faked in tests.
pub(crate) trait Screen: Send + 'static {
    /// Whether capture is allowed. If not, opens the onboarding window with the steps.
    fn permitted<R: Runtime>(&self, app: &AppHandle<R>) -> bool;
    fn displays(&self) -> Result<Vec<Display>, CaptureError>;
    fn capture(&self, ids: &[u32]) -> Result<Vec<DisplayFrame>, CaptureError>;
    /// The cursor in the displays' OS units (points on macOS, pixels on Windows).
    fn cursor<R: Runtime>(&self, app: &AppHandle<R>) -> Option<(f64, f64)>;
    /// The remembered regions, whether quick copy is on and whether the tip shows (see
    /// `region::load`).
    fn start<R: Runtime>(&self, app: &AppHandle<R>) -> Start;
    /// A capture opened with the quick copy tip: counts it (see `tip::showing`).
    fn tip_shown<R: Runtime>(&self, app: &AppHandle<R>);
    /// Pays the OS's one-time capture setup now. Does nothing unless capture is already
    /// allowed, so it never asks for permission.
    fn warm_up(&self);
}

/// The real screen, through `capture`.
struct Os;

impl Screen for Os {
    fn permitted<R: Runtime>(&self, app: &AppHandle<R>) -> bool {
        if capture::has_permission() {
            return true;
        }
        crate::onboarding::show(app);
        false
    }

    fn displays(&self) -> Result<Vec<Display>, CaptureError> {
        capture::displays()
    }

    fn capture(&self, ids: &[u32]) -> Result<Vec<DisplayFrame>, CaptureError> {
        capture::capture_displays(ids)
    }

    fn cursor<R: Runtime>(&self, app: &AppHandle<R>) -> Option<(f64, f64)> {
        let p = app.cursor_position().ok()?;
        // macOS reports points times the primary display's scale.
        let scale = if cfg!(target_os = "macos") {
            app.primary_monitor().ok()??.scale_factor()
        } else {
            1.0
        };
        Some((p.x / scale, p.y / scale))
    }

    fn start<R: Runtime>(&self, app: &AppHandle<R>) -> Start {
        region::load(app)
    }

    fn tip_shown<R: Runtime>(&self, app: &AppHandle<R>) {
        crate::tip::showing(app);
    }

    fn warm_up(&self) {
        if !capture::has_permission() {
            return;
        }
        let started = Instant::now();
        match capture::warm_up() {
            Ok(()) => log::info!("capture warmed up in {:.0} ms", ms_since(started)),
            Err(e) => log::warn!("could not warm up the capture: {e}"),
        }
    }
}

/// Creates a hidden overlay window per display at startup, so the hotkey only has to show
/// them, then warms up the capture on a worker thread so the first hotkey is as fast as the
/// next ones. Never blocks startup.
pub fn prewarm<R: Runtime>(app: &AppHandle<R>) {
    prewarm_with(app, Os);
}

fn prewarm_with<R: Runtime>(app: &AppHandle<R>, screen: impl Screen) {
    lock(&LAUNCHED).get_or_insert_with(Instant::now);
    let displays = match screen.displays() {
        Ok(displays) if !displays.is_empty() => displays,
        Ok(_) => return log::error!("no displays for the overlay"),
        Err(e) => return log::error!("could not list displays for the overlay: {e}"),
    };
    if let Err(e) = sync(app, &ids(&displays)) {
        log::error!("could not create overlay windows: {e}");
    }
    std::thread::spawn(move || {
        screen.warm_up();
        // The baseline that memory after captures is compared with.
        log_memory("after the startup warm-up");
    });
}

/// Logs the app's own memory (the webviews are other processes). Nothing where it isn't read.
fn log_memory(when: &str) {
    if let Some(usage) = crate::memory::now() {
        log::info!("memory {when}: {usage}");
    }
}

/// Logs the memory [`MEMORY_AFTER`] the `closed`th capture closed, to show whether it
/// returns to a steady level or grows with each capture. Not if another capture started
/// meanwhile: its frames would count.
fn log_memory_later<R: Runtime>(app: &AppHandle<R>, closed: u32) {
    if crate::memory::now().is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(MEMORY_AFTER).await;
        if matches!(*phase(&app), Phase::Idle) {
            log_memory(&format!("{MEMORY_AFTER:?} after capture {closed} closed"));
        }
    });
}

/// Hotkey handler (main thread). Returns the session id if a capture started.
pub fn start_capture<R: Runtime>(app: &AppHandle<R>) -> Option<u64> {
    start_with(app, Os)
}

/// Claims the phase, starts the capture on a worker thread, then registers Esc and places the
/// hidden windows while it runs.
fn start_with<R: Runtime>(app: &AppHandle<R>, screen: impl Screen) -> Option<u64> {
    let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    if !phase(app).begin(session, Instant::now()) {
        log::info!("capture ignored: the overlay is already open");
        return None;
    }
    let listed = screen.permitted(app).then(|| screen.displays());
    let Some(Ok(displays)) = listed else {
        if let Some(Err(e)) = listed {
            log::error!("could not list displays: {e}");
        }
        *phase(app) = Phase::Idle;
        app.state::<Overlay>().idle.notify_waiters();
        return None;
    };
    // Before the capture, so a notice or a toast isn't in the frame; with none open it costs
    // nothing. The overlay is always on top: a notice waits until it closes.
    crate::notice::hide_for_capture(app);
    crate::toast::hide_for_capture(app);
    // Before the capture, so the pages learn whether Rust owns Esc.
    let escape_shortcut = sync_escape(app);
    let worker = app.clone();
    let listed = displays.clone();
    std::thread::spawn(move || {
        let frames = screen.capture(&ids(&listed));
        let focus = screen
            .cursor(&worker)
            .and_then(|(x, y)| display_at(&listed, x, y));
        // Read last, just before the session opens, so it follows the current setting. No
        // regions if a display failed: see `Phase::open`.
        let mut start = screen.start(&worker);
        if !frames.as_ref().is_ok_and(|f| captured_all(&listed, f)) {
            start.regions = None;
        }
        let tip = start.mode.quick_copy_tip;
        let opened = finish(
            &worker,
            session,
            frames,
            focus,
            escape_shortcut,
            start,
            WATCHDOG,
        );
        // Only once a page was told to draw it, and after: nothing on screen waits for this.
        if opened && tip {
            screen.tip_shown(&worker);
        }
    });
    // After the capture starts, and before any overlay window takes focus. A cheap read: no
    // system call.
    let front = focus::frontmost(app);
    phase(app).set_front(front);
    // Once the capture is under way, in parallel with it, so a double-click can pick a window.
    crate::pick::snapshot(app, session, &displays);
    if let Err(e) = place(app, &displays) {
        log::error!("could not place the overlay windows: {e}");
        close(app);
        return None;
    }
    Some(session)
}

/// Capture finished (worker thread). Opens the session and tells each window to draw. Returns
/// whether the session opened and at least one window was told to draw.
fn finish<R: Runtime>(
    app: &AppHandle<R>,
    session: u64,
    frames: Result<Vec<DisplayFrame>, CaptureError>,
    focus: Option<u32>,
    escape_shortcut: bool,
    start: Start,
    watchdog: Duration,
) -> bool {
    let frames = match frames {
        Ok(frames) => frames,
        Err(e) => {
            log::error!("capture failed: {e}");
            on_main(app, move |app| close_session(app, session));
            return false;
        }
    };
    // `None`: Esc closed the overlay during the capture.
    let regions = start.regions.as_deref();
    let opened = phase(app).open(session, frames, focus, escape_shortcut, regions, start.mode);
    let Some(shows) = opened else {
        return false;
    };
    let displays: Vec<u32> = shows.iter().map(|(d, _)| *d).collect();
    let cold = is_cold(&lock(&app.state::<Overlay>().warm), &displays);
    on_main(app, move |app| {
        sync_escape(app);
        stage_unpainted(app, session, &displays);
    });
    let mut told = false;
    for (display, info) in shows {
        match app.emit_to(label(display).as_str(), SHOW_EVENT, info) {
            Ok(()) => told = true,
            Err(e) => log::warn!("could not tell overlay {display} to draw: {e}"),
        }
    }
    // Tests reveal by hand, so their sessions never change under them.
    let reveal = if cfg!(test) {
        watchdog
    } else {
        reveal_after(watchdog, cold)
    };
    if cold {
        log::info!("first paint of an overlay window: showing it unpainted after {reveal:?}");
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(reveal);
        on_main(&app, move |app| reveal_late(app, session));
        std::thread::sleep(watchdog - reveal);
        on_main(&app, move |app| rescue(app, session));
    });
    told
}

/// Whether a capture on `displays` waits for a window that has never painted (`warm` lists
/// those that have): the first capture since launch, or on a display added since.
fn is_cold(warm: &[u32], displays: &[u32]) -> bool {
    displays.iter().any(|d| !warm.contains(d))
}

/// When to show windows whose page hasn't reported its paint: [`REVEAL_AFTER`], or
/// [`FIRST_REVEAL_AFTER`] if one is `cold` (see [`is_cold`]); never after the watchdog.
fn reveal_after(watchdog: Duration, cold: bool) -> Duration {
    let after = if cold {
        FIRST_REVEAL_AFTER
    } else {
        REVEAL_AFTER
    };
    after.min(watchdog)
}

/// Shows `session`'s windows whose page hasn't reported its paint (see [`REVEAL_AFTER`]),
/// focusing one if nothing painted. The watchdog still closes the overlay if no page ever
/// reports: that page is dead.
fn reveal_late<R: Runtime>(app: &AppHandle<R>, session: u64) {
    let Some((displays, focus_on)) = phase(app).unreported(session) else {
        return;
    };
    log::warn!("overlays {displays:?} did not report their paint in time; showing them");
    for d in displays {
        let shown = app
            .get_webview_window(&label(d))
            .map_or(Ok(()), |w| set_shown(&w, true));
        if let Err(e) = shown {
            log::error!("could not show the overlay: {e}");
            return close(app);
        }
    }
    focus(app, focus_on, None);
}

/// Stages `displays`' windows for `session`'s paint. Not if a close ran first: nothing would
/// hide them again. Not one that already painted: it would turn invisible and click-through.
fn stage_unpainted<R: Runtime>(app: &AppHandle<R>, session: u64, displays: &[u32]) {
    if phase(app).session() != Some(session) {
        return;
    }
    let shown = lock(&app.state::<Overlay>().shown).clone();
    for &d in displays.iter().filter(|d| !shown.contains(d)) {
        if let Some(window) = app.get_webview_window(&label(d)) {
            stage(&window);
        }
    }
}

pub(super) fn on_main<R: Runtime>(
    app: &AppHandle<R>,
    f: impl FnOnce(&AppHandle<R>) + Send + 'static,
) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || f(&handle)) {
        log::error!("could not run on the main thread: {e}");
    }
}

/// Watchdog: nothing painted, so the overlay page is broken. Close instead of hanging.
fn rescue<R: Runtime>(app: &AppHandle<R>, session: u64) {
    if phase(app).stalled(session) {
        log::warn!("overlay did not paint within {WATCHDOG:?}; closing it");
        close(app);
    }
}

/// Closes the overlay only if `session` is still the current one.
pub fn close_session<R: Runtime>(app: &AppHandle<R>, session: u64) {
    close_if(app, |phase| phase.session() == Some(session));
}

/// Hides every overlay, frees the frames, releases Esc and allows the next capture. Does
/// nothing while the Save dialog or share sheet is open: they end the session themselves, so a
/// save or share is never lost to a close. Main thread only. Safe to call twice.
pub fn close<R: Runtime>(app: &AppHandle<R>) {
    close_if(app, |_| true);
}

/// [`close`], only if `ok` holds for the phase. Checked and closed under one lock, so nothing
/// can start in between (a save, say).
pub fn close_if<R: Runtime>(app: &AppHandle<R>, ok: impl FnOnce(&Phase) -> bool) {
    let before = end_if(app, false, |phase| {
        if phase.in_dialog() {
            log::info!("not closing the overlay while the Save dialog or share sheet is open");
            return false;
        }
        ok(phase)
    });
    // Give focus back to the app the user was in, even with Settings open behind it. Only the
    // close that ends the capture does: a second close has nothing to hand back.
    if let Some(before) = before {
        focus::hand_back(app, before, None);
    }
}

/// Ends `session`'s share sheet and closes the overlay in one step, so nothing can start or
/// close in between. The app stays active: a service chosen in the sheet shows its UI
/// (AirDrop, Mail…) in this app, and hiding the app would hide it too. Our other windows are
/// put away first if focus would land on them instead (see [`focus::park_for_share`]).
/// Returns whether `session` was the one sharing. Main thread only.
pub(crate) fn close_for_share<R: Runtime>(app: &AppHandle<R>, session: u64) -> bool {
    end_if(app, true, |phase| phase.end_share(session)).is_some()
}

/// Closes if `ok` (which may change the phase first) holds, under one lock. Returns the app
/// that was in front at the capture's hotkey if this ended a capture; `None` if `ok` failed
/// or nothing was open (see [`Phase::end`]). `share`: a service was picked in the share sheet.
fn end_if<R: Runtime>(
    app: &AppHandle<R>,
    share: bool,
    ok: impl FnOnce(&mut Phase) -> bool,
) -> Option<focus::Front> {
    let (before, shown) = {
        let mut phase = phase(app);
        if !ok(&mut phase) {
            return None;
        }
        let shown = matches!(*phase, Phase::Open(_));
        (phase.end(), shown)
    };
    sync_escape(app);
    // Before the overlay hides: AppKit would make the next window of ours key at once.
    let parked = share && before.is_some_and(|front| focus::park_for_share(app, front));
    // Concealed and hidden before the page frees its frame.
    for (label, window) in app.webview_windows() {
        if display_id(&label).is_none() {
            continue;
        }
        conceal(&window);
        if !hide_window(&label, &window) {
            continue;
        }
        if let Err(e) = app.emit_to(label.as_str(), CLOSED_EVENT, ()) {
            log::warn!("could not tell {label} to free its frame: {e}");
        }
    }
    // A notice hidden for the capture comes back after a share too, unless the share put our
    // windows away (it comes back with them); only giving focus back (in `close_if`) is
    // skipped. First, so a notice shown again counts as an open window.
    if !parked {
        crate::notice::show_after_capture(app);
    }
    // For any page that waits for a capture to end (onboarding's "Try it"). Only a capture
    // that was shown counts, not one that failed while capturing.
    if shown {
        if let Err(e) = app.emit(CAPTURE_CLOSED, ()) {
            log::warn!("could not send {CAPTURE_CLOSED}: {e}");
        }
        log_memory_later(app, CLOSED.fetch_add(1, Ordering::Relaxed) + 1);
    }
    // Last, so a waiting update restart sees the overlay gone.
    app.state::<Overlay>().idle.notify_waiters();
    before
}

/// Hides the overlay windows on screen, keeping the session (the Save dialog hides them this
/// way). A window still staged for its paint stays staged: invisible and click-through, it
/// goes on rendering its frame, so it is ready when [`reshow`] shows it. Main thread only.
pub(super) fn hide<R: Runtime>(app: &AppHandle<R>) {
    let shown = lock(&app.state::<Overlay>().shown).clone();
    hide_if(app, |d| shown.contains(&d));
}

/// Hides the overlay windows of the displays where `which` holds. Main thread only.
fn hide_if<R: Runtime>(app: &AppHandle<R>, which: impl Fn(u32) -> bool) {
    for (label, window) in app.webview_windows() {
        if display_id(&label).is_some_and(&which) {
            hide_window(&label, &window);
        }
    }
}

/// Hides one overlay window, or destroys it if it can't be: a window left on screen would block
/// input, and `sync` rebuilds it. Returns whether it was hidden. Main thread only.
fn hide_window<R: Runtime>(label: &str, window: &WebviewWindow<R>) -> bool {
    set_shown(window, false)
        .inspect_err(|e| {
            log::warn!("could not hide {label}, destroying it: {e}");
            let _ = window.destroy();
        })
        .is_ok()
}

/// Shows `displays` again after [`hide`], focusing `focus`, and takes Esc back if Rust held it.
/// Closes the overlay instead if that fails, so it never stays open without a way out.
/// Main thread only.
pub(super) fn reshow<R: Runtime>(app: &AppHandle<R>, displays: &[u32], focus: u32) {
    if !sync_escape(app) {
        log::error!("could not take Esc back; closing the overlay");
        return close(app);
    }
    for &display in displays {
        let shown = app
            .get_webview_window(&label(display))
            .map(|w| set_shown(&w, true).map(|()| w));
        match shown {
            Some(Ok(w)) if display == focus => {
                let _ = w.set_focus();
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => {
                log::error!("could not show the overlay again: {e}");
                return close(app);
            }
            None => {
                log::error!("overlay {display} is gone; closing the overlay");
                return close(app);
            }
        }
    }
}

/// Forwards Esc to the pages, which know what to undo. Closes if nothing is shown yet, or if
/// no page answers within `timeout` (a hung page never locks the user out). Returns the Esc
/// token if it was forwarded.
fn escape<R: Runtime>(app: &AppHandle<R>, timeout: Duration) -> Option<u64> {
    // The Save dialog or share sheet is open: Esc is theirs.
    if phase(app).in_dialog() {
        return None;
    }
    let token = NEXT_ESCAPE.fetch_add(1, Ordering::Relaxed);
    let forwarded = phase(app).forward_escape(token);
    let Some(session) = forwarded else {
        close(app);
        return None;
    };
    let payload = EscapeEvent { session, token };
    for label in app.webview_windows().into_keys() {
        if display_id(&label).is_some()
            && let Err(e) = app.emit_to(label.as_str(), ESCAPE_EVENT, payload)
        {
            log::warn!("could not send Esc to {label}: {e}");
        }
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(timeout);
        on_main(&app, move |app| {
            let unhandled = phase(app).escape_unhandled(token);
            if unhandled {
                log::warn!("no overlay page handled Esc within {timeout:?}; closing");
                close(app);
            }
        });
    });
    Some(token)
}

/// Sent with `overlay:escape`; the page answers with the same token.
#[derive(Debug, Clone, Copy, serde::Serialize)]
struct EscapeEvent {
    session: u64,
    token: u64,
}

/// Takes (`true`) or releases (`false`) the Esc shortcut and returns whether that worked. The
/// app manages [`global_escape`]; tests manage a stand-in, since the OS allows one shortcut
/// manager per process.
pub struct EscapeKey<R: Runtime>(pub Box<SetEscape<R>>);

type SetEscape<R> = dyn Fn(&AppHandle<R>, bool) -> bool + Send + Sync;

/// Esc as a global shortcut, so it works even if the page is broken.
pub fn global_escape<R: Runtime>() -> EscapeKey<R> {
    EscapeKey(Box::new(set_global_escape))
}

/// Holds Esc exactly when the phase wants it ([`Phase::wants_escape`]). Returns whether Esc is
/// where the phase wants it. Call after every phase change. Main thread only.
pub(super) fn sync_escape<R: Runtime>(app: &AppHandle<R>) -> bool {
    let wants = phase(app).wants_escape();
    match app.try_state::<EscapeKey<R>>() {
        Some(key) => (key.0)(app, wants),
        // Only in tests without Esc.
        None => !wants,
    }
}

fn set_global_escape<R: Runtime>(app: &AppHandle<R>, on: bool) -> bool {
    let Some(shortcuts) = app.try_state::<GlobalShortcut<R>>() else {
        return false;
    };
    if on == shortcuts.is_registered(ESCAPE) {
        return true;
    }
    let result = if on {
        shortcuts.on_shortcut(ESCAPE, |app, _, event| {
            if event.state() == ShortcutState::Pressed {
                let _ = escape(app, ESCAPE_TIMEOUT);
            }
        })
    } else {
        shortcuts.unregister(ESCAPE)
    };
    if let Err(e) = result {
        log::warn!("could not update the Esc shortcut: {e}");
        return false;
    }
    true
}

fn ids(displays: &[Display]) -> Vec<u32> {
    displays.iter().map(|d| d.id).collect()
}

/// Matches the windows to the displays and moves each (still hidden) over its display.
fn place<R: Runtime>(app: &AppHandle<R>, displays: &[Display]) -> tauri::Result<()> {
    sync(app, &ids(displays))?;
    for d in displays {
        if let Some(window) = app.get_webview_window(&label(d.id)) {
            position(&window, d.bounds)?;
        }
    }
    Ok(())
}

/// Moves and sizes `window` to `b`, in its display's OS units (see `capture::Display`).
pub(crate) fn position<R: Runtime>(window: &WebviewWindow<R>, b: Bounds) -> tauri::Result<()> {
    if cfg!(target_os = "windows") {
        // Pixels. Move first: Windows rescales a window that moves to a monitor with another DPI.
        window.set_position(PhysicalPosition::new(b.x, b.y))?;
        window.set_size(PhysicalSize::new(b.width, b.height))
    } else {
        // Points. macOS resizes from the bottom-left corner, so size first, then move.
        window.set_size(LogicalSize::new(b.width, b.height))?;
        window.set_position(LogicalPosition::new(b.x, b.y))
    }
}

/// Creates and destroys overlay windows so there is one per display. Main thread only, so
/// startup and the hotkey never build the same window twice.
fn sync<R: Runtime>(app: &AppHandle<R>, displays: &[u32]) -> tauri::Result<()> {
    let windows = app.webview_windows();
    let d = diff(windows.keys().map(String::as_str), displays);
    for label in &d.destroy {
        if let Some(w) = windows.get(label) {
            w.destroy()?;
        }
    }
    for id in d.create {
        build(app, id)?;
    }
    Ok(())
}

fn build<R: Runtime>(app: &AppHandle<R>, display: u32) -> tauri::Result<WebviewWindow<R>> {
    // A new page: cold until it paints.
    if let Some(overlay) = app.try_state::<Overlay>() {
        lock(&overlay.warm).retain(|&d| d != display);
    }
    let url = WebviewUrl::App("overlay.html".into());
    let builder = WebviewWindowBuilder::new(app, label(display), url)
        .title("xcreenshot")
        .visible(false)
        .focused(false)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .background_color(tauri::window::Color(0, 0, 0, 255))
        // The page fetches and draws its frame while the window is hidden or transparent;
        // WebKit would otherwise throttle it.
        .background_throttling(BackgroundThrottlingPolicy::Disabled);
    // The first click on the overlay starts the selection instead of only focusing it.
    #[cfg(target_os = "macos")]
    let builder = builder.accept_first_mouse(true);
    let window = builder.build()?;
    // A user close (Alt+F4) closes the overlay and keeps the window for the next capture.
    // Quitting the app does not send CloseRequested, so it is never blocked.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            close(&handle);
        }
    });
    // The mock runtime used in tests has no real NSWindow.
    #[cfg(all(target_os = "macos", not(test)))]
    macos::raise(&window);
    Ok(window)
}

/// Stages a hidden overlay window: on screen but fully transparent and click-through, so its
/// webview renders the frame while nothing shows yet. A hidden webview renders nothing, so a
/// window shown as soon as the page drew would first show what it rendered last (black, or
/// the previous capture). The page reports the paint once the frame is rendered, and
/// [`set_shown`] unstages it (opaque, taking clicks again). Main thread only.
fn stage<R: Runtime>(window: &WebviewWindow<R>) {
    make_transparent(window, true);
}

/// Makes an overlay window transparent and click-through as it closes, where it is. Anything
/// that puts it back on screen before the next capture (AppKit, as a service picked in the
/// share sheet shows its UI) then shows nothing and takes no clicks, instead of its unpainted,
/// black page (#118). The next capture stages it again, and its paint unstages it. Main
/// thread only.
fn conceal<R: Runtime>(window: &WebviewWindow<R>) {
    make_transparent(window, false);
}

/// Click-through, then alpha 0 (see [`stage_steps`]), and on screen too if `order_in`. Listed
/// as staged, so it is unstaged before it shows.
fn make_transparent<R: Runtime>(window: &WebviewWindow<R>, order_in: bool) {
    let Some(display) = display_id(window.label()) else {
        return;
    };
    let staged = stage_steps(
        window.label(),
        |on| click_through(window, on),
        || present(window, true, order_in),
    );
    // A window already listed stays listed: it may still be transparent from before.
    let overlay = window.state::<Overlay>();
    let mut list = lock(&overlay.staged);
    if staged && !list.contains(&display) {
        list.push(display);
    }
}

/// Makes a staged window opaque and clickable again; does nothing to one that isn't staged.
/// Main thread only.
fn unstage<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
    let Some(display) = display_id(window.label()) else {
        return Ok(());
    };
    let overlay = window.state::<Overlay>();
    if !lock(&overlay.staged).contains(&display) {
        return Ok(());
    }
    unstage_steps(
        || present(window, false, false),
        |on| click_through(window, on),
    )
    .map_err(|e| {
        format!(
            "could not make {} opaque and clickable: {e}",
            window.label()
        )
    })?;
    lock(&overlay.staged).retain(|&d| d != display);
    Ok(())
}

/// Staging, in order: click-through, then alpha 0 (`present`, which puts it on screen when
/// staging for a paint). If click-through fails the window isn't staged: a transparent window
/// taking clicks would swallow them, so it shows opaque on its paint, as before staging. If it
/// can't turn transparent, click-through is undone. Returns whether the window must be
/// unstaged before it shows: staged, or still click-through because undoing that failed too.
fn stage_steps(
    label: &str,
    mut click_through: impl FnMut(bool) -> Result<(), String>,
    present: impl FnOnce() -> Result<(), String>,
) -> bool {
    if let Err(e) = click_through(true) {
        log::warn!("could not make {label} click-through; it shows when painted: {e}");
        return false;
    }
    let Err(e) = present() else {
        return true;
    };
    log::warn!("could not stage {label}; it shows when painted: {e}");
    click_through(false)
        .inspect_err(|e| log::warn!("could not make {label} clickable again: {e}"))
        .is_err()
}

/// Unstaging, in order: opaque, then taking clicks, each tried twice. An error means the
/// window would be invisible or ignore clicks, so the caller must not show it.
fn unstage_steps(
    mut opaque: impl FnMut() -> Result<(), String>,
    mut click_through: impl FnMut(bool) -> Result<(), String>,
) -> Result<(), String> {
    opaque().or_else(|_| opaque())?;
    click_through(false).or_else(|_| click_through(false))
}

fn click_through<R: Runtime>(window: &WebviewWindow<R>, on: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(on)
        .map_err(|e| e.to_string())
}

/// The platform half of staging: alpha 0 (`transparent`) and, if `order_in`, on screen
/// without focus; or opaque again.
fn present<R: Runtime>(
    window: &WebviewWindow<R>,
    transparent: bool,
    order_in: bool,
) -> Result<(), String> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        macos::stage(window, transparent, order_in)
    }
    #[cfg(all(target_os = "windows", not(test)))]
    {
        win::stage(window, transparent, order_in)
    }
    // The mock runtime has no native window.
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    {
        let _ = (window, transparent, order_in);
        Ok(())
    }
}

/// Window level and Spaces: `always_on_top` alone stays under the menu bar and the Dock.
#[cfg(all(target_os = "macos", not(test)))]
mod macos {
    use crate::macos::{SCREEN_SAVER_LEVEL, main_ns_window};
    use objc2_app_kit::NSWindowCollectionBehavior;
    use tauri::{Runtime, WebviewWindow};

    /// Alpha 0 or opaque; `order_in`: ordered in too, without taking focus.
    pub fn stage<R: Runtime>(
        window: &WebviewWindow<R>,
        transparent: bool,
        order_in: bool,
    ) -> Result<(), String> {
        let ns_window = main_ns_window(window).ok_or("no NSWindow on the main thread")?;
        ns_window.setAlphaValue(if transparent { 0.0 } else { 1.0 });
        if order_in {
            ns_window.orderFrontRegardless();
        }
        Ok(())
    }

    pub fn raise<R: Runtime>(window: &WebviewWindow<R>) {
        let Some(ns_window) = main_ns_window(window) else {
            return;
        };
        // Above the menu bar (24), the Dock (20) and pop-up menus (101).
        ns_window.setLevel(SCREEN_SAVER_LEVEL);
        // Over full-screen apps too. `visible_on_all_workspaces` already set CanJoinAllSpaces.
        ns_window.setCollectionBehavior(
            ns_window.collectionBehavior() | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
    }
}

#[cfg(all(target_os = "windows", not(test)))]
mod win {
    use tauri::{Runtime, WebviewWindow};
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::WindowsAndMessaging::{LWA_ALPHA, SetLayeredWindowAttributes};

    /// Alpha 0 (`transparent`) or 255. Click-through (`set_ignore_cursor_events`) already made
    /// the window layered, and Tauri keeps that style while it is set; opaque comes before
    /// click-through goes and with it the layered style. `order_in`: shown through Tauri, which
    /// activates it, so Tauri's own style updates keep it visible; the capture is already taken
    /// and the overlay takes focus anyway once revealed.
    pub fn stage<R: Runtime>(
        window: &WebviewWindow<R>,
        transparent: bool,
        order_in: bool,
    ) -> Result<(), String> {
        let hwnd = window.hwnd().map_err(|e| format!("no HWND: {e}"))?;
        let alpha = if transparent { 0 } else { 255 };
        // SAFETY: Tauri returns the live HWND of this window, and we are on its thread.
        unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA) }
            .map_err(|e| format!("could not set the alpha: {e}"))?;
        if order_in {
            window.show().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// The calling window's frame info, if its frame is waiting. Called by a window that loads
/// after `overlay:show` was sent (a display plugged in during the capture).
#[tauri::command]
pub(crate) fn overlay_ready<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Overlay>,
) -> Option<FrameInfo> {
    state.phase().pending_frame(display_id(window.label())?)
}

/// A copy of the calling window's frame as raw RGBA (the original stays for the crop); empty
/// if it was already sent or the session is gone. Async, so it runs off the main thread.
#[tauri::command]
pub(crate) async fn overlay_frame<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Overlay>,
    session: u64,
) -> Result<Response, ()> {
    let image = display_id(window.label()).and_then(|d| state.phase().frame_for_page(session, d));
    // Copied after the lock is released (about 1 ms for 3600x2338).
    Ok(Response::new(
        image.map(|i| i.as_raw().to_vec()).unwrap_or_default(),
    ))
}

/// A page answered Esc `token`: `handled` if it undid something. When every shown page had
/// nothing to undo, the overlay closes.
#[tauri::command]
pub(crate) fn overlay_escaped<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Overlay>,
    session: u64,
    token: u64,
    handled: bool,
) {
    let Some(display) = display_id(window.label()) else {
        return;
    };
    let close = state.phase().escape_reply(session, token, display, handled);
    if close {
        close_session(window.app_handle(), session);
    }
}

/// Esc reached the page because the global shortcut could not be registered: handle it the
/// same way.
#[tauri::command]
pub(crate) fn overlay_escape<R: Runtime>(app: AppHandle<R>) {
    let _ = escape(&app, ESCAPE_TIMEOUT);
}

/// The window rendered its frame: show it (only if its session is still open), focus the
/// display under the cursor, and log the time on the first paint. The page calls this only
/// once its frame is on screen, so the window never shows what the webview drew before.
/// `rendered` is false if the page gave up waiting for that and the window may flash.
#[tauri::command]
pub(crate) fn overlay_painted<R: Runtime>(
    window: WebviewWindow<R>,
    state: tauri::State<'_, Overlay>,
    session: u64,
    rendered: bool,
) {
    let Some(display) = display_id(window.label()) else {
        return;
    };
    let Some(paint) = state.phase().painted(session, display) else {
        return;
    };
    let mut warm = lock(&state.warm);
    if !warm.contains(&display) {
        warm.push(display);
    }
    drop(warm);
    if paint.hidden {
        return;
    }
    if !rendered {
        log::warn!("overlay {display} shown before its frame was rendered");
    }
    let app = window.app_handle();
    if let Err(e) = set_shown(&window, true) {
        log::error!("could not show the overlay: {e}");
        return close(app);
    }
    focus(app, paint.focus, paint.fallback);
    if let Some((total, capture)) = paint.first {
        let since_launch = lock(&LAUNCHED).take().map(|t| t.elapsed());
        log::info!("{}", paint_message(total, capture, since_launch));
    }
}

/// Focuses `display`'s window; if that fails, logs it and tries `fallback`'s.
fn focus<R: Runtime>(app: &AppHandle<R>, display: Option<u32>, fallback: Option<u32>) {
    let Some(display) = display else { return };
    let focus_one = |d: u32| {
        let focused = app
            .get_webview_window(&label(d))
            .ok_or_else(|| "no window".to_owned())
            .and_then(|w| w.set_focus().map_err(|e| e.to_string()));
        focused
            .inspect_err(|e| log::warn!("could not focus overlay {d}: {e}"))
            .is_ok()
    };
    if !focus_one(display)
        && let Some(fallback) = fallback
    {
        focus_one(fallback);
    }
}

/// The page gave up on `session` (it could not draw its frame).
#[tauri::command]
pub(crate) fn close_overlay<R: Runtime>(app: AppHandle<R>, session: u64) {
    close_session(&app, session);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::tests::frame;
    use super::*;
    use crate::region::Mode;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, mpsc};
    use tauri::ipc::{InvokeResponseBody, IpcResponse};
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    pub(crate) type Handle = AppHandle<MockRuntime>;

    /// Displays `ids`. Its capture waits for the test: `true` captures, `false` fails. Warm-ups
    /// call `warmed`.
    struct Fake {
        permitted: bool,
        ids: Vec<u32>,
        release: mpsc::Receiver<bool>,
        start: Start,
        warmed: Option<Box<dyn Fn() + Send>>,
        /// Counts the captures that opened with the quick copy tip.
        tips: Arc<AtomicUsize>,
    }

    impl Screen for Fake {
        fn permitted<R: Runtime>(&self, _: &AppHandle<R>) -> bool {
            self.permitted
        }

        fn displays(&self) -> Result<Vec<Display>, CaptureError> {
            if self.ids.is_empty() {
                return Err(CaptureError::NoDisplays);
            }
            Ok(self
                .ids
                .iter()
                .map(|&id| Display {
                    id,
                    bounds: frame(id, 4, 2).logical,
                })
                .collect())
        }

        fn capture(&self, ids: &[u32]) -> Result<Vec<DisplayFrame>, CaptureError> {
            match self.release.recv() {
                Ok(true) => Ok(ids.iter().map(|&id| frame(id, 4, 2)).collect()),
                _ => Err(CaptureError::EmptyImage),
            }
        }

        fn cursor<R: Runtime>(&self, _: &AppHandle<R>) -> Option<(f64, f64)> {
            None
        }

        fn start<R: Runtime>(&self, _: &AppHandle<R>) -> Start {
            self.start.clone()
        }

        fn tip_shown<R: Runtime>(&self, _: &AppHandle<R>) {
            self.tips.fetch_add(1, Ordering::SeqCst);
        }

        fn warm_up(&self) {
            if let Some(warmed) = &self.warmed {
                warmed();
            }
        }
    }

    fn fake(permitted: bool, ids: &[u32]) -> (Fake, mpsc::Sender<bool>) {
        let (tx, release) = mpsc::channel();
        let ids = ids.to_vec();
        (
            Fake {
                permitted,
                ids,
                release,
                start: Start {
                    regions: Some(Vec::new()),
                    mode: Mode::default(),
                },
                warmed: None,
                tips: Arc::default(),
            },
            tx,
        )
    }

    pub(crate) fn app() -> tauri::App<MockRuntime> {
        mock_builder()
            .manage(Overlay::default())
            .manage(crate::toast::Toasts::default())
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    /// Whether the stand-in Esc shortcut is held.
    #[derive(Default)]
    struct HeldEscape(std::sync::atomic::AtomicBool);

    /// An app whose Esc stand-in answers taking (`true`) or releasing (`false`) Esc with
    /// `works`.
    pub(crate) fn app_with_escape_by(
        works: impl Fn(bool) -> bool + Send + Sync + 'static,
    ) -> tauri::App<MockRuntime> {
        let key = EscapeKey::<MockRuntime>(Box::new(move |app, on| {
            let ok = works(on);
            if ok {
                app.state::<HeldEscape>().0.store(on, Ordering::Relaxed);
            }
            ok
        }));
        mock_builder()
            .manage(Overlay::default())
            .manage(crate::toast::Toasts::default())
            .manage(HeldEscape::default())
            .manage(key)
            .build(mock_context(noop_assets()))
            .unwrap()
    }

    /// An app that holds Esc (through a stand-in), like the real one.
    pub(crate) fn app_with_escape() -> tauri::App<MockRuntime> {
        app_with_escape_by(|_| true)
    }

    /// Whether the app holds Esc now.
    pub(crate) fn esc_held(app: &Handle) -> bool {
        app.try_state::<HeldEscape>()
            .is_some_and(|f| f.0.load(Ordering::Relaxed))
    }

    fn overlay_labels(app: &Handle) -> Vec<String> {
        let mut labels: Vec<_> = app
            .webview_windows()
            .into_keys()
            .filter(|l| display_id(l).is_some())
            .collect();
        labels.sort();
        labels
    }

    pub(crate) fn wait_until(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub(crate) fn is_open(app: &Handle, session: u64) -> bool {
        matches!(&*phase(app), Phase::Open(_)) && phase(app).session() == Some(session)
    }

    pub(crate) fn is_idle(app: &Handle) -> bool {
        matches!(*phase(app), Phase::Idle)
    }

    /// Presses the hotkey on fake displays; the capture finishes when `release` is sent.
    pub(crate) fn press(app: &Handle, ids: &[u32]) -> (u64, mpsc::Sender<bool>) {
        let (screen, release) = fake(true, ids);
        (start_with(app, screen).unwrap(), release)
    }

    /// Presses the hotkey and lets the capture finish.
    pub(crate) fn open(app: &Handle, ids: &[u32]) -> u64 {
        let (session, release) = press(app, ids);
        release.send(true).unwrap();
        wait_until("the session to open", || is_open(app, session));
        session
    }

    pub(crate) fn window(app: &Handle, display: u32) -> WebviewWindow<MockRuntime> {
        app.get_webview_window(&label(display)).unwrap()
    }

    pub(crate) fn fetch(app: &Handle, display: u32, session: u64) -> Vec<u8> {
        let state = app.state::<Overlay>();
        let response =
            tauri::async_runtime::block_on(overlay_frame(window(app, display), state, session));
        match response.unwrap().body().unwrap() {
            InvokeResponseBody::Raw(bytes) => bytes,
            InvokeResponseBody::Json(json) => panic!("expected raw bytes, got {json}"),
        }
    }

    #[test]
    fn prewarm_creates_a_window_per_display_then_warms_up_off_the_main_thread() {
        let app = app();
        let h = app.handle();
        // Each warm-up reports its thread and the overlay windows that existed when it ran.
        let screen = |ids: &[u32]| {
            let (tx, warm_ups) = mpsc::channel();
            let handle = h.clone();
            let warmed = move || {
                let _ = tx.send((std::thread::current().id(), overlay_labels(&handle)));
            };
            let screen = Fake {
                warmed: Some(Box::new(warmed)),
                ..fake(true, ids).0
            };
            (screen, warm_ups)
        };
        // Listing the displays fails: no windows, and nothing to warm up.
        let (failing, warm_ups) = screen(&[]);
        prewarm_with(h, failing);
        assert!(overlay_labels(h).is_empty());
        assert!(warm_ups.recv().is_err(), "no warm-up");
        // Listed: the windows first, then one warm-up on a worker thread.
        let (listed, warm_ups) = screen(&[4, 5]);
        prewarm_with(h, listed);
        assert_eq!(overlay_labels(h), ["overlay-4", "overlay-5"]);
        assert!(is_idle(h));
        let (thread, windows) = warm_ups.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_ne!(thread, std::thread::current().id());
        assert_eq!(windows, ["overlay-4", "overlay-5"]);
        assert!(warm_ups.recv().is_err(), "warmed up once");
    }
    #[test]
    fn hotkey_places_a_window_per_display_then_sends_frames() {
        let app = app();
        let h = app.handle();
        let (session, release) = press(h, &[1, 2]);
        // Windows are placed while the capture runs; nothing to draw yet.
        assert_eq!(overlay_labels(h), ["overlay-1", "overlay-2"]);
        assert_eq!(overlay_ready(window(h, 1), h.state()), None);
        release.send(true).unwrap();
        wait_until("the session to open", || is_open(h, session));
        let info = overlay_ready(window(h, 2), h.state()).unwrap();
        assert_eq!((info.session, info.width, info.height), (session, 4, 2));
        assert_eq!(fetch(h, 2, session), frame(2, 4, 2).image.into_raw());
        // Sent once; a second fetch (load check and show both firing) gets nothing.
        assert!(fetch(h, 2, session).is_empty());
        assert_eq!(overlay_ready(window(h, 2), h.state()), None);
        assert!(fetch(h, 1, session + 1).is_empty(), "other session");
    }

    #[test]
    fn hotkey_is_ignored_while_the_overlay_is_open() {
        let app = app();
        let h = app.handle();
        assert!(!super::super::is_busy(h));
        let (session, release) = press(h, &[1]);
        assert!(super::super::is_busy(h));
        assert_eq!(start_with(h, fake(true, &[1]).0), None, "while capturing");
        release.send(true).unwrap();
        wait_until("the session to open", || is_open(h, session));
        assert_eq!(start_with(h, fake(true, &[1]).0), None, "while open");
        assert_eq!(phase(h).session(), Some(session));
    }

    #[test]
    fn no_permission_or_no_displays_captures_nothing() {
        let app = app();
        let h = app.handle();
        assert_eq!(start_with(h, fake(false, &[1]).0), None);
        assert!(is_idle(h));
        assert!(overlay_labels(h).is_empty(), "no window before permission");
        assert_eq!(start_with(h, fake(true, &[]).0), None);
        assert!(is_idle(h));
    }

    #[test]
    fn a_failed_capture_closes_the_overlay() {
        let app = app();
        let h = app.handle();
        let ended = std::sync::Arc::new(AtomicU64::new(0));
        let count = ended.clone();
        tauri::Listener::listen_any(h, CAPTURE_CLOSED, move |_| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        let (_, release) = press(h, &[1]);
        release.send(false).unwrap();
        wait_until("the overlay to close", || is_idle(h));
        let failed = ended.load(Ordering::Relaxed);
        assert_eq!(failed, 0, "nothing was shown, so nothing was tried");
        open(h, &[1]);
    }

    #[test]
    fn a_remembered_region_opens_selected_on_its_display() {
        use crate::region::{Monitor, Region, arrangement, tests::rect};
        let app = app();
        let h = app.handle();
        let monitors = [Monitor::of(&frame(1, 4, 2)), Monitor::of(&frame(2, 4, 2))];
        let remembered = Region {
            arrangement: arrangement(&monitors),
            display: monitors[1],
            rect: rect(0.5, 0.0, 1.0, 1.0),
        };
        let (mut screen, release) = fake(true, &[1, 2]);
        screen.start.regions = Some(vec![remembered]);
        let session = start_with(h, screen).unwrap();
        release.send(true).unwrap();
        wait_until("the session to open", || is_open(h, session));
        assert_eq!(phase(h).pending_frame(1).unwrap().region, None);
        assert_eq!(
            phase(h).pending_frame(2).unwrap().region,
            Some(rect(0.5, 0.0, 1.0, 1.0))
        );
    }

    #[test]
    fn quick_copy_reaches_the_pages() {
        let app = app();
        let h = app.handle();
        let (mut screen, release) = fake(true, &[1]);
        screen.start.mode.quick_copy = true;
        let session = start_with(h, screen).unwrap();
        release.send(true).unwrap();
        wait_until("the session to open", || is_open(h, session));
        assert!(phase(h).pending_frame(1).unwrap().mode.quick_copy);
    }

    #[test]
    fn the_quick_copy_tip_reaches_the_pages_and_is_counted_once_open() {
        let app = app();
        let h = app.handle();
        let (mut screen, release) = fake(true, &[1]);
        screen.start.mode.quick_copy_tip = true;
        let tips = Arc::clone(&screen.tips);
        let session = start_with(h, screen).unwrap();
        release.send(true).unwrap();
        wait_until("the tip to be counted", || tips.load(Ordering::SeqCst) == 1);
        assert!(is_open(h, session));
        assert!(phase(h).pending_frame(1).unwrap().mode.quick_copy_tip);
        close(h);

        // A capture that fails never showed it.
        let (mut screen, release) = fake(true, &[1]);
        screen.start.mode.quick_copy_tip = true;
        let tips = Arc::clone(&screen.tips);
        start_with(h, screen).unwrap();
        release.send(false).unwrap();
        wait_until("the capture to fail", || is_idle(h));
        assert_eq!(tips.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_failure_from_an_older_capture_is_ignored() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1]);
        let failed = Err(CaptureError::NoDisplays);
        finish(
            h,
            session + 100,
            failed,
            None,
            true,
            Start::default(),
            WATCHDOG,
        );
        assert!(is_open(h, session));
    }

    #[test]
    fn painting_shows_only_the_current_session() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1, 2]);
        overlay_painted(window(h, 1), h.state(), session, true);
        assert!(!phase(h).stalled(session));
        close_overlay(h.clone(), session + 1);
        assert!(is_open(h, session), "another session is left alone");
        close_overlay(h.clone(), session);
        // A late paint from the closed session must not reopen anything.
        overlay_painted(window(h, 2), h.state(), session, false);
        assert!(is_idle(h));
    }

    #[test]
    fn only_the_open_sessions_unpainted_windows_are_staged() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1, 2]);
        overlay_painted(window(h, 1), h.state(), session, true);
        stage_unpainted(h, session, &[1, 2]);
        assert_eq!(staged(h), [2], "a painted window is not staged again");
        overlay_painted(window(h, 2), h.state(), session, true);
        assert_eq!(super::shown(h), [1, 2]);
        assert!(staged(h).is_empty(), "shown means unstaged");
        close(h);
        stage_unpainted(h, session, &[1, 2]);
        assert!(super::shown(h).is_empty(), "closed first");
    }

    #[test]
    fn closing_leaves_the_windows_transparent_until_the_next_paint() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1, 2]);
        overlay_painted(window(h, 1), h.state(), session, true);
        overlay_painted(window(h, 2), h.state(), session, true);
        close(h);
        // Nothing black if something puts them back on screen (#118).
        assert_eq!(staged(h), [1, 2], "concealed as they hide");
        let session = open(h, &[1]);
        overlay_painted(window(h, 1), h.state(), session, true);
        assert_eq!(super::shown(h), [1]);
        assert_eq!(staged(h), [2], "opaque again once painted");
    }

    /// A step that fails its first `fails` calls, recording each call's argument.
    fn step(
        fails: usize,
        calls: &std::cell::RefCell<Vec<bool>>,
    ) -> impl FnMut(bool) -> Result<(), String> {
        move |on| {
            calls.borrow_mut().push(on);
            if calls.borrow().len() <= fails {
                Err("no".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn staging_needs_click_through_first() {
        let clicks = Default::default();
        let mut presented = false;
        let staged = stage_steps("w", step(1, &clicks), || {
            presented = true;
            Ok(())
        });
        assert!(!staged, "never transparent while taking clicks");
        assert!(!presented);
        assert_eq!(*clicks.borrow(), [true]);
    }

    #[test]
    fn staging_that_cannot_go_on_screen_undoes_click_through() {
        let clicks = Default::default();
        assert!(stage_steps("w", step(0, &clicks), || Ok(())), "staged");
        let clicks = Default::default();
        assert!(!stage_steps("w", step(0, &clicks), || Err("no".into())));
        assert_eq!(*clicks.borrow(), [true, false], "clickable again");
        // Still click-through: it must be unstaged before it shows.
        let undo_fails = |on: bool| if on { Ok(()) } else { Err("no".to_owned()) };
        assert!(stage_steps("w", undo_fails, || Err("no".into())));
    }

    #[test]
    fn unstaging_retries_each_step_once_then_fails() {
        let run = |opaque_fails: usize, click_fails: usize| {
            let mut opaque_calls = 0;
            let clicks = Default::default();
            let result = unstage_steps(
                || {
                    opaque_calls += 1;
                    if opaque_calls <= opaque_fails {
                        Err("no".into())
                    } else {
                        Ok(())
                    }
                },
                step(click_fails, &clicks),
            );
            (result.is_ok(), opaque_calls, clicks.borrow().len())
        };
        assert_eq!(run(0, 0), (true, 1, 1));
        assert_eq!(run(1, 1), (true, 2, 2), "one retry each");
        assert_eq!(run(2, 0), (false, 2, 0), "never clickable while invisible");
        assert_eq!(run(0, 2), (false, 1, 2));
    }

    #[test]
    fn the_save_dialog_leaves_a_staged_window_rendering() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1, 2]);
        stage_unpainted(h, session, &[1, 2]);
        overlay_painted(window(h, 1), h.state(), session, true);
        assert!(phase(h).begin_save(session));
        hide(h);
        assert!(super::shown(h).is_empty());
        assert_eq!(staged(h), [2], "still staged, so it goes on rendering");
        overlay_painted(window(h, 2), h.state(), session, true);
        assert_eq!(
            staged(h),
            [2],
            "painted during the dialog: shown with the others"
        );
        let displays = phase(h).end_save(session).unwrap();
        reshow(h, &displays, 1);
        assert_eq!(super::shown(h), [1, 2]);
        assert!(staged(h).is_empty());
    }

    #[test]
    fn closing_frees_the_frames_and_allows_the_next_capture() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1]);
        close(h);
        assert!(fetch(h, 1, session).is_empty());
        // The windows stay, hidden, for the next capture.
        assert_eq!(overlay_labels(h), ["overlay-1"]);
        open(h, &[1]);
    }

    #[test]
    fn closing_a_capture_hands_focus_back_once_to_the_app_in_front() {
        use crate::focus::tests::{Fake, SAFARI};
        let fake = Fake::new(Some(SAFARI));
        let app = mock_builder()
            .manage(Overlay::default())
            .manage(focus::Focus::new(fake.clone()))
            .build(mock_context(noop_assets()))
            .unwrap();
        let h = app.handle();
        // Nothing else open: hide, as before.
        open(h, &[1]);
        close(h);
        assert_eq!(fake.calls(), ["hide"]);
        // Settings open behind Safari: Safari comes back, not Settings.
        WebviewWindowBuilder::new(h, "settings", WebviewUrl::default())
            .build()
            .unwrap();
        open(h, &[1]);
        close(h);
        assert_eq!(fake.calls(), ["hide", "activate 7"]);
        // A second close of the same capture leaves focus alone.
        close(h);
        assert_eq!(fake.calls().len(), 2);
        // Started from Settings: Settings keeps focus.
        h.state::<focus::Focus>().activated(focus::tests::us());
        open(h, &[1]);
        close(h);
        assert_eq!(fake.calls().len(), 2);
    }

    #[test]
    fn a_capture_finished_after_esc_shows_nothing() {
        let app = app();
        let h = app.handle();
        let (session, release) = press(h, &[1]);
        close(h);
        release.send(true).unwrap();
        // The worker finishes on its own; give it time, then check nothing opened.
        std::thread::sleep(Duration::from_millis(50));
        assert!(is_idle(h));
        assert_eq!(overlay_ready(window(h, 1), h.state()), None);
        close_session(h, session);
        assert!(is_idle(h));
    }

    #[test]
    fn close_session_leaves_a_newer_session_alone() {
        let app = app();
        let h = app.handle();
        let ended = std::sync::Arc::new(AtomicU64::new(0));
        let count = ended.clone();
        tauri::Listener::listen_any(h, CAPTURE_CLOSED, move |_| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        let session = open(h, &[1]);
        close_session(h, session + 1);
        assert!(is_open(h, session));
        assert_eq!(ended.load(Ordering::Relaxed), 0);
        close_session(h, session);
        assert!(is_idle(h));
        let sent = ended.load(Ordering::Relaxed);
        assert_eq!(sent, 1, "sent by the close that ended it");
        close(h);
        let again = ended.load(Ordering::Relaxed);
        assert_eq!(again, 1, "not again when nothing was open");
    }

    #[test]
    fn watchdog_closes_an_overlay_that_never_painted() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1, 2]);
        rescue(h, session + 1);
        assert!(is_open(h, session), "other session");
        rescue(h, session);
        assert!(is_idle(h));

        let session = open(h, &[1]);
        overlay_painted(window(h, 1), h.state(), session, true);
        rescue(h, session);
        assert!(is_open(h, session), "painted in time");
    }

    #[test]
    fn unreported_windows_are_shown_after_the_page_fallback_and_before_the_watchdog() {
        // After the page's own fallback (`PAINT_TIMEOUT_MS` in paint.ts, 200 ms, or 600 ms
        // for its first draw: `FIRST_PAINT_TIMEOUT_MS`), with room for fetching the frame.
        assert!(REVEAL_AFTER > Duration::from_millis(200) && REVEAL_AFTER < WATCHDOG);
        assert!(FIRST_REVEAL_AFTER >= Duration::from_millis(900) && FIRST_REVEAL_AFTER < WATCHDOG);
        assert_eq!(reveal_after(WATCHDOG, false), REVEAL_AFTER);
        assert_eq!(
            reveal_after(WATCHDOG, true),
            FIRST_REVEAL_AFTER,
            "a cold webview gets longer"
        );
        for cold in [false, true] {
            assert_eq!(
                reveal_after(Duration::ZERO, cold),
                Duration::ZERO,
                "never after the watchdog"
            );
        }

        let app = app();
        let h = app.handle();
        let session = open(h, &[1, 2]);
        stage_unpainted(h, session, &[1, 2]);
        overlay_painted(window(h, 1), h.state(), session, true);
        reveal_late(h, session + 1);
        assert_eq!(super::shown(h), [1], "other session");
        reveal_late(h, session);
        assert_eq!(super::shown(h), [1, 2], "shown though it never reported");
        assert!(staged(h).is_empty());
        overlay_painted(window(h, 2), h.state(), session, true);
        assert_eq!(super::shown(h), [1, 2], "a late report changes nothing");

        // Nothing reported: shown, but the watchdog still closes a dead page.
        close(h);
        let session = open(h, &[1]);
        reveal_late(h, session);
        assert_eq!(super::shown(h), [1]);
        rescue(h, session);
        assert!(is_idle(h));
    }

    #[test]
    fn a_window_is_cold_until_it_paints() {
        assert!(is_cold(&[], &[1]));
        assert!(is_cold(&[1], &[1, 2]), "a display added since");
        assert!(!is_cold(&[2, 1], &[1, 2]));

        let app = app();
        let h = app.handle();
        let warm = || lock(&h.state::<Overlay>().warm).clone();
        let session = open(h, &[1, 2]);
        overlay_painted(window(h, 1), h.state(), session, true);
        overlay_painted(window(h, 1), h.state(), session, true);
        assert_eq!(warm(), [1], "2 hasn't painted");
        overlay_painted(window(h, 2), h.state(), session, false);
        assert_eq!(warm(), [1, 2], "reported, even if not rendered");
        // A window built again is a new page.
        build(h, 2).ok();
        assert_eq!(warm(), [1]);
    }

    #[test]
    fn display_changes_rebuild_the_windows() {
        let app = app();
        let h = app.handle();
        WebviewWindowBuilder::new(h, "settings", WebviewUrl::default())
            .build()
            .unwrap();
        sync(h, &[1, 2]).unwrap();
        sync(h, &[2, 1]).unwrap();
        assert_eq!(overlay_labels(h), ["overlay-1", "overlay-2"]);
        open(h, &[2, 3]);
        // The mock runtime never reports a window destroyed, so overlay-1 stays listed here;
        // which windows get destroyed is covered by the `diff` tests.
        assert!(overlay_labels(h).contains(&"overlay-3".to_owned()));
        assert!(h.get_webview_window("settings").is_some());
    }

    #[test]
    fn esc_is_held_only_while_the_overlay_is_open() {
        // The one test with the real global shortcut (one manager per process).
        let app = mock_builder()
            .plugin(tauri_plugin_global_shortcut::Builder::new().build())
            .manage(Overlay::default())
            .manage(global_escape::<MockRuntime>())
            .build(mock_context(noop_assets()))
            .unwrap();
        let h = app.handle();
        let shortcuts = h.state::<GlobalShortcut<MockRuntime>>();
        let session = open(h, &[1]);
        assert!(shortcuts.is_registered(ESCAPE));
        assert!(sync_escape(h), "already held");
        close_session(h, session);
        assert!(!shortcuts.is_registered(ESCAPE));
        close(h);
        assert!(!shortcuts.is_registered(ESCAPE));
    }

    /// Opens a session on `ids` with every window painted.
    pub(crate) fn shown(app: &Handle, ids: &[u32]) -> u64 {
        let session = open(app, ids);
        for &id in ids {
            overlay_painted(window(app, id), app.state(), session, true);
        }
        session
    }

    #[test]
    fn esc_handled_by_a_page_keeps_the_overlay_open() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1, 2]);
        let token = escape(h, Duration::from_millis(20)).unwrap();
        overlay_escaped(window(h, 1), h.state(), session, token, false);
        overlay_escaped(window(h, 2), h.state(), session, token, true);
        std::thread::sleep(Duration::from_millis(60));
        assert!(is_open(h, session));
    }

    #[test]
    fn esc_closes_when_no_page_had_anything_to_undo() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1, 2]);
        let token = escape(h, WATCHDOG).unwrap();
        overlay_escaped(window(h, 1), h.state(), session, token, false);
        assert!(is_open(h, session), "display 2 has not answered");
        overlay_escaped(window(h, 2), h.state(), session, token, false);
        assert!(is_idle(h));
    }

    #[test]
    fn esc_closes_when_no_page_answers() {
        let app = app();
        let h = app.handle();
        let session = shown(h, &[1]);
        assert!(escape(h, Duration::from_millis(10)).is_some());
        assert!(is_open(h, session), "the pages get a chance first");
        wait_until("the overlay to close", || is_idle(h));
    }

    #[test]
    fn esc_before_anything_is_shown_closes_at_once() {
        let app = app();
        let h = app.handle();
        let (_, release) = press(h, &[1]);
        assert_eq!(escape(h, WATCHDOG), None);
        assert!(is_idle(h));
        release.send(true).unwrap();
        open(h, &[1]);
        // The page's own Esc key (no global shortcut) takes the same path.
        overlay_escape(h.clone());
        assert!(is_idle(h), "open but not painted");
    }

    #[test]
    fn calls_from_other_windows_are_ignored() {
        let app = app();
        let h = app.handle();
        let session = open(h, &[1]);
        let settings = WebviewWindowBuilder::new(h, "settings", WebviewUrl::default())
            .build()
            .unwrap();
        assert_eq!(overlay_ready(settings.clone(), h.state()), None);
        overlay_painted(settings, h.state(), session, true);
        assert!(phase(h).stalled(session));
    }
}
