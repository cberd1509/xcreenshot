//! Confirmation toasts: "Copied", "Saved" or "Shared" at the bottom of the display the capture was
//! on, above the taskbar or the Dock, after every finished copy, save and share, with the system
//! screenshot sound on macOS (Windows has none).
//!
//! One window (`toast.html`), built hidden at startup and shown only while toasts are up. It never
//! takes focus: it can't become the key window and is shown without activating the app. It
//! ignores the mouse, except while a Saved toast is up: its Open and Show in Finder (Show in
//! folder) buttons take clicks, still without activating the app. The page stacks, counts and
//! fades the toasts (`src/toast/stack.ts`). Rust numbers them, keeps the ones sent before the
//! page loaded, remembers the files saved, places the window and hides it once the page has
//! nothing left on screen.
//!
//! This file is the pure part; the Tauri calls are in `window.rs`, the sound in `sound.rs` and
//! opening the saved file in `open.rs`.

mod open;
mod sound;
/// Public to the crate so `generate_handler!` can reach the commands.
pub(crate) mod window;

pub use open::Action;
pub use window::{Toasts, hide_for_capture, on_page_load, prewarm, show};

use crate::capture::{Bounds, Rect};
use std::path::{Path, PathBuf};

/// The toast window's label.
pub const LABEL: &str = "toast";

/// What a toast says. The page has the words.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Toast {
    /// Copied: the image's size in pixels.
    Copied { width: u32, height: u32 },
    /// Saved: the file's name. The page only gets the name; the path stays in Rust.
    Saved {
        file: String,
        #[serde(skip)]
        path: PathBuf,
    },
    /// Shared: the service picked, if the share sheet said which.
    Shared { service: Option<String> },
}

impl Toast {
    /// The Saved toast for a file written to `path`.
    pub fn saved(path: &Path) -> Self {
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::Saved {
            file,
            path: path.to_path_buf(),
        }
    }
}

/// A toast and its number, sent with `toast:show`. Numbers only grow.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Shown {
    pub seq: u64,
    #[serde(flatten)]
    pub toast: Toast,
}

/// The window's size in logical px: room for three toasts and their shadows. The page puts
/// them 28 px above the bottom, as designed, so they sit 28 px above the work area.
pub const SIZE: (u32, u32) = (560, 180);

/// The part of `display` the toasts go in: its `work` area (without the taskbar, or the menu
/// bar and the Dock), wherever those are, in whole OS units inside it. The whole display if the
/// OS didn't say or the work area isn't on it.
pub fn room(display: Bounds, work: Option<Rect>) -> Bounds {
    let Some(work) = work.filter(|w| [w.x, w.y, w.width, w.height].iter().all(|v| v.is_finite()))
    else {
        return display;
    };
    let left = work.x.max(f64::from(display.x)).ceil();
    let top = work.y.max(f64::from(display.y)).ceil();
    let right = (work.x + work.width)
        .min(f64::from(display.x) + f64::from(display.width))
        .floor();
    let bottom = (work.y + work.height)
        .min(f64::from(display.y) + f64::from(display.height))
        .floor();
    if right <= left || bottom <= top {
        return display;
    }
    // Inside the display, so the casts can't overflow.
    Bounds {
        x: left as i32,
        y: top as i32,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    }
}

/// Where the window goes: centred at the bottom of `area` (see [`room`]), in its OS units
/// (points on macOS, pixels on Windows), `scale` of them per logical px. Never wider or taller
/// than it.
pub fn place(area: Bounds, scale: f64) -> Bounds {
    let fit = |logical: u32, room: u32| {
        // At most `room`, so the cast can't overflow.
        (f64::from(logical) * scale)
            .round()
            .clamp(0.0, f64::from(room)) as u32
    };
    let width = fit(SIZE.0, area.width);
    let height = fit(SIZE.1, area.height);
    Bounds {
        x: area.x + ((area.width - width) / 2) as i32,
        y: area.y + (area.height - height) as i32,
        width,
        height,
    }
}

