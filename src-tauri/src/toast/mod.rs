//! Confirmation toasts: "Copied", "Saved" or "Shared" at the bottom of the display the capture
//! was on, after every finished copy, save and share, with the system screenshot sound.
//!
//! One window (`toast.html`), built hidden at startup and shown only while toasts are up. It never
//! takes focus: it can't become the key window, ignores the mouse, and is shown without
//! activating the app. The page stacks, counts and fades the toasts (`src/toast/stack.ts`).
//! Rust numbers them, keeps the ones sent before the page loaded, places the window and hides
//! it once the page has nothing left on screen.
//!
//! This file is the pure part; the Tauri calls are in `window.rs` and the sound in `sound.rs`.

mod sound;
/// Public to the crate so `generate_handler!` can reach the commands.
pub(crate) mod window;

pub use window::{Toasts, hide_for_capture, prewarm, show};

use crate::capture::Bounds;
use std::path::Path;

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
    /// Saved: the file's name.
    Saved { file: String },
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
        Self::Saved { file }
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
/// them 28 px above the bottom, as designed.
pub const SIZE: (u32, u32) = (560, 180);

/// Where the window goes: centred at the bottom of `display`, in its OS units (points on
/// macOS, pixels on Windows), `scale` of them per logical px. Never wider or taller than it.
pub fn place(display: Bounds, scale: f64) -> Bounds {
    let fit = |logical: u32, room: u32| {
        // At most `room`, so the cast can't overflow.
        (f64::from(logical) * scale)
            .round()
            .clamp(0.0, f64::from(room)) as u32
    };
    let width = fit(SIZE.0, display.width);
    let height = fit(SIZE.1, display.height);
    Bounds {
        x: display.x + ((display.width - width) / 2) as i32,
        y: display.y + (display.height - height) as i32,
        width,
        height,
    }
}

/// Toasts kept for a page that has not loaded yet: it shows three at most.
const PENDING: usize = 3;

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
}

impl Queue {
    /// A new toast. Returns it to send now if the page is listening (the window is then on
    /// screen), or keeps it for [`Queue::ready`].
    pub fn push(&mut self, toast: Toast) -> Option<Shown> {
        self.seq += 1;
        let shown = Shown {
            seq: self.seq,
            toast,
        };
        if self.ready {
            self.on_screen = true;
            return Some(shown);
        }
        self.pending.push(shown);
        if self.pending.len() > PENDING {
            self.pending.remove(0);
        }
        None
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
                file: String::new()
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
}
