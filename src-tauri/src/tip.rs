//! The quick copy tip: after a few plain copies in a row, the next capture suggests quick copy.
//!
//! The rules follow the `QuickCopyTip.dc.html` artboard. While quick copy is off, [`STREAK`]
//! captures copied in a row with no annotations, within [`WINDOW_MS`], make the tip due. The next
//! capture shows it in place of the hint pill, before the drag. "Not now" ends it for good;
//! ignored, it can show again after another streak, [`MAX_SHOWN`] times in all. An annotated
//! copy, a save or a share resets the streak. Turning quick copy on (the tip's "Turn on" or
//! Settings) ends it too: the user knows the mode then.
//!
//! The state is [`Tip`], kept in the prefs. Whether a capture shows the tip is read with the
//! rest of its start from the prefs already in memory (see `region::load`), so the hotkey waits
//! for nothing. Recording a copy and counting a showing write the prefs, on a worker.

use crate::prefs::{self, Storage};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::Store;

/// Plain copies in a row that make the tip due.
pub const STREAK: usize = 3;
/// The time the whole streak must fit in, in ms.
pub const WINDOW_MS: i64 = 10 * 60 * 1000;
/// The most times the tip is ever shown.
pub const MAX_SHOWN: u32 = 2;

/// The tip's state, in the prefs as `quickCopyTip`. Missing in prefs from before it: the
/// defaults, so no migration is needed.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tip {
    /// When the plain copies of the streak under way were made, in ms since the Unix epoch,
    /// oldest first.
    pub copies: Vec<i64>,
    /// A streak was reached: the next capture shows the tip.
    pub due: bool,
    /// How many captures showed it.
    pub shown: u32,
    /// It never shows again: "Not now", or quick copy was turned on.
    pub done: bool,
}

/// How a capture ended, as far as the tip cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// Copied with no annotations.
    PlainCopy,
    /// Copied with annotations, saved or shared: the streak starts over.
    Other,
}

impl Tip {
    /// Whether the next capture shows the tip.
    pub fn shows(&self, quick_copy: bool) -> bool {
        self.due && !self.done && !quick_copy && self.shown < MAX_SHOWN
    }

    /// Whether it can ever show again.
    fn over(&self) -> bool {
        self.done || self.shown >= MAX_SHOWN
    }

    /// Counts a capture that ended with `output` at `now` (ms since the epoch). With quick copy
    /// on, the user knows the mode: the tip ends. Returns whether anything changed.
    pub fn record(&mut self, output: Output, quick_copy: bool, now: i64) -> bool {
        if quick_copy {
            return self.end();
        }
        if self.over() {
            return false;
        }
        let before = self.clone();
        match output {
            Output::PlainCopy => {
                // Only copies within the window before this one count; a clock set back
                // drops the ones now in the future.
                self.copies.retain(|&t| t <= now && now - t < WINDOW_MS);
                self.copies.push(now);
                if self.copies.len() >= STREAK {
                    self.copies.clear();
                    self.due = true;
                }
            }
            Output::Other => {
                self.copies.clear();
                self.due = false;
            }
        }
        *self != before
    }

    /// A capture showed the tip: one fewer left, and a new streak is needed for the next.
    pub fn showing(&mut self) -> bool {
        self.shown = self.shown.saturating_add(1);
        self.due = false;
        self.copies.clear();
        true
    }

    /// It never shows again. Returns whether that is new.
    pub fn end(&mut self) -> bool {
        let changed = !self.done || self.due || !self.copies.is_empty();
        self.done = true;
        self.due = false;
        self.copies.clear();
        changed
    }
}

/// Changes the tip in `storage` with `change`, saving only if it says something changed.
fn change(storage: &impl Storage, change: impl FnOnce(&mut prefs::Prefs) -> bool) {
    const UNCHANGED: &str = "unchanged";
    let changed = prefs::modify(storage, |p| change(p).then_some(()).ok_or(UNCHANGED));
    match changed {
        Ok(_) | Err(UNCHANGED) => {}
        Err(e) => log::warn!("could not save the quick copy tip: {e}"),
    }
}

