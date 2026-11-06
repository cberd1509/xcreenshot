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
pub(crate) mod window;

pub use window::{Overlay, prewarm, start_capture};

use crate::capture::{self, CaptureError, Display, DisplayFrame, Rect};
use image::RgbaImage;
use std::time::Instant;

const LABEL_PREFIX: &str = "overlay-";

/// Window label for a display's overlay.
pub fn label(display: u32) -> String {
    format!("{LABEL_PREFIX}{display}")
}

/// The display id in an overlay window label, or `None` for any other window.
pub fn display_id(label: &str) -> Option<u32> {
    label.strip_prefix(LABEL_PREFIX)?.parse().ok()
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

/// What the overlay window needs to draw a frame; sent with `overlay:show`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct FrameInfo {
    pub session: u64,
    pub width: u32,
    pub height: u32,
}

/// The overlay's whole state. One capture at a time: the hotkey is ignored unless `Idle`.
#[derive(Debug, Default)]
pub enum Phase {
    #[default]
    Idle,
    Capturing {
        session: u64,
        started: Instant,
    },
    Open(Session),
}

#[derive(Debug)]
pub struct Session {
    id: u64,
    started: Instant,
    capture_ms: f64,
    slots: Vec<Slot>,
    /// The display under the cursor, if it has a frame. It gets keyboard focus.
    focus: Option<u32>,
    painted: Vec<u32>,
    /// The display with a selection. There is at most one across all displays.
    selection: Option<u32>,
}

/// One display's original frame, kept for the crop. Its window gets a copy, once.
#[derive(Debug)]
struct Slot {
    info: FrameInfo,
    frame: DisplayFrame,
    sent: bool,
}

/// What Esc does.
#[derive(Debug, PartialEq, Eq)]
pub enum Escape {
    /// Clear the selection on this display.
    Clear(u32),
    /// Nothing is selected: close the overlay.
    Close,
}

/// What to do when a window reports it painted.
#[derive(Debug, PartialEq)]
pub struct Paint {
    /// Window to focus after showing this one.
    pub focus: Option<u32>,
    /// Set on the first paint of the session: hotkey-to-paint and capture time, in ms.
    pub first: Option<(f64, f64)>,
}

fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

impl Phase {
    /// Starts a capture. Returns `false` (and changes nothing) unless idle.
    pub fn begin(&mut self, session: u64, started: Instant) -> bool {
        if !matches!(self, Phase::Idle) {
            return false;
        }
        *self = Phase::Capturing { session, started };
        true
    }

