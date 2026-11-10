//! The frozen-frame overlay: one prewarmed window per display, shown on the capture hotkey.
//!
//! This file is pure logic: window labels, window diffing, finding the display under the
//! cursor and the overlay's state ([`Phase`]). The Tauri calls live in `window.rs`.
//!
//! Rust keeps each original frame while the overlay is open and sends the page a copy, so the
//! crop is cut from the exact captured pixels (see `docs/spikes/capture.md`).

#[cfg(debug_assertions)]
pub mod debug;
/// Public to the crate so `generate_handler!` can reach the commands.
pub(crate) mod export;
/// Public to the crate so `generate_handler!` can reach the commands.
pub(crate) mod window;

pub use window::{Overlay, global_escape, prewarm, start_capture};

use crate::capture::{Density, Display, DisplayFrame, PixelRect};
use crate::focus::Front;
use crate::region::{self, LogicalRect, Mode, Monitor, Region};
use image::RgbaImage;
use std::sync::Arc;
use std::time::{Duration, Instant};

const LABEL_PREFIX: &str = "overlay-";

/// Window label for a display's overlay.
pub fn label(display: u32) -> String {
    format!("{LABEL_PREFIX}{display}")
}

/// The display id in an overlay window label, or `None` for any other window.
pub fn display_id(label: &str) -> Option<u32> {
    label.strip_prefix(LABEL_PREFIX)?.parse().ok()
}

/// Whether a capture is running or the overlay is open. Read-only; the update prompt waits
/// for this to clear so it never interrupts a capture.
pub fn is_busy<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    front(app).is_some()
}

/// The app in front at the hotkey of the capture or session under way; `None` if neither is.
pub fn front<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<Front> {
    window::phase(app).front()
}

/// Overlay windows to create and destroy so there is exactly one per display.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WindowDiff {
    pub create: Vec<u32>,
    pub destroy: Vec<String>,
}

/// Compares the open window labels with the current displays (already unique). Labels that
/// are not overlay windows (Settings) are left alone.
pub fn diff<'a>(labels: impl IntoIterator<Item = &'a str>, displays: &[u32]) -> WindowDiff {
    let mut existing = Vec::new();
    let mut destroy = Vec::new();
    for label in labels {
        match display_id(label) {
            Some(id) if displays.contains(&id) => existing.push(id),
            Some(_) => destroy.push(label.to_owned()),
            None => {}
        }
    }
    let create = displays
        .iter()
        .copied()
        .filter(|id| !existing.contains(id))
        .collect();
    WindowDiff { create, destroy }
}

/// The display containing a point, in the displays' OS units.
pub fn display_at(displays: &[Display], x: f64, y: f64) -> Option<u32> {
    let inside = |start: i32, len: u32, v: f64| {
        let start = f64::from(start);
        v >= start && v < start + f64::from(len)
    };
    displays
        .iter()
        .find(|d| {
            let b = d.bounds;
            inside(b.x, b.width, x) && inside(b.y, b.height, y)
        })
        .map(|d| d.id)
}

/// Whether every listed display has a frame. If one failed, the frames don't make up the whole
/// arrangement, so no region is restored or remembered.
pub fn captured_all(displays: &[Display], frames: &[DisplayFrame]) -> bool {
    displays.iter().all(|d| frames.iter().any(|f| f.id == d.id))
}

/// What the overlay window needs to draw a frame; sent with `overlay:show`. The page maps its
/// logical (CSS) px to the frame's pixels with `width / logical_width`, the same scale Rust
/// uses, and sends the crop as a pixel rect.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameInfo {
    pub session: u64,
    pub width: u32,
    pub height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    /// Whether Rust owns Esc (the global shortcut registered). If not, the page forwards its
    /// own Esc key to Rust instead.
    pub escape_shortcut: bool,
    /// The remembered region on this display, in logical px: the page starts with it selected.
    pub region: Option<LogicalRect>,
    /// Quick copy, and the quick copy tip.
    #[serde(flatten)]
    pub mode: Mode,
}

impl FrameInfo {
    /// That same scale, for the PNG's pixel density.
    pub fn density(&self) -> Density {
        Density::of(
            (self.width, self.height),
            (self.logical_width, self.logical_height),
        )
    }
}

/// The overlay's whole state. One capture at a time: the hotkey is ignored unless `Idle`.
#[derive(Debug, Default)]
pub enum Phase {
    #[default]
    Idle,
    Capturing {
        session: u64,
        started: Instant,
        /// Who was in front at the hotkey, to decide where focus goes on close.
        front: Front,
    },
    Open(Session),
}

