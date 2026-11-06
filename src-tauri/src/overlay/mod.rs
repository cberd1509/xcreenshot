//! The frozen-frame overlay: one prewarmed window per display, shown on the capture hotkey.
//!
//! This file is pure logic (labels, window diffing, the re-entry guard, frame packing).
//! The Tauri calls live in `window.rs`.

/// Public to the crate so `generate_handler!` can reach the commands.
pub(crate) mod window;

pub use window::{Overlay, close, close_requested, prewarm, start_capture};

use crate::capture::DisplayFrame;
use std::sync::atomic::{AtomicBool, Ordering};

const LABEL_PREFIX: &str = "overlay-";

/// Window label for a display's overlay.
pub fn label(display: u32) -> String {
    format!("{LABEL_PREFIX}{display}")
}

/// The display id in an overlay window label, or `None` for any other window.
pub fn display_id(label: &str) -> Option<u32> {
    // Only labels that `label` writes: `u32::from_str` also takes "+1" and "01".
    let id = label.strip_prefix(LABEL_PREFIX)?.parse().ok()?;
    (self::label(id) == label).then_some(id)
}

/// Overlay windows to create and destroy so there is exactly one per display.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WindowDiff {
    pub create: Vec<u32>,
    pub destroy: Vec<String>,
}

/// Compares the open window labels with the current displays. Labels that are not overlay
/// windows (Settings) are left alone.
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
    let mut create: Vec<u32> = Vec::new();
    for &id in displays {
        if !existing.contains(&id) && !create.contains(&id) {
            create.push(id);
        }
    }
    destroy.sort();
    WindowDiff { create, destroy }
}

/// Set while the overlay is open, so a second hotkey press (or the menu item firing with the
/// shortcut) does not capture again.
#[derive(Debug, Default)]
pub struct Guard(AtomicBool);

impl Guard {
    /// Claims the guard. Returns `false` if the overlay is already open.
    pub fn try_begin(&self) -> bool {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn end(&self) {
        self.0.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Size of the header before the pixels in [`pack_frame`].
pub const HEADER_LEN: usize = 8;

/// The frame as sent to the overlay: width and height (u32, little-endian), then raw RGBA rows.
/// One copy, no encoding; the frontend wraps the pixels in an `ImageData` without copying.
pub fn pack_frame(frame: &DisplayFrame) -> Vec<u8> {
    let pixels = frame.image.as_raw();
    let mut out = Vec::with_capacity(HEADER_LEN + pixels.len());
    out.extend_from_slice(&frame.image.width().to_le_bytes());
    out.extend_from_slice(&frame.image.height().to_le_bytes());
    out.extend_from_slice(pixels);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::Bounds;
    use image::RgbaImage;

    pub(super) fn frame(id: u32, width: u32, height: u32) -> DisplayFrame {
        let image = RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([x as u8, y as u8, 9, 255])
        });
        let bounds = Bounds {
            x: id as i32 * 100,
            y: -20,
            width,
            height,
        };
        DisplayFrame {
            id,
            logical: Bounds {
                width: width / 2,
                height: height / 2,
                ..bounds
            },
            physical: bounds,
            scale_factor: 2.0,
            image,
        }
    }

    #[test]
    fn labels_round_trip() {
        for id in [0, 1, 69_734_208, u32::MAX] {
            assert_eq!(display_id(&label(id)), Some(id));
        }
        assert_eq!(label(3), "overlay-3");
    }

    #[test]
    fn other_labels_are_not_overlays() {
        for l in [
            "settings",
            "overlay-",
            "overlay-x",
            "overlay--1",
            "overlay-+1",
            "overlay-01",
            "overlay-4294967296",
            "xoverlay-1",
            "overlay-1 ",
        ] {
            assert_eq!(display_id(l), None, "{l}");
        }
    }

    #[test]
    fn diff_creates_missing_and_destroys_unplugged() {
        let d = diff(["settings", "overlay-1", "overlay-2"], &[2, 3]);
        assert_eq!(
            d,
            WindowDiff {
                create: vec![3],
                destroy: vec!["overlay-1".into()],
            }
        );
    }

    #[test]
    fn diff_is_empty_when_displays_match() {
        assert_eq!(
            diff(["overlay-2", "overlay-1"], &[1, 2]),
            WindowDiff::default()
        );
        assert_eq!(diff([], &[]), WindowDiff::default());
    }

    #[test]
    fn diff_from_nothing_creates_every_display() {
        // A display listed twice (hot-plug) still gets one window.
        assert_eq!(diff([], &[1, 2, 1]).create, [1, 2]);
        let d = diff(["settings"], &[5, 7]);
        assert_eq!(d.create, [5, 7]);
        assert!(d.destroy.is_empty());
        let d = diff(["overlay-9", "overlay-8"], &[]);
        assert_eq!(d.destroy, ["overlay-8", "overlay-9"]);
    }

    #[test]
    fn guard_blocks_reentry_until_closed() {
        let g = Guard::default();
        assert!(!g.is_open());
        assert!(g.try_begin());
        assert!(g.is_open());
        assert!(!g.try_begin(), "second press while open is ignored");
        g.end();
        assert!(!g.is_open());
        assert!(g.try_begin());
    }

    #[test]
    fn guard_admits_one_of_many_concurrent_presses() {
        let g = Guard::default();
        let wins = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8).map(|_| s.spawn(|| g.try_begin())).collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .filter(|&won| won)
                .count()
        });
        assert_eq!(wins, 1);
    }

    #[test]
    fn packed_frame_has_size_header_then_pixels() {
        let f = frame(1, 3, 2);
        let bytes = pack_frame(&f);
        assert_eq!(bytes.len(), HEADER_LEN + 3 * 2 * 4);
        assert_eq!(&bytes[..4], &3u32.to_le_bytes());
        assert_eq!(&bytes[4..8], &2u32.to_le_bytes());
        assert_eq!(&bytes[HEADER_LEN..], f.image.as_raw().as_slice());
        // Pixel (2, 1) is the last one: x=2, y=1, 9, 255.
        assert_eq!(&bytes[bytes.len() - 4..], &[2, 1, 9, 255]);
    }
}