fn record_in(storage: &impl Storage, output: Output, now: i64) {
    change(storage, |p| {
        let quick_copy = p.quick_copy;
        p.quick_copy_tip.record(output, quick_copy, now)
    });
}

fn showing_in(storage: &impl Storage) {
    change(storage, |p| p.quick_copy_tip.showing());
}

fn end_in(storage: &impl Storage) {
    change(storage, |p| p.quick_copy_tip.end());
}

/// Runs `f` with the app's prefs store. Writes the file, so call it off the hot path.
fn with_prefs<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(&Store<R>)) {
    let opened = prefs::with_store(app, |store| {
        f(store);
        Ok(())
    });
    if let Err(e) = opened {
        log::warn!("could not save the quick copy tip: {e}");
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Counts a copy, save or share that worked (see `overlay::export`).
pub fn record<R: Runtime>(app: &AppHandle<R>, output: Output) {
    let now = now_ms();
    with_prefs(app, |store| record_in(store, output, now));
}

/// A capture opened with the tip.
pub fn showing<R: Runtime>(app: &AppHandle<R>) {
    with_prefs(app, showing_in);
}

/// The tip's "Not now": it never shows again.
#[tauri::command]
pub async fn quick_copy_tip_not_now<R: Runtime>(app: AppHandle<R>) {
    with_prefs(&app, end_in);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::Prefs;
    use crate::prefs::tests::FakeStorage;
    use serde_json::json;

    const MIN: i64 = 60 * 1000;

    /// `n` plain copies, `gap` ms apart, from `start`.
    fn copies(tip: &mut Tip, n: usize, start: i64, gap: i64) {
        for i in 0..n {
            tip.record(Output::PlainCopy, false, start + gap * i as i64);
        }
    }

    #[test]
    fn three_plain_copies_within_ten_minutes_make_it_due() {
        let mut tip = Tip::default();
        assert!(!tip.shows(false));
        copies(&mut tip, 2, 0, MIN);
        assert!(!tip.shows(false), "two are not a streak");
        assert!(tip.record(Output::PlainCopy, false, 9 * MIN));
        assert!(tip.shows(false));
        assert!(tip.copies.is_empty());
        assert!(!tip.shows(true), "not while quick copy is on");
    }

    #[test]
    fn copies_further_apart_than_the_window_do_not_count() {
        let mut tip = Tip::default();
        copies(&mut tip, 3, 0, 5 * MIN);
        assert!(
            !tip.shows(false),
            "the first is 10 minutes before the third"
        );
        assert_eq!(tip.copies, [5 * MIN, 10 * MIN]);
        tip.record(Output::PlainCopy, false, 14 * MIN);
        assert!(tip.shows(false), "the last three fit");
    }

    #[test]
    fn a_clock_set_back_drops_the_copies_in_the_future() {
        let mut tip = Tip::default();
        copies(&mut tip, 2, 10 * MIN, MIN);
        tip.record(Output::PlainCopy, false, MIN);
        assert_eq!(tip.copies, [MIN]);
    }

    #[test]
    fn an_annotated_copy_a_save_or_a_share_resets_the_streak() {
        let mut tip = Tip::default();
        copies(&mut tip, 2, 0, MIN);
        assert!(tip.record(Output::Other, false, 2 * MIN));
        tip.record(Output::PlainCopy, false, 3 * MIN);
        assert!(!tip.shows(false));
        assert!(tip.record(Output::Other, false, 4 * MIN));
        // Even when due.
        copies(&mut tip, 3, 5 * MIN, MIN);
        assert!(tip.shows(false));
        tip.record(Output::Other, false, 9 * MIN);
        assert!(!tip.shows(false));
        assert!(
            !tip.record(Output::Other, false, 10 * MIN),
            "nothing to reset"
        );
    }

    #[test]
    fn ignored_it_shows_once_more_after_another_streak_then_never() {
        let mut tip = Tip::default();
        copies(&mut tip, 3, 0, MIN);
        assert!(tip.showing());
        assert_eq!(tip.shown, 1);
        assert!(!tip.shows(false), "a new streak is needed");
        copies(&mut tip, 3, 10 * MIN, MIN);
        assert!(tip.shows(false));
        tip.showing();
        copies(&mut tip, 3, 20 * MIN, MIN);
        assert!(!tip.shows(false), "at most twice ever");
        assert!(
            tip.copies.is_empty() && !tip.due,
            "nothing is counted any more"
        );
    }

    #[test]
    fn not_now_ends_it_for_good() {
        let mut tip = Tip::default();
        copies(&mut tip, 3, 0, MIN);
        tip.showing();
        assert!(tip.end());
        assert!(!tip.end(), "already over");
        copies(&mut tip, 3, 10 * MIN, MIN);
        assert!(!tip.shows(false));
        assert_eq!(tip.shown, 1);
    }

    #[test]
    fn copies_with_quick_copy_on_end_it() {
        let mut tip = Tip::default();
        copies(&mut tip, 2, 0, MIN);
        assert!(tip.record(Output::PlainCopy, true, 2 * MIN));
        assert!(tip.done && tip.copies.is_empty());
        assert!(!tip.record(Output::PlainCopy, true, 3 * MIN));
    }

    #[test]
    fn it_is_stored_in_the_prefs_and_defaults_when_missing_or_invalid() {
        let tip = Tip {
            copies: vec![1, 2],
            due: true,
            shown: 1,
            done: false,
        };
        let prefs = Prefs {
            quick_copy_tip: tip.clone(),
            ..Prefs::default()
        };
        let stored = serde_json::to_value(&prefs).unwrap();
        assert_eq!(
            stored["quickCopyTip"],
            json!({ "copies": [1, 2], "due": true, "shown": 1, "done": false })
        );
        let back: Prefs = serde_json::from_value(stored).unwrap();
        assert_eq!(back.quick_copy_tip, tip);
        for (stored, expected) in [
            (json!({}), Tip::default()),
            (json!({ "quickCopyTip": "yes" }), Tip::default()),
            (json!({ "quickCopyTip": { "shown": -1 } }), Tip::default()),
            (
                json!({ "quickCopyTip": { "done": true } }),
                Tip {
                    done: true,
                    ..Tip::default()
                },
            ),
        ] {
            let prefs: Prefs = serde_json::from_value(stored.clone()).unwrap();
            assert_eq!(prefs.quick_copy_tip, expected, "{stored}");
        }
    }

    #[test]
    fn changes_are_saved_only_when_something_changed() {
        let storage = FakeStorage::default();
        record_in(&storage, Output::Other, 0);
        assert_eq!(storage.saves.get(), 0, "nothing to reset");
        record_in(&storage, Output::PlainCopy, 0);
        assert_eq!(storage.load().quick_copy_tip.copies, [0]);
        showing_in(&storage);
        assert_eq!(storage.load().quick_copy_tip.shown, 1);
        end_in(&storage);
        assert!(storage.load().quick_copy_tip.done);
        let saves = storage.saves.get();
        end_in(&storage);
        record_in(&storage, Output::PlainCopy, 1);
        assert_eq!(storage.saves.get(), saves);

        // Quick copy on in the prefs ends it.
        let on = FakeStorage::default();
        on.prefs.borrow_mut().quick_copy = true;
        record_in(&on, Output::PlainCopy, 0);
        assert!(on.load().quick_copy_tip.done);

        // A failed save is logged only.
        let failing = FakeStorage::failing();
        record_in(&failing, Output::PlainCopy, 0);
        assert_eq!(failing.saves.get(), 1);
    }

    #[test]
    fn now_is_after_the_epoch() {
        assert!(now_ms() > 0);
    }
}