#[derive(Debug)]
pub struct Session {
    id: u64,
    started: Instant,
    capture_ms: f64,
    slots: Vec<Slot>,
    /// The display that gets keyboard focus: the one with the remembered region, else the one
    /// under the cursor.
    focus: Option<u32>,
    /// The display under the cursor, if it has a frame: focused instead if `focus` can't be.
    cursor: Option<u32>,
    /// Every display was captured, so the region can be remembered (see [`Phase::open`]).
    remember: bool,
    painted: Vec<u32>,
    /// The Esc forwarded to the pages that is still waiting for their answers.
    escape: Option<EscapeWait>,
    /// The Save dialog or share sheet that owns the session while it is open.
    dialog: Option<Dialog>,
    /// Saves started in this session, so a slow copy can tell whether one began meanwhile.
    saves: u32,
    /// Whether Rust held Esc when the session opened (see [`Phase::wants_escape`]).
    escape_shortcut: bool,
    front: Front,
}

/// A dialog over the overlay. One at a time; Esc is its while it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialog {
    /// The Save dialog: the windows are hidden.
    Save,
    /// The share sheet: the windows stay.
    Share,
}

/// One Esc press, until a page handles it or every shown page says it had nothing to undo.
#[derive(Debug)]
struct EscapeWait {
    token: u64,
    /// Shown displays whose page had nothing to undo.
    declined: Vec<u32>,
}

/// One display's original frame, kept for the crop. Its window gets a copy, once.
#[derive(Debug)]
struct Slot {
    display: u32,
    monitor: Monitor,
    info: FrameInfo,
    image: Arc<RgbaImage>,
    sent: bool,
}

/// What to do when a window reports it painted.
#[derive(Debug, PartialEq)]
pub struct Paint {
    /// Window to focus after showing this one.
    pub focus: Option<u32>,
    /// Window to focus if `focus` can't be: the one under the cursor.
    pub fallback: Option<u32>,
    /// Set on the first paint of the session: hotkey-to-paint and capture time, in ms.
    pub first: Option<(f64, f64)>,
    /// The Save dialog is open: keep the window hidden; it shows when the dialog closes.
    pub hidden: bool,
}

fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// The log line for a session's first paint: hotkey to paint and the capture's part, in ms.
/// `since_launch` is set for the first capture since launch, which is slower (#55): the line
/// says so, and how long after launch it came, since that changes its time.
pub fn paint_message(total: f64, capture: f64, since_launch: Option<Duration>) -> String {
    let line = format!("overlay painted {total:.0} ms after the hotkey (capture {capture:.0} ms)");
    match since_launch {
        Some(d) => format!(
            "{line}; first capture since launch, {} s after it",
            d.as_secs()
        ),
        None => line,
    }
}

impl Phase {
    /// Starts a capture. Returns `false` (and changes nothing) unless idle.
    pub fn begin(&mut self, session: u64, started: Instant) -> bool {
        if !matches!(self, Phase::Idle) {
            return false;
        }
        *self = Phase::Capturing {
            session,
            started,
            front: Front::Other,
        };
        true
    }

    /// Records who was in front for the capture under way, which may have opened already.
    pub fn set_front(&mut self, app: Front) {
        match self {
            Phase::Idle => {}
            Phase::Capturing { front, .. } => *front = app,
            Phase::Open(s) => s.front = app,
        }
    }

    /// The app in front at the hotkey of the capture or session under way, if any.
    pub fn front(&self) -> Option<Front> {
        match self {
            Phase::Idle => None,
            Phase::Capturing { front, .. } => Some(*front),
            Phase::Open(s) => Some(s.front),
        }
    }

    /// Ends the capture or session, if any, and returns the app that was in front at its
    /// hotkey: `None` if there was nothing to end.
    pub fn end(&mut self) -> Option<Front> {
        let front = self.front();
        *self = Phase::Idle;
        front
    }

    /// The capture finished. Opens the session if this capture is still current (Esc may have
    /// closed it) and returns each display's frame info to send. `focus` is the display under
    /// the cursor; it gets keyboard focus if it was captured, unless a display starts with a
    /// remembered region from `regions`, which then does. `regions` is `None` when not every
    /// display was captured: the key would name another arrangement, so nothing is restored or
    /// remembered. `escape_shortcut` tells the pages whether Rust owns Esc, and `mode`
    /// whether the drag copies at once and whether the quick copy tip shows.
    pub fn open(
        &mut self,
        session: u64,
        frames: Vec<DisplayFrame>,
        focus: Option<u32>,
        escape_shortcut: bool,
        regions: Option<&[Region]>,
        mode: Mode,
    ) -> Option<Vec<(u32, FrameInfo)>> {
        let Phase::Capturing {
            session: id,
            started,
            front,
        } = *self
        else {
            return None;
        };
        if id != session {
            return None;
        }
        let monitors: Vec<Monitor> = frames.iter().map(Monitor::of).collect();
        let arrangement = region::arrangement(&monitors);
        let slots: Vec<Slot> = frames
            .into_iter()
            .zip(monitors)
            .map(|(f, monitor)| {
                let (width, height) = f.image.dimensions();
                let info = FrameInfo {
                    session,
                    width,
                    height,
                    logical_width: f.logical.width,
                    logical_height: f.logical.height,
                    escape_shortcut,
                    region: regions.and_then(|r| region::find(r, &arrangement, &monitor)),
                    mode,
                };
                Slot {
                    display: f.id,
                    monitor,
                    info,
                    image: Arc::new(f.image),
                    sent: false,
                }
            })
            .collect();
        let cursor = focus.filter(|&d| slots.iter().any(|s| s.display == d));
        let restored = slots.iter().find(|s| s.info.region.is_some());
        let focus = restored.map(|s| s.display).or(cursor);
        let shows = slots.iter().map(|s| (s.display, s.info)).collect();
        *self = Phase::Open(Session {
            id: session,
            started,
            capture_ms: ms_since(started),
            slots,
            focus,
            cursor,
            remember: regions.is_some(),
            painted: Vec::new(),
            escape: None,
            dialog: None,
            saves: 0,
            escape_shortcut,
            front,
        });
        Some(shows)
    }

