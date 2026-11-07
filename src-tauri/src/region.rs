//! Remember region: the overlay opens with the last copied or saved selection.
//!
//! A [`Region`] is kept per display arrangement (see [`arrangement`]), so a laptop on its own
//! and the same laptop docked each remember their own; the last [`KEEP`] arrangements are kept,
//! newest first, in [`Prefs::regions`]. A region names its display by id, logical bounds and
//! pixel size (its scale), so a display that changed never gets a region meant for another.
//!
//! Rust saves it after a copy or save (from the session it already holds, never from the page)
//! and sends it with the frame info when the overlay opens. Turning the setting off clears them.
//! Saves run on other threads, so [`ORDER`] numbers them: an older one, or one from before the
//! setting was last turned off, is dropped.

use crate::capture::{Bounds, DisplayFrame, PixelRect};
use crate::prefs::{self, Prefs, Storage};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
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

/// A display as captured: its id, logical bounds and frame size in pixels, which give its scale
/// on each axis exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

impl Monitor {
    /// The display of `frame`, measured from its image (as the crop does).
    pub fn of(frame: &DisplayFrame) -> Self {
        let Bounds {
            x,
            y,
            width,
            height,
        } = frame.logical;
        let (pixel_width, pixel_height) = frame.image.dimensions();
        Self {
            id: frame.id,
            x,
            y,
            width,
            height,
            pixel_width,
            pixel_height,
        }
    }