    /// The capture finished. Opens the session if this capture is still current (Esc may have
    /// closed it) and returns each display's frame info to send. `focus` is the display under
    /// the cursor; it gets keyboard focus if it was captured.
    pub fn open(
        &mut self,
        session: u64,
        frames: Vec<DisplayFrame>,
        focus: Option<u32>,
    ) -> Option<Vec<(u32, FrameInfo)>> {
        let Phase::Capturing {
            session: id,
            started,
        } = *self
        else {
            return None;
        };
        if id != session {
            return None;
        }
        let focus = focus.filter(|&d| frames.iter().any(|f| f.id == d));
        let slots: Vec<Slot> = frames
            .into_iter()
            .map(|frame| {
                let (width, height) = frame.image.dimensions();
                let info = FrameInfo {
                    session,
                    width,
                    height,
                };
                Slot {
                    info,
                    frame,
                    sent: false,
                }
            })
            .collect();
        let shows = slots.iter().map(|s| (s.frame.id, s.info)).collect();
        *self = Phase::Open(Session {
            id: session,
            started,
            capture_ms: ms_since(started),
            slots,
            focus,
            painted: Vec::new(),
            selection: None,
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
        s.slots.iter_mut().find(|slot| slot.frame.id == display)
    }

    /// A display's frame info while its frame is not sent yet, for a window that loaded after
    /// `overlay:show` was sent.
    pub fn pending_frame(&self, display: u32) -> Option<FrameInfo> {
        let Phase::Open(s) = self else { return None };
        s.slots
            .iter()
            .find(|slot| slot.frame.id == display && !slot.sent)
            .map(|slot| slot.info)
    }

    /// A copy of a display's frame as raw RGBA for its window, once. The original stays here
    /// for [`Phase::crop`]. The copy takes about 1 ms for a 3600x2338 frame.
    pub fn frame_copy(&mut self, session: u64, display: u32) -> Option<Vec<u8>> {
        let slot = self.slot(session, display)?;
        if slot.sent {
            return None;
        }
        slot.sent = true;
        Some(slot.frame.image.as_raw().to_vec())
    }

    /// Cuts a logical selection out of a display's original frame, at native resolution.
    /// `None` if the session or display is gone.
    pub fn crop(
        &mut self,
        session: u64,
        display: u32,
        rect: Rect,
    ) -> Option<Result<RgbaImage, CaptureError>> {
        let slot = self.slot(session, display)?;
        Some(capture::crop(&slot.frame, rect))
    }

    /// A display gained or lost its selection. Returns the display whose selection to clear:
    /// starting a selection on one display clears the one on another.
    pub fn select(&mut self, session: u64, display: u32, active: bool) -> Option<u32> {
        let s = self.current(session)?;
        if active {
            s.selection.replace(display).filter(|&d| d != display)
        } else {
            if s.selection == Some(display) {
                s.selection = None;
            }
            None
        }
    }

    /// Esc clears the selection if there is one, else closes the overlay.
    pub fn escape(&mut self) -> Escape {
        match self {
            Phase::Open(s) => s.selection.take().map_or(Escape::Close, Escape::Clear),
            _ => Escape::Close,
        }
    }

    /// A window drew its frame. `None` if the session is gone or it already painted.
    pub fn painted(&mut self, session: u64, display: u32) -> Option<Paint> {
        self.slot(session, display)?;
        let s = self.current(session)?;
        if s.painted.contains(&display) {
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
        Some(Paint { focus, first })
    }

    /// Whether this session is still open with nothing painted (the overlay page is broken).
    pub fn stalled(&self, session: u64) -> bool {
        matches!(self, Phase::Open(s) if s.id == session && s.painted.is_empty())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::capture::Bounds;

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
        p.open(7, vec![frame(1, 4, 2), frame(2, 4, 2)], focus)
            .unwrap();
        p
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
        p.open(1, vec![frame(1, 4, 2)], None).unwrap();
        assert!(!p.begin(3, Instant::now()), "pressed while open");
        assert_eq!(p.session(), Some(1));
    }

    #[test]
    fn a_capture_closed_meanwhile_does_not_open() {
        let mut p = Phase::default();
        assert!(p.begin(1, Instant::now()));
        p = Phase::Idle; // Esc during the capture
        assert_eq!(p.open(1, vec![frame(1, 4, 2)], None), None);
        assert!(p.begin(2, Instant::now()));
        assert_eq!(p.open(1, vec![frame(1, 4, 2)], None), None, "older capture");
        assert!(matches!(p, Phase::Capturing { session: 2, .. }));
    }

    #[test]
    fn open_sends_each_frame_size() {
        let mut p = Phase::default();
        p.begin(4, Instant::now());
        let shows = p
            .open(4, vec![frame(1, 4, 2), frame(2, 6, 8)], None)
            .unwrap();
        let info = |width, height| FrameInfo {
            session: 4,
            width,
            height,
        };
        assert_eq!(shows, [(1, info(4, 2)), (2, info(6, 8))]);
        assert_eq!(p.pending_frame(2), Some(info(6, 8)));
        assert_eq!(p.pending_frame(3), None);
    }

    #[test]
    fn each_frame_is_copied_to_its_window_once() {
        let mut p = open(None);
        assert_eq!(p.frame_copy(6, 1), None, "other session");
        let bytes = p.frame_copy(7, 1).unwrap();
        assert_eq!(bytes, frame(1, 4, 2).image.into_raw());
        assert_eq!(p.frame_copy(7, 1), None);
        assert_eq!(p.pending_frame(1), None);
        assert_eq!(p.frame_copy(7, 9), None, "unknown display");
        assert_eq!(Phase::Idle.frame_copy(7, 1), None);
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn crops_the_original_after_the_copy_was_sent() {
        let mut p = open(None);
        p.frame_copy(7, 2).unwrap();
        // 4x2 px at 2x: logical (1, 0, 1, 1) is pixels x 2..4, y 0..2.
        let image = p.crop(7, 2, rect(1.0, 0.0, 1.0, 1.0)).unwrap().unwrap();
        let expected = RgbaImage::from_fn(2, 2, |x, y| image::Rgba([2 + x as u8, y as u8, 9, 255]));
        assert_eq!(image, expected);
        let empty = p.crop(7, 2, rect(1.0, 0.0, 0.0, 1.0)).unwrap();
        assert_eq!(empty, Err(CaptureError::EmptySelection));
        assert!(
            p.crop(6, 2, rect(0.0, 0.0, 1.0, 1.0)).is_none(),
            "other session"
        );
        assert!(
            p.crop(7, 9, rect(0.0, 0.0, 1.0, 1.0)).is_none(),
            "unknown display"
        );
    }

    #[test]
    fn one_selection_across_displays() {
        let mut p = open(None);
        assert_eq!(p.select(6, 1, true), None, "other session");
        assert_eq!(p.select(7, 1, true), None);
        assert_eq!(p.select(7, 1, true), None, "same display again");
        assert_eq!(p.select(7, 2, true), Some(1), "display 1 is cleared");
        assert_eq!(p.select(7, 1, false), None, "display 1 had none left");
        assert_eq!(p.escape(), Escape::Clear(2));
        assert_eq!(p.escape(), Escape::Close);
    }

    #[test]
    fn esc_closes_without_a_selection() {
        let mut p = open(None);
        p.select(7, 1, true);
        p.select(7, 1, false);
        assert_eq!(p.escape(), Escape::Close);
        assert_eq!(Phase::Idle.escape(), Escape::Close);
        let mut capturing = Phase::default();
        capturing.begin(1, Instant::now());
        assert_eq!(capturing.escape(), Escape::Close);
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
    fn stalled_until_something_paints() {
        let mut p = open(None);
        assert!(p.stalled(7));
        assert!(!p.stalled(6));
        p.painted(7, 1);
        assert!(!p.stalled(7));
        assert!(!Phase::Idle.stalled(7));
    }
}