/// Toasts kept for a page that has not loaded yet: it shows three at most.
const PENDING: usize = 3;
/// Saved files kept for the Saved toasts' buttons: as many as the page shows (`MOST` in
/// `src/toast/stack.ts`).
const SAVED: usize = 3;

/// Which toasts went to the page, and whether the window is on screen.
#[derive(Debug, Default)]
pub struct Queue {
    /// The last toast's number.
    seq: u64,
    /// The page is listening.
    ready: bool,
    /// Toasts sent before it was.
    pending: Vec<Shown>,
    /// The window is shown.
    on_screen: bool,
    /// The newest files saved, by toast number.
    saved: Vec<(u64, PathBuf)>,
}

impl Queue {
    /// A new toast. Returns it to send now if the page is listening (the window is then on
    /// screen), or keeps it for [`Queue::ready`].
    pub fn push(&mut self, toast: Toast) -> Option<Shown> {
        self.seq += 1;
        if let Toast::Saved { path, .. } = &toast {
            self.saved.push((self.seq, path.clone()));
            if self.saved.len() > SAVED {
                self.saved.remove(0);
            }
        }
        let shown = Shown {
            seq: self.seq,
            toast,
        };
        if self.ready {
            self.on_screen = true;
            return Some(shown);
        }
        self.keep(shown);
        None
    }

    /// Keeps `shown` for the page's next [`Queue::ready`], with the newest few.
    fn keep(&mut self, shown: Shown) {
        self.pending.push(shown);
        if self.pending.len() > PENDING {
            self.pending.remove(0);
        }
    }

    /// `shown` could not reach the page: it waits for the page to ask again, and the window is
    /// off screen meanwhile.
    pub fn undelivered(&mut self, shown: Shown) {
        self.reloading();
        self.keep(shown);
    }

    /// The page is loading (again): toasts wait until it listens. Returns whether the window
    /// was on screen; it goes off until then.
    pub fn reloading(&mut self) -> bool {
        self.ready = false;
        std::mem::take(&mut self.on_screen)
    }

    /// The page is listening: the toasts it missed. If there are any, the window is on screen.
    pub fn ready(&mut self) -> Vec<Shown> {
        self.ready = true;
        let missed = std::mem::take(&mut self.pending);
        self.on_screen |= !missed.is_empty();
        missed
    }

    /// The page has shown everything up to `seq` and nothing is left on screen. Returns whether
    /// to hide the window: not if a newer toast is on its way, or it is hidden already.
    pub fn idle(&mut self, seq: u64) -> bool {
        if !self.on_screen || seq < self.seq {
            return false;
        }
        self.on_screen = false;
        true
    }

    /// The file the Saved toast numbered `seq` is about, if it is one of the newest saved. The
    /// page names a toast, never a path, so its buttons only reach files the app just saved.
    pub fn file(&self, seq: u64) -> Option<&Path> {
        self.saved
            .iter()
            .find(|(s, _)| *s == seq)
            .map(|(_, path)| path.as_path())
    }

    /// The page asks for the window to take clicks (`on`: a Saved toast is up) or not. Returns
    /// whether it should: only while on screen, so a late ask can't leave the window catching
    /// clicks when it is shown again for another toast.
    pub fn clickable(&self, on: bool) -> bool {
        on && self.on_screen
    }

