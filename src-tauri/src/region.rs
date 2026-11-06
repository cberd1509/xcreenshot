//! Remember region: the overlay opens with the last copied or saved selection.
//!
//! A [`Region`] is kept per display arrangement (see [`arrangement`]), so a laptop on its own
//! and the same laptop docked each remember their own; the last [`KEEP`] arrangements are kept,
//! newest first, in [`Prefs::regions`]. A region names its display by id, logical bounds and
//! scale, so a display that changed never gets a region meant for another.
//!
//! Rust saves it after a copy or save (from the session it already holds, never from the page)
//! and sends it with the frame info when the overlay opens. Turning the setting off clears them.

use crate::capture::{Bounds, DisplayFrame, PixelRect};
use crate::prefs::{self, Prefs, Storage};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

/// How many display arrangements keep a region.
pub const KEEP: usize = 8;

/// A rect in logical px, relative to its display's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A display as captured: its id, logical bounds and scale (frame px per logical px).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

impl Screen {
    /// The display of `frame`, with the scale measured from its image (as the crop does).
    pub fn of(frame: &DisplayFrame) -> Self {
        let Bounds {
            x,
            y,
            width,
            height,
        } = frame.logical;
        Self {
            id: frame.id,
            x,
            y,
            width,
            height,
            scale: f64::from(frame.image.width()) / f64::from(width.max(1)),
        }
    }

    /// `px`, a rect in this display's frame pixels, in logical px.
    pub fn to_logical(self, px: PixelRect) -> LogicalRect {
        let s = self.scale;
        LogicalRect {
            x: f64::from(px.x) / s,
            y: f64::from(px.y) / s,
            width: f64::from(px.width) / s,
            height: f64::from(px.height) / s,
        }
    }