    /// The current session id, capturing or open.
    pub fn session(&self) -> Option<u64> {
        match self {
            Phase::Idle => None,
            Phase::Capturing { session, .. } => Some(*session),
            Phase::Open(s) => Some(s.id),
        }
    }

    /// The open session, if it is `session`.
    fn current(&mut self, session: u64) -> Option<&mut Session> {
        match self {
            Phase::Open(s) if s.id == session => Some(s),
            _ => None,
        }
    }

    fn slot(&mut self, session: u64, display: u32) -> Option<&mut Slot> {
        let s = self.current(session)?;
        s.slots.iter_mut().find(|slot| slot.display == display)
    }

    /// A display's frame info while its frame is not sent yet, for a window that loaded after
    /// `overlay:show` was sent.
    pub fn pending_frame(&self, display: u32) -> Option<FrameInfo> {
        let Phase::Open(s) = self else { return None };
        s.slots
            .iter()
            .find(|slot| slot.display == display && !slot.sent)
            .map(|slot| slot.info)
    }

    /// A display's frame for its window, once. The caller copies it outside the lock; the
    /// original stays here for the crop.
    pub fn frame_for_page(&mut self, session: u64, display: u32) -> Option<Arc<RgbaImage>> {
        let slot = self.slot(session, display)?;
        if slot.sent {
            return None;
        }
        slot.sent = true;
        Some(Arc::clone(&slot.image))
    }

    /// The region to remember for a selection of `rect` (frame px) on `display`. `None` if a
    /// display was not captured.
    pub fn region(&mut self, session: u64, display: u32, rect: PixelRect) -> Option<Region> {
        let s = self.current(session).filter(|s| s.remember)?;
        let slot = s.slots.iter().find(|slot| slot.display == display)?;
        let monitors: Vec<Monitor> = s.slots.iter().map(|slot| slot.monitor).collect();
        Some(Region {
            arrangement: region::arrangement(&monitors),
            display: slot.monitor,
            rect: slot.monitor.to_logical(rect),
        })
    }

    /// A display's original frame, to crop outside the lock, and its pixel density.
    pub fn image(&mut self, session: u64, display: u32) -> Option<(Arc<RgbaImage>, Density)> {
        self.slot(session, display)
            .map(|slot| (Arc::clone(&slot.image), slot.info.density()))
    }

    /// Esc was pressed. Returns the session if Esc goes to its pages (one may have a size edit
    /// or a selection to undo), `None` if nothing is shown yet and the overlay should close.
    pub fn forward_escape(&mut self, token: u64) -> Option<u64> {
        let Phase::Open(s) = self else { return None };
        if s.painted.is_empty() {
            return None;
        }
        s.escape = Some(EscapeWait {
            token,
            declined: Vec::new(),
        });
        Some(s.id)
    }

    /// A page answered Esc `token`: it undid something (`handled`) or had nothing to undo.
    /// Returns `true` when every shown page had nothing to undo, so the overlay should close.
    /// Answers to an older Esc or session are ignored.
    pub fn escape_reply(&mut self, session: u64, token: u64, display: u32, handled: bool) -> bool {
        let Some(s) = self.current(session) else {
            return false;
        };
        let Some(wait) = s.escape.as_mut().filter(|w| w.token == token) else {
            return false;
        };
        if handled {
            s.escape = None;
            return false;
        }
        if s.painted.contains(&display) && !wait.declined.contains(&display) {
            wait.declined.push(display);
        }
        let close = s.painted.iter().all(|d| wait.declined.contains(d));
        if close {
            s.escape = None;
        }
        close
    }

    /// Whether Esc `token` is still unanswered; if so, the pages are not responding.
    pub fn escape_unhandled(&mut self, token: u64) -> bool {
        let Phase::Open(s) = self else { return false };
        s.escape.take_if(|w| w.token == token).is_some()
    }