    /// A capture is starting: drops the toasts. Returns whether the window was on screen.
    pub fn clear(&mut self) -> bool {
        self.pending.clear();
        std::mem::take(&mut self.on_screen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn copied(width: u32) -> Toast {
        Toast::Copied { width, height: 9 }
    }

    #[test]
    fn toasts_reach_the_page_as_kind_and_detail() {
        let shown = |toast| serde_json::to_value(Shown { seq: 4, toast }).unwrap();
        assert_eq!(
            shown(copied(12)),
            json!({ "seq": 4, "kind": "copied", "width": 12, "height": 9 })
        );
        assert_eq!(
            shown(Toast::saved(&PathBuf::from("/a/b/shot.png"))),
            json!({ "seq": 4, "kind": "saved", "file": "shot.png" })
        );
        assert_eq!(
            shown(Toast::Shared {
                service: Some("AirDrop".into())
            }),
            json!({ "seq": 4, "kind": "shared", "service": "AirDrop" })
        );
        assert_eq!(
            shown(Toast::Shared { service: None }),
            json!({ "seq": 4, "kind": "shared", "service": null })
        );
        assert_eq!(
            Toast::saved(Path::new("/")),
            Toast::Saved {
                file: String::new(),
                path: PathBuf::from("/"),
            }
        );
    }

    fn bounds(x: i32, y: i32, width: u32, height: u32) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn the_window_sits_at_the_bottom_centre_of_the_display() {
        // A 1440 x 900 pt display left of the main one (macOS: points, scale 1).
        assert_eq!(
            place(bounds(-1440, 0, 1440, 900), 1.0),
            bounds(-1440 + 440, 720, 560, 180)
        );
        // A 150% display in pixels (Windows), below the main one.
        assert_eq!(
            place(bounds(0, 1080, 2880, 1620), 1.5),
            bounds(1020, 1080 + 1350, 840, 270)
        );
        // Never past a tiny display.
        assert_eq!(
            place(bounds(10, 20, 300, 100), 1.0),
            bounds(10, 20, 300, 100)
        );
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
    fn the_toasts_sit_above_the_taskbar_wherever_it_is() {
        // A 2880 x 1620 px display at 150% (Windows), right of the main one; the 48 px taskbar
        // is 72 px.
        let display = bounds(1920, 0, 2880, 1620);
        let bottom = room(display, Some(rect(1920.0, 0.0, 2880.0, 1548.0)));
        assert_eq!(bottom, bounds(1920, 0, 2880, 1548));
        assert_eq!(
            place(bottom, 1.5),
            bounds(1920 + 1020, 1548 - 270, 840, 270),
            "the window ends where the taskbar starts, so the toasts are 28 px above it"
        );
        let top = room(display, Some(rect(1920.0, 72.0, 2880.0, 1548.0)));
        assert_eq!(place(top, 1.5), bounds(1920 + 1020, 1350, 840, 270));
        // On the left or right, the toasts centre on what's left.
        let left = room(display, Some(rect(1992.0, 0.0, 2808.0, 1620.0)));
        assert_eq!(place(left, 1.5), bounds(1992 + 984, 1350, 840, 270));
        let right = room(display, Some(rect(1920.0, 0.0, 2808.0, 1620.0)));
        assert_eq!(place(right, 1.5), bounds(1920 + 984, 1350, 840, 270));
    }

    #[test]
    fn the_toasts_sit_above_the_dock() {
        // macOS, in points: a 25 pt menu bar and a 70 pt Dock at the bottom.
        let display = bounds(0, 0, 1512, 982);
        let work = room(display, Some(rect(0.0, 25.0, 1512.0, 887.0)));
        assert_eq!(work, bounds(0, 25, 1512, 887));
        assert_eq!(place(work, 1.0), bounds(476, 912 - 180, 560, 180));
    }

    #[test]
    fn the_room_stays_in_whole_units_inside_the_display() {
        let display = bounds(-1440, 0, 1440, 900);
        // Fractional work areas round inward.
        assert_eq!(
            room(display, Some(rect(-1439.5, 24.5, 1439.0, 800.7))),
            bounds(-1439, 25, 1438, 800)
        );
        // A work area that spills past the display is cut to it.
        assert_eq!(
            room(display, Some(rect(-2000.0, -10.0, 5000.0, 860.0))),
            bounds(-1440, 0, 1440, 850)
        );
        // None, empty, off the display or NaN: the whole display.
        for work in [
            None,
            Some(rect(-1440.0, 0.0, 0.0, 900.0)),
            Some(rect(5000.0, 0.0, 100.0, 100.0)),
            Some(rect(f64::NAN, 0.0, 100.0, 100.0)),
        ] {
            assert_eq!(room(display, work), display);
        }
    }

    #[test]
    fn toasts_wait_for_the_page_then_go_straight_to_it() {
        let mut q = Queue::default();
        for width in 1..=4 {
            assert_eq!(q.push(copied(width)), None);
        }
        assert!(!q.idle(4), "nothing on screen yet");
        let missed = q.ready();
        let seqs: Vec<u64> = missed.iter().map(|s| s.seq).collect();
        assert_eq!(seqs, [2, 3, 4], "the newest three");
        assert_eq!(missed[0].toast, copied(2));
        assert!(q.ready().is_empty(), "sent once");
        assert_eq!(
            q.push(copied(5)),
            Some(Shown {
                seq: 5,
                toast: copied(5)
            })
        );
        assert!(q.idle(5));
        assert!(!q.idle(5), "hidden already");
    }

    #[test]
    fn a_ready_page_with_nothing_missed_keeps_the_window_hidden() {
        let mut q = Queue::default();
        assert!(q.ready().is_empty());
        assert!(!q.idle(0));
    }

    #[test]
    fn the_window_stays_while_a_newer_toast_is_on_its_way() {
        let mut q = Queue::default();
        q.ready();
        q.push(copied(1));
        q.push(copied(2));
        assert!(!q.idle(1), "toast 2 is not shown yet");
        assert!(q.idle(2));
    }

    #[test]
    fn a_toast_the_page_did_not_get_waits_for_it_to_ask_again() {
        let mut q = Queue::default();
        q.ready();
        let lost = q.push(copied(1)).unwrap();
        q.undelivered(lost.clone());
        assert!(!q.idle(1), "off screen");
        assert_eq!(q.push(copied(2)), None, "the page is not listening");
        let missed: Vec<u64> = q.ready().iter().map(|s| s.seq).collect();
        assert_eq!(missed, [1, 2]);
        assert!(q.idle(2));
    }

    #[test]
    fn a_reloading_page_gets_the_toasts_sent_meanwhile() {
        let mut q = Queue::default();
        q.ready();
        q.push(copied(1));
        assert!(q.reloading(), "it was on screen");
        assert!(!q.reloading());
        assert_eq!(q.push(copied(2)), None);
        assert_eq!(q.ready().len(), 1);
    }

    #[test]
    fn a_capture_drops_the_toasts() {
        let mut q = Queue::default();
        q.push(copied(1));
        assert!(!q.clear(), "not on screen");
        assert!(q.ready().is_empty(), "dropped");
        q.push(copied(2));
        assert!(q.clear());
        assert!(!q.clear());
        assert!(!q.idle(2));
    }

    fn saved(name: &str) -> Toast {
        Toast::saved(&PathBuf::from("/shots").join(name))
    }

    #[test]
    fn the_newest_saved_files_are_kept_for_their_buttons() {
        let mut q = Queue::default();
        q.ready();
        q.push(saved("1.png"));
        q.push(copied(2));
        for name in ["3.png", "4.png", "5.png"] {
            q.push(saved(name));
        }
        assert_eq!(q.file(1), None, "only the newest three");
        assert_eq!(q.file(2), None, "not a Saved toast");
        assert_eq!(q.file(3), Some(Path::new("/shots/3.png")));
        assert_eq!(q.file(5), Some(Path::new("/shots/5.png")));
        assert_eq!(q.file(6), None, "no such toast");
        q.clear();
        assert_eq!(
            q.file(5),
            Some(Path::new("/shots/5.png")),
            "kept by a capture"
        );
    }

    #[test]
    fn the_window_takes_clicks_only_while_on_screen() {
        let mut q = Queue::default();
        assert!(!q.clickable(true), "hidden");
        q.ready();
        q.push(saved("a.png"));
        assert!(q.clickable(true));
        assert!(!q.clickable(false));
        q.clear();
        assert!(!q.clickable(true), "a late ask after a capture");
        q.push(saved("b.png"));
        assert!(q.idle(2));
        assert!(!q.clickable(true), "a late ask after the last toast faded");
    }
}