    /// `px`, a rect in this display's frame pixels, in logical px (the page's scale per axis).
    pub fn to_logical(self, px: PixelRect) -> LogicalRect {
        let sx = f64::from(self.pixel_width) / f64::from(self.width.max(1));
        let sy = f64::from(self.pixel_height) / f64::from(self.height.max(1));
        LogicalRect {
            x: f64::from(px.x) / sx,
            y: f64::from(px.y) / sy,
            width: f64::from(px.width) / sx,
            height: f64::from(px.height) / sy,
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
    pub display: Monitor,
    pub rect: LogicalRect,
}

/// A key for the connected displays: each one's id, logical bounds and pixel size, sorted by id.
/// Plugging, unplugging, moving or rescaling a display gives a different key.
pub fn arrangement(monitors: &[Monitor]) -> String {
    let mut sorted = monitors.to_vec();
    sorted.sort_by_key(|s| s.id);
    sorted
        .iter()
        .map(|s| {
            format!(
                "{}:{},{},{}x{}@{}x{}",
                s.id, s.x, s.y, s.width, s.height, s.pixel_width, s.pixel_height
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// The remembered rect for `monitor` in the arrangement `key`, clamped to it. `None` if there
/// is none, or it was on another display or the display changed.
pub fn find(regions: &[Region], key: &str, monitor: &Monitor) -> Option<LogicalRect> {
    regions
        .iter()
        .find(|r| r.arrangement == key)
        .filter(|r| r.display == *monitor)
        .and_then(|r| monitor.clamp(r.rect))
}

/// Keeps `region` as its arrangement's region, newest first, at most [`KEEP`]. Returns whether
/// anything changed: not while the setting is off (turning it off cleared them), nor for the
/// region already kept.
pub fn remember(prefs: &mut Prefs, region: Region) -> bool {
    if !prefs.remember_region || prefs.regions.first() == Some(&region) {
        return false;
    }
    prefs
        .regions
        .retain(|r| r.arrangement != region.arrangement);
    prefs.regions.insert(0, region);
    prefs.regions.truncate(KEEP);
    true
}

/// Numbers region saves in the order the outputs succeeded.
pub struct Order {
    /// The last number given out.
    issued: AtomicU64,
    /// Saves numbered up to this are dropped: the newest written, or every one numbered before
    /// the setting was last turned off.
    floor: AtomicU64,
}

/// The app's region saves.
pub static ORDER: Order = Order::new();

impl Order {
    pub const fn new() -> Self {
        Self {
            issued: AtomicU64::new(0),
            floor: AtomicU64::new(0),
        }
    }

    /// The number of an output that just succeeded.
    pub fn next(&self) -> u64 {
        self.issued.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The setting was turned off: every save numbered so far is dropped.
    pub fn forget(&self) {
        self.floor
            .fetch_max(self.issued.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    /// Whether save `seq` is newer than any written or forgotten.
    fn admits(&self, seq: u64) -> bool {
        seq > self.floor.load(Ordering::SeqCst)
    }

    /// Save `seq` was written: older ones are dropped from now on.
    fn written(&self, seq: u64) {
        self.floor.fetch_max(seq, Ordering::SeqCst);
    }
}

/// What a capture starts with, read when the overlay opens.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Start {
    /// The saved regions to start from, or `None` while Remember region is off (then the
    /// session remembers nothing either).
    pub regions: Option<Vec<Region>>,
    /// Quick copy is on: the drag copies at once. The capture starts with no region then.
    pub quick_copy: bool,
}

impl Start {
    pub fn of(prefs: Prefs) -> Self {
        // With quick copy on, nothing is restored, but a capture annotated this time may still
        // be remembered.
        let regions = match prefs.quick_copy {
            true => Vec::new(),
            false => prefs.regions,
        };
        Self {
            regions: prefs.remember_region.then_some(regions),
            quick_copy: prefs.quick_copy,
        }
    }
}

/// What the next capture starts with. Never waits for a change being saved.
pub fn load<R: Runtime>(app: &AppHandle<R>) -> Start {
    Start::of(prefs::peek(app))
}

/// Saves `region`, numbered `seq` by [`ORDER`] (see [`remember`]). Writes the file, so call it
/// off the hot path.
pub fn save<R: Runtime>(app: &AppHandle<R>, region: Region, seq: u64) {
    let saved = prefs::with_store(app, |store| save_to(store, region, seq, &ORDER));
    if let Err(e) = saved {
        log::warn!("could not remember the region: {e}");
    }
}

/// [`save`] into `storage`. Writes nothing if nothing changed, or if `order` drops it.
fn save_to(
    storage: &impl Storage,
    region: Region,
    seq: u64,
    order: &Order,
) -> Result<(), &'static str> {
    const UNCHANGED: &str = "unchanged";
    // Under the prefs lock, which orders this with other saves and with `forget`.
    let change = |p: &mut Prefs| {
        (order.admits(seq) && remember(p, region))
            .then_some(())
            .ok_or(UNCHANGED)
    };
    match prefs::modify(storage, change) {
        Ok(_) => {
            order.written(seq);
            Ok(())
        }
        Err(UNCHANGED) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::overlay::tests::frame;
    use crate::prefs::tests::FakeStorage;

    pub(crate) fn monitor(id: u32) -> Monitor {
        Monitor {
            id,
            x: id as i32 * 1000,
            y: 0,
            width: 800,
            height: 600,
            pixel_width: 1600,
            pixel_height: 1200,
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

    fn region(key: &str, display: Monitor) -> Region {
        Region {
            arrangement: key.into(),
            display,
            rect: rect(10.0, 20.0, 30.0, 40.0),
        }
    }

    #[test]
    fn a_monitor_is_its_frames_display_and_measured_scale() {
        let s = Monitor::of(&frame(3, 8, 6));
        assert_eq!(
            s,
            Monitor {
                id: 3,
                x: 3000,
                y: 0,
                width: 4,
                height: 3,
                pixel_width: 8,
                pixel_height: 6
            }
        );
        let px = PixelRect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        };
        assert_eq!(s.to_logical(px), rect(0.5, 1.0, 1.5, 2.0));
        // Each axis has its own scale (rounded logical sizes on Windows).
        let uneven = Monitor {
            pixel_height: 9,
            ..s
        };
        assert_eq!(uneven.to_logical(px), rect(0.5, 2.0 / 3.0, 1.5, 4.0 / 3.0));
    }

    #[test]
    fn the_arrangement_key_is_sorted_and_changes_with_any_display() {
        let (a, b) = (monitor(1), monitor(2));
        let key = arrangement(&[b, a]);
        assert_eq!(key, "1:1000,0,800x600@1600x1200;2:2000,0,800x600@1600x1200");
        assert_eq!(arrangement(&[a, b]), key, "order does not matter");
        assert_ne!(arrangement(&[a]), key, "unplugged");
        let moved = Monitor { y: -600, ..b };
        let rescaled = Monitor {
            pixel_width: 800,
            pixel_height: 600,
            ..b
        };
        let resized = Monitor { width: 1024, ..b };
        let renumbered = Monitor { id: 3, ..b };
        for other in [moved, rescaled, resized, renumbered] {
            assert_ne!(arrangement(&[a, other]), key, "{other:?}");
        }
    }

    #[test]
    fn clamps_to_the_display() {
        let s = monitor(1);
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
        let regions = [region("a", monitor(1)), region("b", monitor(2))];
        assert_eq!(
            find(&regions, "b", &monitor(2)),
            Some(rect(10.0, 20.0, 30.0, 40.0))
        );
        assert_eq!(find(&regions, "b", &monitor(1)), None, "another display");
        let rescaled = Monitor {
            pixel_width: 800,
            ..monitor(2)
        };
        assert_eq!(find(&regions, "b", &rescaled), None, "the display changed");
        assert_eq!(find(&regions, "c", &monitor(1)), None, "no region there");
        let small = Monitor {
            width: 20,
            ..monitor(1)
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
            assert!(remember(&mut prefs, region(&format!("k{i}"), monitor(1))));
        }
        let keys: Vec<_> = prefs
            .regions
            .iter()
            .map(|r| r.arrangement.as_str())
            .collect();
        assert_eq!(keys, ["k9", "k8", "k7", "k6", "k5", "k4", "k3", "k2"]);
        // The same arrangement again replaces its region and moves to the front.
        let mut newer = region("k5", monitor(1));
        newer.rect = rect(1.0, 1.0, 1.0, 1.0);
        assert!(remember(&mut prefs, newer.clone()));
        assert!(!remember(&mut prefs, newer.clone()), "already kept");
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
    fn a_capture_starts_from_the_saved_regions_unless_off_or_quick() {
        let saved = vec![region("a", monitor(1))];
        let prefs = |remember_region, quick_copy| Prefs {
            remember_region,
            quick_copy,
            regions: saved.clone(),
            ..Prefs::default()
        };
        let start = |regions, quick_copy| Start {
            regions,
            quick_copy,
        };
        assert_eq!(
            Start::of(prefs(true, false)),
            start(Some(saved.clone()), false)
        );
        assert_eq!(Start::of(prefs(false, false)), start(None, false));
        assert_eq!(Start::of(prefs(true, true)), start(Some(Vec::new()), true));
        assert_eq!(Start::of(prefs(false, true)), start(None, true));
    }

    #[test]
    fn nothing_is_kept_while_off() {
        let mut prefs = Prefs {
            remember_region: false,
            ..Prefs::default()
        };
        assert!(!remember(&mut prefs, region("b", monitor(1))));
        assert!(prefs.regions.is_empty());
    }

    /// Saves `region` with the next number from `order`.
    fn save_next(storage: &FakeStorage, order: &Order, region: Region) -> Result<(), &'static str> {
        save_to(storage, region, order.next(), order)
    }

    #[test]
    fn saving_writes_the_prefs_unless_off() {
        let storage = FakeStorage::default();
        let order = Order::new();
        assert_eq!(save_next(&storage, &order, region("a", monitor(1))), Ok(()));
        assert_eq!(storage.load().regions, [region("a", monitor(1))]);
        assert_eq!(save_next(&storage, &order, region("a", monitor(1))), Ok(()));
        storage.prefs.borrow_mut().remember_region = false;
        assert_eq!(save_next(&storage, &order, region("b", monitor(1))), Ok(()));
        assert_eq!(
            storage.saves.get(),
            1,
            "nothing written while off or unchanged"
        );
        let failing = FakeStorage::failing();
        assert_eq!(
            save_next(&failing, &order, region("a", monitor(1))),
            Err(prefs::NOT_SAVED)
        );
    }

    #[test]
    fn a_save_that_lands_late_never_replaces_a_newer_one() {
        let storage = FakeStorage::default();
        let order = Order::new();
        let (older, newer) = (order.next(), order.next());
        assert_eq!(
            save_to(&storage, region("new", monitor(1)), newer, &order),
            Ok(())
        );
        assert_eq!(
            save_to(&storage, region("old", monitor(1)), older, &order),
            Ok(())
        );
        assert_eq!(storage.load().regions, [region("new", monitor(1))]);
        assert_eq!(storage.saves.get(), 1);
    }

    #[test]
    fn a_failed_write_does_not_drop_older_saves() {
        let order = Order::new();
        let (older, newer) = (order.next(), order.next());
        let failing = FakeStorage::failing();
        assert!(save_to(&failing, region("new", monitor(1)), newer, &order).is_err());
        let storage = FakeStorage::default();
        assert_eq!(
            save_to(&storage, region("old", monitor(1)), older, &order),
            Ok(())
        );
        assert_eq!(storage.load().regions, [region("old", monitor(1))]);
    }

    #[test]
    fn a_save_from_before_turning_off_never_brings_a_region_back() {
        let storage = FakeStorage::default();
        let order = Order::new();
        let pending = order.next();
        // Off (which clears the regions), then on again before the save lands.
        order.forget();
        assert_eq!(
            save_to(&storage, region("a", monitor(1)), pending, &order),
            Ok(())
        );
        assert!(storage.load().regions.is_empty());
        // Outputs after that are remembered again.
        assert_eq!(save_next(&storage, &order, region("b", monitor(1))), Ok(()));
        assert_eq!(storage.load().regions, [region("b", monitor(1))]);
    }

    #[test]
    fn regions_round_trip_through_the_prefs_json() {
        let mut prefs = Prefs::default();
        assert!(remember(&mut prefs, region("a", monitor(1))));
        let json = serde_json::to_value(&prefs).unwrap();
        assert_eq!(json["regions"][0]["display"]["pixelWidth"], 1600);
        let back: Prefs = serde_json::from_value(json).unwrap();
        assert_eq!(back, prefs);
        // A bad list falls back to none, keeping the other prefs.
        let bad: Prefs =
            serde_json::from_value(serde_json::json!({ "regions": [1], "magnifier": true }))
                .unwrap();
        assert!(bad.regions.is_empty() && bad.magnifier);
    }
}