    /// A window drew its frame. `None` if the session is gone or it already painted.
    pub fn painted(&mut self, session: u64, display: u32) -> Option<Paint> {
        let s = self.current(session)?;
        if !s.slots.iter().any(|slot| slot.display == display) || s.painted.contains(&display) {
            return None;
        }
        let first = s
            .painted
            .is_empty()
            .then(|| (ms_since(s.started), s.capture_ms));
        s.painted.push(display);
        let focus = match s.focus {
            Some(f) if s.painted.contains(&f) => Some(f),
            Some(_) => None,
            None => s.painted.first().copied(),
        };
        // Only for a focus being made now, never while waiting for the focused display.
        let fallback = focus
            .and(s.cursor)
            .filter(|c| Some(*c) != focus && s.painted.contains(c));
        Some(Paint {
            focus,
            fallback,
            first,
            hidden: s.dialog == Some(Dialog::Save),
        })
    }

    /// The open session `session` if it is shown and neither saving nor sharing, so a save or
    /// share may start.
    fn idle_shown(&mut self, session: u64) -> Option<&mut Session> {
        self.current(session)
            .filter(|s| s.dialog.is_none() && !s.painted.is_empty())
    }

    /// Starts a save in `session`: its windows hide while the Save dialog is open. `false`
    /// unless the session is open, shown and not already saving or sharing.
    pub fn begin_save(&mut self, session: u64) -> bool {
        let Some(s) = self.idle_shown(session) else {
            return false;
        };
        s.dialog = Some(Dialog::Save);
        s.saves += 1;
        s.escape = None;
        true
    }

    /// Opens the share sheet in `session`: Esc is the sheet's until it closes. `false` unless
    /// the session is open, shown and not already saving or sharing.
    pub fn begin_share(&mut self, session: u64) -> bool {
        let Some(s) = self.idle_shown(session) else {
            return false;
        };
        s.dialog = Some(Dialog::Share);
        s.escape = None;
        true
    }

    /// The share sheet closed. Returns whether `session` was the one sharing.
    pub fn end_share(&mut self, session: u64) -> bool {
        self.current(session)
            .is_some_and(|s| s.dialog.take_if(|d| *d == Dialog::Share).is_some())
    }

    /// The Save dialog closed. Returns the displays to show again if `session` is still the
    /// one saving (one lock, so nothing can close it in between), or `None`.
    pub fn end_save(&mut self, session: u64) -> Option<Vec<u32>> {
        let s = self.current(session)?;
        s.dialog.take_if(|d| *d == Dialog::Save)?;
        Some(s.painted.clone())
    }

    /// Whether Rust should hold Esc now: while capturing, and while the overlay is open unless
    /// the Save dialog or share sheet is up (Esc is theirs) or Esc could not be registered at
    /// the start (the pages forward their own Esc instead, so holding it would act twice).
    pub fn wants_escape(&self) -> bool {
        match self {
            Phase::Idle => false,
            Phase::Capturing { .. } => true,
            Phase::Open(s) => s.escape_shortcut && s.dialog.is_none(),
        }
    }

    /// Whether the Save dialog is open (the windows are hidden). Tests tell it from sharing.
    #[cfg(test)]
    pub fn saving(&self) -> bool {
        matches!(self, Phase::Open(s) if s.dialog == Some(Dialog::Save))
    }

    /// Whether the Save dialog or the share sheet is open, so Esc is theirs.
    pub fn in_dialog(&self) -> bool {
        matches!(self, Phase::Open(s) if s.dialog.is_some())
    }

    /// How many saves `session` has started, or `None` unless it is open and neither saving nor
    /// sharing. A copy notes it when it starts.
    pub fn saves(&self, session: u64) -> Option<u32> {
        match self {
            Phase::Open(s) if s.id == session && s.dialog.is_none() => Some(s.saves),
            _ => None,
        }
    }

    /// Whether a copy that started at `saves` may close `session`: it is still open, neither
    /// saving nor sharing, and no save started since (a save, even cancelled, means the user
    /// moved on).
    pub fn copy_may_close(&self, session: u64, saves: u32) -> bool {
        self.saves(session) == Some(saves)
    }

    /// Whether this session is still open with nothing painted (the overlay page is broken).
    pub fn stalled(&self, session: u64) -> bool {
        matches!(self, Phase::Open(s) if s.id == session && s.painted.is_empty())
    }