    /// `rect` cut to this display, or `None` if nothing of it is on the display.
    pub fn clamp(&self, rect: LogicalRect) -> Option<LogicalRect> {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let left = rect.x.clamp(0.0, w);
        let top = rect.y.clamp(0.0, h);
        let right = (rect.x + rect.width).clamp(0.0, w);
        let bottom = (rect.y + rect.height).clamp(0.0, h);
        // `!(a > b)` is also true for NaN.
        if !(right > left && bottom > top) {
            return None;
        }
        Some(LogicalRect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

/// The last selection copied or saved on one display arrangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// See [`arrangement`].
    pub arrangement: String,
    pub display: Screen,
    pub rect: LogicalRect,
}

/// A key for the connected displays: each one's id, logical bounds and scale, sorted by id.
/// Plugging, unplugging, moving or rescaling a display gives a different key.
pub fn arrangement(screens: &[Screen]) -> String {
    let mut sorted = screens.to_vec();
    sorted.sort_by_key(|s| s.id);
    sorted
        .iter()
        .map(|s| {
            format!(
                "{}:{},{},{}x{}@{}",
                s.id, s.x, s.y, s.width, s.height, s.scale
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// The remembered rect for `screen` in the arrangement `key`, clamped to it. `None` if there
/// is none, or it was on another display or the display changed.
pub fn find(regions: &[Region], key: &str, screen: &Screen) -> Option<LogicalRect> {
    regions
        .iter()
        .find(|r| r.arrangement == key)
        .filter(|r| r.display == *screen)
        .and_then(|r| screen.clamp(r.rect))
}

/// Why a region was not saved.
const OFF: &str = "Remember region is off.";

/// Keeps `region` as its arrangement's region, newest first, at most [`KEEP`]. Nothing changes
/// if the setting is off.
pub fn remember(prefs: &mut Prefs, region: Region) -> Result<(), &'static str> {
    if !prefs.remember_region {
        return Err(OFF);
    }
    prefs
        .regions
        .retain(|r| r.arrangement != region.arrangement);
    prefs.regions.insert(0, region);
    prefs.regions.truncate(KEEP);
    Ok(())
}

/// The regions to restore from: none while the setting is off.
pub fn recall(prefs: Prefs) -> Vec<Region> {
    if prefs.remember_region {
        prefs.regions
    } else {
        Vec::new()
    }
}

/// The saved regions, read when the overlay opens. Never waits for a change being saved.
pub fn load<R: Runtime>(app: &AppHandle<R>) -> Vec<Region> {
    recall(prefs::peek(app))
}

/// Saves `region` (see [`remember`]). Writes the file, so call it off the hot path.
pub fn save<R: Runtime>(app: &AppHandle<R>, region: Region) {
    let saved = prefs::with_store(app, |store| save_to(store, region));
    if let Err(e) = saved {
        log::warn!("could not remember the region: {e}");
    }
}

/// [`save`] into `storage`. Off is not an error.
fn save_to(storage: &impl Storage, region: Region) -> Result<(), &'static str> {
    match prefs::modify(storage, |p| remember(p, region)) {
        Ok(_) | Err(OFF) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::overlay::tests::frame;
    use crate::prefs::tests::FakeStorage;

    pub(crate) fn screen(id: u32) -> Screen {
        Screen {
            id,
            x: id as i32 * 1000,
            y: 0,
            width: 800,
            height: 600,
            scale: 2.0,
        }
    }

    pub(crate) fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
        LogicalRect {
            x,
            y,
            width,
            height,
        }
    }

    fn region(key: &str, display: Screen) -> Region {
        Region {
            arrangement: key.into(),
            display,
            rect: rect(10.0, 20.0, 30.0, 40.0),
        }
    }

    #[test]
    fn a_screen_is_its_frames_display_and_measured_scale() {
        let s = Screen::of(&frame(3, 8, 6));
        assert_eq!(
            s,
            Screen {
                id: 3,
                x: 3000,
                y: 0,
                width: 4,
                height: 3,
                scale: 2.0
            }
        );
        let px = PixelRect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        };
        assert_eq!(s.to_logical(px), rect(0.5, 1.0, 1.5, 2.0));
    }

    #[test]
    fn the_arrangement_key_is_sorted_and_changes_with_any_display() {
        let (a, b) = (screen(1), screen(2));
        let key = arrangement(&[b, a]);
        assert_eq!(key, "1:1000,0,800x600@2;2:2000,0,800x600@2");
        assert_eq!(arrangement(&[a, b]), key, "order does not matter");
        assert_ne!(arrangement(&[a]), key, "unplugged");
        let moved = Screen { y: -600, ..b };
        let rescaled = Screen { scale: 1.0, ..b };
        let resized = Screen { width: 1024, ..b };
        let renumbered = Screen { id: 3, ..b };
        for other in [moved, rescaled, resized, renumbered] {
            assert_ne!(arrangement(&[a, other]), key, "{other:?}");
        }
    }

    #[test]
    fn clamps_to_the_display() {
        let s = screen(1);
        let r = rect(10.0, 20.0, 30.0, 40.0);
        assert_eq!(s.clamp(r), Some(r));
        assert_eq!(
            s.clamp(rect(-10.0, 590.0, 900.0, 40.0)),
            Some(rect(0.0, 590.0, 800.0, 10.0))
        );
        for off in [
            rect(800.0, 0.0, 10.0, 10.0),
            rect(-20.0, 0.0, 10.0, 10.0),
            rect(0.0, 0.0, 0.0, 10.0),
            rect(f64::NAN, 0.0, 10.0, 10.0),
        ] {
            assert_eq!(s.clamp(off), None, "{off:?}");
        }
    }

    #[test]
    fn finds_only_its_arrangement_and_unchanged_display() {
        let regions = [region("a", screen(1)), region("b", screen(2))];
        assert_eq!(
            find(&regions, "b", &screen(2)),
            Some(rect(10.0, 20.0, 30.0, 40.0))
        );
        assert_eq!(find(&regions, "b", &screen(1)), None, "another display");
        let rescaled = Screen {
            scale: 1.0,
            ..screen(2)
        };
        assert_eq!(find(&regions, "b", &rescaled), None, "the display changed");
        assert_eq!(find(&regions, "c", &screen(1)), None, "no region there");
        let small = Screen {
            width: 20,
            ..screen(1)
        };
        let mut far = region("a", small);
        far.rect = rect(10.0, 0.0, 30.0, 5.0);
        assert_eq!(
            find(&[far], "a", &small),
            Some(rect(10.0, 0.0, 10.0, 5.0)),
            "clamped"
        );
    }

    #[test]
    fn keeps_the_newest_per_arrangement_up_to_the_cap() {
        let mut prefs = Prefs::default();
        for i in 0..10 {
            remember(&mut prefs, region(&format!("k{i}"), screen(1))).unwrap();
        }
        let keys: Vec<_> = prefs
            .regions
            .iter()
            .map(|r| r.arrangement.as_str())
            .collect();
        assert_eq!(keys, ["k9", "k8", "k7", "k6", "k5", "k4", "k3", "k2"]);
        // The same arrangement again replaces its region and moves to the front.
        let mut newer = region("k5", screen(1));
        newer.rect = rect(1.0, 1.0, 1.0, 1.0);
        remember(&mut prefs, newer.clone()).unwrap();
        assert_eq!(prefs.regions.len(), KEEP);
        assert_eq!(prefs.regions[0], newer);
        assert_eq!(
            prefs
                .regions
                .iter()
                .filter(|r| r.arrangement == "k5")
                .count(),
            1
        );
    }

    #[test]
    fn nothing_is_kept_or_recalled_while_off() {
        let mut prefs = Prefs::default();
        remember(&mut prefs, region("a", screen(1))).unwrap();
        assert_eq!(recall(prefs.clone()).len(), 1);
        prefs.remember_region = false;
        assert_eq!(remember(&mut prefs, region("b", screen(1))), Err(OFF));
        assert_eq!(prefs.regions.len(), 1);
        assert!(recall(prefs).is_empty());
    }

    #[test]
    fn saving_writes_the_prefs_unless_off() {
        let storage = FakeStorage::default();
        assert_eq!(save_to(&storage, region("a", screen(1))), Ok(()));
        assert_eq!(storage.load().regions, [region("a", screen(1))]);
        storage.prefs.borrow_mut().remember_region = false;
        assert_eq!(save_to(&storage, region("b", screen(1))), Ok(()));
        assert_eq!(storage.saves.get(), 1, "nothing written while off");
        let failing = FakeStorage::failing();
        assert_eq!(
            save_to(&failing, region("a", screen(1))),
            Err(prefs::NOT_SAVED)
        );
    }

    #[test]
    fn regions_round_trip_through_the_prefs_json() {
        let mut prefs = Prefs::default();
        remember(&mut prefs, region("a", screen(1))).unwrap();
        let json = serde_json::to_value(&prefs).unwrap();
        assert_eq!(json["regions"][0]["display"]["scale"], 2.0);
        let back: Prefs = serde_json::from_value(json).unwrap();
        assert_eq!(back, prefs);
        // A bad list falls back to none, keeping the other prefs.
        let bad: Prefs =
            serde_json::from_value(serde_json::json!({ "regions": [1], "magnifier": true }))
                .unwrap();
        assert!(bad.regions.is_empty() && bad.magnifier);
    }
}