    /// The displays of `session` whose page hasn't reported its paint, to show anyway, and the
    /// one to focus if nothing painted yet. `None` if there are none, the session closed, or
    /// the Save dialog is open (its windows stay hidden until it closes).
    pub fn unreported(&self, session: u64) -> Option<(Vec<u32>, Option<u32>)> {
        let Phase::Open(s) = self else {
            return None;
        };
        if s.id != session || s.dialog == Some(Dialog::Save) {
            return None;
        }
        let late: Vec<u32> = s
            .slots
            .iter()
            .map(|slot| slot.display)
            .filter(|d| !s.painted.contains(d))
            .collect();
        let focus = s.painted.is_empty().then(|| {
            s.focus
                .or(s.cursor)
                .filter(|d| late.contains(d))
                .or(late.first().copied())
        });
        (!late.is_empty()).then_some((late, focus.flatten()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::capture::{Bounds, PixelRect};

    /// A display frame of `width` x `height` pixels at 2x, placed at x = id * 1000 points.
    pub(crate) fn frame(id: u32, width: u32, height: u32) -> DisplayFrame {
        let image = RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([x as u8, y as u8, 9, 255])
        });
        let x = id as i32 * 1000;
        DisplayFrame {
            id,
            logical: Bounds {
                x,
                y: 0,
                width: width / 2,
                height: height / 2,
            },
            physical: Bounds {
                x: x * 2,
                y: 0,
                width,
                height,
            },
            scale_factor: 2.0,
            image,
        }
    }

    pub(crate) fn px(x: u32, y: u32, width: u32, height: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }

    fn bounds(x: i32, y: i32, width: u32, height: u32) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    /// An open session on displays 1 and 2 (4x2 px frames), `focus` under the cursor.
    fn open(focus: Option<u32>) -> Phase {
        let mut p = Phase::default();
        assert!(p.begin(7, Instant::now()));
        p.open(
            7,
            vec![frame(1, 4, 2), frame(2, 4, 2)],
            focus,
            true,
            Some(&[]),
            Mode::default(),
        )
        .unwrap();
        p
    }

    #[test]
    fn the_first_capture_since_launch_says_so() {
        assert_eq!(
            paint_message(120.4, 81.6, None),
            "overlay painted 120 ms after the hotkey (capture 82 ms)"
        );
        assert_eq!(
            paint_message(160.0, 95.0, Some(Duration::from_millis(62_900))),
            "overlay painted 160 ms after the hotkey (capture 95 ms); first capture since \
             launch, 62 s after it"
        );
    }

    #[test]
    fn labels_round_trip() {
        for id in [0, 1, 69_734_208, u32::MAX] {
            assert_eq!(display_id(&label(id)), Some(id));
        }
        for other in ["settings", "overlay-", "overlay-x", "xoverlay-1"] {
            assert_eq!(display_id(other), None, "{other}");
        }
    }

    #[test]
    fn diff_creates_missing_and_destroys_unplugged() {
        let d = diff(["settings", "overlay-1", "overlay-2"], &[2, 3]);
        assert_eq!(d.create, [3]);
        assert_eq!(d.destroy, ["overlay-1"]);
        assert_eq!(
            diff(["overlay-2", "overlay-1"], &[1, 2]),
            WindowDiff::default()
        );
    }

    #[test]
    fn finds_the_display_under_a_point() {
        let display = |id, bounds| Display { id, bounds };
        let displays = [
            display(1, bounds(0, 0, 100, 50)),
            display(2, bounds(-80, -40, 80, 40)),
        ];
        assert_eq!(display_at(&displays, 0.0, 0.0), Some(1));
        assert_eq!(display_at(&displays, 99.9, 49.9), Some(1));
        assert_eq!(display_at(&displays, -1.0, -1.0), Some(2));
        assert_eq!(display_at(&displays, 100.0, 10.0), None);
        assert_eq!(display_at(&displays, -1.0, 10.0), None);
    }

    #[test]
    fn hotkey_is_ignored_unless_idle() {
        let mut p = Phase::default();
        assert_eq!(p.session(), None);
        assert!(p.begin(1, Instant::now()));
        assert!(!p.begin(2, Instant::now()), "pressed while capturing");
        assert_eq!(p.session(), Some(1));
        p.open(
            1,
            vec![frame(1, 4, 2)],
            None,
            true,
            Some(&[]),
            Mode::default(),
        )
        .unwrap();
        assert!(!p.begin(3, Instant::now()), "pressed while open");
        assert_eq!(p.session(), Some(1));
    }

    #[test]
    fn the_app_in_front_lasts_from_the_hotkey_to_the_close() {
        let mut p = Phase::default();
        assert_eq!(p.front(), None);
        let us = Front::Us { switches: 3 };
        p.set_front(us);
        assert_eq!(p.front(), None, "nothing to record it for");
        assert!(p.begin(1, Instant::now()));
        assert_eq!(p.front(), Some(Front::Other));
        p.set_front(us);
        p.open(
            1,
            vec![frame(1, 4, 2)],
            None,
            true,
            Some(&[]),
            Mode::default(),
        )
        .unwrap();
        assert_eq!(p.front(), Some(us));
        // Recorded after a fast capture has opened the session.
        p.set_front(Front::Other);
        assert_eq!(p.front(), Some(Front::Other));
        p.set_front(us);
        assert_eq!(p.end(), Some(us));
        assert_eq!((p.front(), p.end()), (None, None));
    }

    #[test]
    fn a_capture_closed_meanwhile_does_not_open() {
        let mut p = Phase::default();
        assert!(p.begin(1, Instant::now()));
        p = Phase::Idle; // Esc during the capture
        assert_eq!(
            p.open(
                1,
                vec![frame(1, 4, 2)],
                None,
                true,
                Some(&[]),
                Mode::default()
            ),
            None
        );
        assert!(p.begin(2, Instant::now()));
        assert_eq!(
            p.open(
                1,
                vec![frame(1, 4, 2)],
                None,
                true,
                Some(&[]),
                Mode::default()
            ),
            None,
            "older capture"
        );
        assert!(matches!(p, Phase::Capturing { session: 2, .. }));
    }

    #[test]
    fn open_sends_each_frame_size() {
        let mut p = Phase::default();
        p.begin(4, Instant::now());
        let shows = p
            .open(
                4,
                vec![frame(1, 4, 2), frame(2, 6, 8)],
                None,
                true,
                Some(&[]),
                Mode::default(),
            )
            .unwrap();
        let info = |width, height| FrameInfo {
            session: 4,
            width,
            height,
            logical_width: width / 2,
            logical_height: height / 2,
            escape_shortcut: true,
            region: None,
            mode: Mode::default(),
        };
        assert_eq!(shows, [(1, info(4, 2)), (2, info(6, 8))]);
        assert_eq!(p.pending_frame(2), Some(info(6, 8)));
        assert_eq!(p.pending_frame(3), None);
        let json = serde_json::to_value(info(4, 2)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "session": 4, "width": 4, "height": 2, "logicalWidth": 2, "logicalHeight": 1,
                "escapeShortcut": true, "region": null, "quickCopy": false,
                "quickCopyTip": false
            })
        );
    }

    #[test]
    fn a_remembered_region_is_sent_to_its_display_which_gets_focus() {
        use crate::region::{arrangement, tests::rect};
        let frames = || vec![frame(1, 4, 2), frame(2, 4, 2)];
        let monitors: Vec<Monitor> = frames().iter().map(Monitor::of).collect();
        let key = arrangement(&monitors);
        let on_two = Region {
            arrangement: key.clone(),
            display: monitors[1],
            rect: rect(1.0, 0.0, 5.0, 1.0),
        };
        let mut p = Phase::default();
        p.begin(1, Instant::now());
        let shows = p
            .open(
                1,
                frames(),
                Some(1),
                true,
                Some(std::slice::from_ref(&on_two)),
                Mode::default(),
            )
            .unwrap();
        assert_eq!(shows[0].1.region, None, "other displays start empty");
        // Clamped to the display (2 logical px wide).
        assert_eq!(shows[1].1.region, Some(rect(1.0, 0.0, 1.0, 1.0)));
        let first = p.painted(1, 1).unwrap();
        assert_eq!(first.focus, None, "waits for display 2");
        assert_eq!(first.fallback, None, "nothing is focused until 2 paints");
        let second = p.painted(1, 2).unwrap();
        // The cursor's display, if 2 can't be focused.
        assert_eq!((second.focus, second.fallback), (Some(2), Some(1)));

        // Another arrangement: nothing restored, the cursor's display has focus.
        let other = Region {
            arrangement: "elsewhere".into(),
            ..on_two
        };
        let mut p = Phase::default();
        p.begin(2, Instant::now());
        let shows = p
            .open(2, frames(), Some(1), true, Some(&[other]), Mode::default())
            .unwrap();
        assert!(shows.iter().all(|(_, info)| info.region.is_none()));
        let paint = p.painted(2, 1).unwrap();
        assert_eq!((paint.focus, paint.fallback), (Some(1), None));

        // A display was not captured: nothing restored, and nothing to remember.
        let mut p = Phase::default();
        p.begin(3, Instant::now());
        let shows = p
            .open(3, frames(), Some(1), true, None, Mode::default())
            .unwrap();
        assert!(shows.iter().all(|(_, info)| info.region.is_none()));
        assert_eq!(p.region(3, 2, px(0, 0, 1, 1)), None);
    }

    #[test]
    fn a_capture_is_whole_only_if_every_display_has_a_frame() {
        let display = |id| Display {
            id,
            bounds: bounds(0, 0, 2, 1),
        };
        let frames = [frame(1, 4, 2), frame(2, 4, 2)];
        assert!(captured_all(&[display(1), display(2)], &frames));
        assert!(captured_all(&[display(2)], &frames));
        assert!(!captured_all(&[display(1), display(3)], &frames));
        assert!(!captured_all(&[display(1)], &[]));
    }

    #[test]
    fn the_region_to_remember_is_the_selection_in_logical_px() {
        let mut p = open(None);
        let region = p.region(7, 2, px(1, 0, 2, 2)).unwrap();
        let monitors: Vec<Monitor> = [frame(1, 4, 2), frame(2, 4, 2)]
            .iter()
            .map(Monitor::of)
            .collect();
        assert_eq!(region.arrangement, region::arrangement(&monitors));
        assert_eq!(region.display, monitors[1]);
        assert_eq!(region.rect, region::tests::rect(0.5, 0.0, 1.0, 1.0));
        assert_eq!(p.region(6, 2, px(0, 0, 1, 1)), None, "other session");
        assert_eq!(p.region(7, 9, px(0, 0, 1, 1)), None, "unknown display");
    }

    #[test]
    fn each_frame_goes_to_its_window_once_and_stays_for_the_crop() {
        let mut p = open(None);
        assert_eq!(p.frame_for_page(6, 1), None, "other session");
        let image = p.frame_for_page(7, 1).unwrap();
        assert_eq!(*image, frame(1, 4, 2).image);
        assert_eq!(p.frame_for_page(7, 1), None);
        assert_eq!(p.pending_frame(1), None);
        assert_eq!(p.frame_for_page(7, 9), None, "unknown display");
        assert_eq!(Phase::Idle.frame_for_page(7, 1), None);
        // The original is shared, not moved.
        let (kept, density) = p.image(7, 1).unwrap();
        assert!(Arc::ptr_eq(&image, &kept));
        assert_eq!(density, Density::new(2.0, 2.0), "pixels over logical size");
        assert_eq!(p.image(7, 9), None);
        assert_eq!(p.image(6, 1), None);
    }

    #[test]
    fn esc_is_done_when_one_page_handles_it() {
        let mut p = shown();
        assert_eq!(p.forward_escape(1), Some(7));
        assert!(
            !p.escape_reply(7, 1, 1, false),
            "display 2 has not answered"
        );
        assert!(
            !p.escape_reply(7, 1, 2, true),
            "display 2 cleared its selection"
        );
        assert!(!p.escape_unhandled(1), "nothing left to wait for");
    }

    #[test]
    fn esc_closes_when_every_shown_page_had_nothing_to_undo() {
        let mut p = shown();
        p.forward_escape(1);
        assert!(!p.escape_reply(7, 1, 2, false));
        assert!(!p.escape_reply(7, 1, 2, false), "the same page twice");
        assert!(!p.escape_reply(7, 1, 9, false), "not a shown display");
        assert!(p.escape_reply(7, 1, 1, false));
        assert!(!p.escape_unhandled(1));
    }

    #[test]
    fn answers_to_an_older_esc_or_session_are_ignored() {
        let mut p = shown();
        p.forward_escape(1);
        p.forward_escape(2);
        assert!(!p.escape_reply(7, 1, 1, true), "older Esc");
        assert!(!p.escape_reply(6, 2, 1, true), "older session");
        assert!(!p.escape_reply(7, 1, 1, false));
        assert!(!p.escape_reply(7, 1, 2, false));
        assert!(p.escape_unhandled(2), "the newer Esc is still waiting");
        assert!(!p.escape_unhandled(2), "reported once");
        assert!(!p.escape_reply(7, 2, 1, true), "no Esc waiting");
        assert!(!Phase::Idle.escape_reply(7, 2, 1, false));
    }

    #[test]
    fn esc_closes_when_nothing_is_shown() {
        assert_eq!(Phase::Idle.forward_escape(1), None);
        assert!(!Phase::Idle.escape_unhandled(1));
        let mut capturing = Phase::default();
        capturing.begin(1, Instant::now());
        assert_eq!(capturing.forward_escape(1), None);
        assert_eq!(
            open(None).forward_escape(1),
            None,
            "open, nothing painted yet"
        );
    }

    /// An open session with both displays painted.
    fn shown() -> Phase {
        let mut p = open(None);
        p.painted(7, 1);
        p.painted(7, 2);
        p
    }
    #[test]
    fn first_paint_reports_the_timing_once() {
        let mut p = open(None);
        let first = p.painted(7, 2).unwrap();
        assert!(first.first.is_some());
        assert_eq!(p.painted(7, 2), None, "same window twice");
        assert_eq!(p.painted(7, 1).unwrap().first, None);
        assert_eq!(p.painted(6, 1), None, "old session");
    }

    #[test]
    fn focus_goes_to_the_display_under_the_cursor() {
        let mut p = open(Some(2));
        assert_eq!(
            p.painted(7, 1).unwrap().focus,
            None,
            "cursor display not shown yet"
        );
        assert_eq!(p.painted(7, 2).unwrap().focus, Some(2));
    }

    #[test]
    fn focus_goes_to_the_first_painted_without_a_cursor_display() {
        // No display under the cursor, or one that was not captured.
        for focus in [None, Some(9)] {
            let mut p = open(focus);
            assert_eq!(p.painted(7, 2).unwrap().focus, Some(2));
            assert_eq!(p.painted(7, 1).unwrap().focus, Some(2));
        }
    }

    #[test]
    fn unreported_displays_are_shown_late_with_focus_only_if_none_painted() {
        let mut p = open(Some(2));
        assert_eq!(
            p.unreported(7),
            Some((vec![1, 2], Some(2))),
            "focus under the cursor"
        );
        assert_eq!(p.unreported(6), None, "other session");
        assert_eq!(
            open(None).unreported(7),
            Some((vec![1, 2], Some(1))),
            "else the first"
        );
        p.painted(7, 1);
        assert_eq!(
            p.unreported(7),
            Some((vec![2], None)),
            "focus already given"
        );
        assert!(p.begin_save(7));
        assert_eq!(p.unreported(7), None, "hidden for the Save dialog");
        p.end_save(7);
        p.painted(7, 2);
        assert_eq!(p.unreported(7), None, "all reported");
        assert_eq!(Phase::Idle.unreported(7), None);
    }

    #[test]
    fn a_copy_may_close_only_if_no_save_began_since() {
        let mut p = open(None);
        p.painted(7, 2);
        let saves = p.saves(7).unwrap();
        assert!(p.copy_may_close(7, saves));
        assert!(!p.copy_may_close(6, saves), "other session");
        p.begin_save(7);
        assert_eq!(p.saves(7), None, "saving");
        assert!(!p.copy_may_close(7, saves));
        p.end_save(7);
        assert!(!p.copy_may_close(7, saves), "a save began since");
        assert!(p.copy_may_close(7, p.saves(7).unwrap()));
        assert_eq!(Phase::Idle.saves(7), None);
    }

    #[test]
    fn a_save_hides_a_shown_session_once() {
        let mut p = open(None);
        assert!(!p.begin_save(7), "nothing shown yet");
        p.painted(7, 2);
        p.forward_escape(1);
        assert!(!p.begin_save(6), "other session");
        assert!(!p.saving());
        assert!(p.begin_save(7));
        assert!(p.saving());
        assert!(!p.escape_unhandled(1), "a pending Esc is dropped");
        assert!(!p.begin_save(7), "already saving");
        // A display that paints during the dialog stays hidden, and shows with the others.
        assert!(p.painted(7, 1).unwrap().hidden);
        assert_eq!(p.painted(7, 1), None);
        assert_eq!(p.end_save(6), None, "other session");
        assert!(!p.wants_escape(), "Esc is the dialog's");
        assert_eq!(p.end_save(7), Some(vec![2, 1]));
        assert!(p.wants_escape());
        assert!(!p.saving());
        assert_eq!(p.end_save(7), None, "not saving any more");
        assert!(!Phase::Idle.begin_save(7));
        assert!(!Phase::Idle.saving());
        assert_eq!(Phase::Idle.end_save(7), None);
    }

    #[test]
    fn a_share_holds_the_session_until_its_sheet_closes() {
        let mut p = open(None);
        assert!(!p.begin_share(7), "nothing shown yet");
        p.painted(7, 2);
        p.forward_escape(1);
        assert!(!p.begin_share(6), "other session");
        assert!(p.wants_escape());
        assert!(p.begin_share(7));
        assert!(!p.escape_unhandled(1), "a pending Esc is dropped");
        assert!(!p.wants_escape(), "Esc is the sheet's");
        assert!(p.in_dialog());
        assert!(!p.saving(), "the windows stay shown");
        assert!(!p.begin_share(7), "already sharing");
        assert!(!p.begin_save(7), "no save over the sheet");
        assert_eq!(p.saves(7), None, "no copy or share over the sheet");
        assert!(!p.end_share(6), "other session");
        assert!(p.end_share(7));
        assert!(!p.end_share(7), "ended once");
        assert!(p.wants_escape());
        assert!(!p.in_dialog());
        assert_eq!(p.saves(7), Some(0));
        // Nor a share over the Save dialog.
        assert!(p.begin_save(7));
        assert!(p.in_dialog());
        assert!(!p.begin_share(7));
        assert!(!Phase::Idle.begin_share(7));
        assert!(!Phase::Idle.end_share(7));
        assert!(!Phase::Idle.in_dialog());
    }

    #[test]
    fn esc_is_held_while_capturing_and_open_if_it_was_registered() {
        let mut p = Phase::default();
        assert!(!p.wants_escape());
        p.begin(1, Instant::now());
        assert!(p.wants_escape());
        p.open(
            1,
            vec![frame(1, 4, 2)],
            None,
            false,
            Some(&[]),
            Mode::default(),
        )
        .unwrap();
        assert!(!p.wants_escape(), "the pages forward Esc");
        assert!(open(None).wants_escape());
    }

    #[test]
    fn stalled_until_something_paints() {
        let mut p = open(None);
        assert!(p.stalled(7));
        assert!(!p.stalled(6));
        p.painted(7, 1);
        assert!(!p.stalled(7));
        assert!(!Phase::Idle.stalled(7));
    }
}
