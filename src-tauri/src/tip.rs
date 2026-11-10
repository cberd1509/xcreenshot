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
use tauri::{AppHandle, Runtime};

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

/// Whether a copy at `t` is in the window before `now`. One in the future (a clock set back)
/// or too far off to subtract is not.
fn recent(t: i64, now: i64) -> bool {
    now.checked_sub(t)
        .is_some_and(|age| (0..WINDOW_MS).contains(&age))
}

impl Tip {
    /// Whether the next capture shows the tip.
    pub fn shows(&self, quick_copy: bool) -> bool {
        self.due && !quick_copy && !self.over()
    }

    /// Whether it can ever show again.
    fn over(&self) -> bool {
        self.done || self.shown >= MAX_SHOWN
    }

    /// Counts a capture that ended with `output` at `now` (ms since the epoch). With quick copy
    /// on, the user knows the mode: the tip ends (this covers quick copy turned on before the tip
    /// existed).
    pub fn record(&mut self, output: Output, quick_copy: bool, now: i64) {
        if quick_copy {
            return self.end();
        }
        if self.over() {
            return;
        }
        match output {
            Output::PlainCopy => {
                // Only copies within the window before this one count; a clock set back
                // drops the ones now in the future.
                self.copies.retain(|&t| recent(t, now));
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
    }

    /// A capture showed the tip: one fewer left, and a new streak is needed for the next.
    pub fn showing(&mut self) {
        self.shown = self.shown.saturating_add(1);
        self.due = false;
        self.copies.clear();
    }

    /// It never shows again.
    pub fn end(&mut self) {
        self.done = true;
        self.due = false;
        self.copies.clear();
    }
}

/// Changes the tip in `storage`'s memory at once, so the next capture reads it; changes are
/// applied in call order, under the prefs lock. Returns whether it changed, and so whether the
/// file needs writing. Kept in memory even if the file can't be written: it holds for this run,
/// and the next save of the prefs writes it.
fn change(storage: &impl Storage, f: impl FnOnce(&mut Tip, bool)) -> bool {
    let mut prefs = storage.load();
    let before = prefs.quick_copy_tip.clone();
    f(&mut prefs.quick_copy_tip, prefs.quick_copy);
    if prefs.quick_copy_tip == before {
        return false;
    }
    storage
        .stage(&prefs)
        .inspect_err(|e| log::warn!("could not keep the quick copy tip: {e}"))
        .is_ok()
}

/// [`change`] in the app's prefs, then writes the file on a worker.
fn update<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(&mut Tip, bool)) {
    let changed = prefs::with_store(app, |store| Ok(change(store, f)))
        .inspect_err(|e| log::warn!("could not change the quick copy tip: {e}"))
        .unwrap_or(false);
    if changed {
        let app = app.clone();
        std::thread::spawn(move || prefs::flush(&app));
    }
}

/// Counts a copy, save or share that just worked (see `overlay::export`). Call it as it
/// happens: the time is taken now, and calls are counted in order.
pub fn record<R: Runtime>(app: &AppHandle<R>, output: Output) {
    let now = chrono::Utc::now().timestamp_millis();
    update(app, |tip, quick_copy| tip.record(output, quick_copy, now));
}

/// A capture showed the tip.
pub fn showing<R: Runtime>(app: &AppHandle<R>) {
    update(app, |tip, _| tip.showing());
}

/// The tip's "Not now": it never shows again.
#[tauri::command]
pub async fn quick_copy_tip_not_now<R: Runtime>(app: AppHandle<R>) {
    update(&app, |tip, _| tip.end());
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
        tip.record(Output::PlainCopy, false, 9 * MIN);
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
        tip.record(Output::Other, false, 2 * MIN);
        assert!(tip.copies.is_empty());
        tip.record(Output::PlainCopy, false, 3 * MIN);
        assert!(!tip.shows(false));
        // Even when due.
        copies(&mut tip, 3, 5 * MIN, MIN);
        assert!(tip.shows(false));
        tip.record(Output::Other, false, 9 * MIN);
        assert!(!tip.shows(false));
        assert_eq!(tip, Tip::default());
    }

    #[test]
    fn ignored_it_shows_once_more_after_another_streak_then_never() {
        let mut tip = Tip::default();
        copies(&mut tip, 3, 0, MIN);
        tip.showing();
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
        tip.end();
        copies(&mut tip, 3, 10 * MIN, MIN);
        assert!(!tip.shows(false));
        assert_eq!(tip.shown, 1);
        assert!(tip.done && tip.copies.is_empty());
    }

    #[test]
    fn copies_with_quick_copy_on_end_it() {
        let mut tip = Tip::default();
        copies(&mut tip, 2, 0, MIN);
        tip.record(Output::PlainCopy, true, 2 * MIN);
        assert!(tip.done && tip.copies.is_empty());
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

    /// [`change`] with a plain copy or another output at `now`.
    fn record_in(storage: &impl Storage, output: Output, now: i64) -> bool {
        change(storage, |tip, quick_copy| {
            tip.record(output, quick_copy, now)
        })
    }

    #[test]
    fn changes_are_kept_in_memory_in_order_and_only_when_something_changed() {
        let storage = FakeStorage::default();
        assert!(!record_in(&storage, Output::Other, 0), "nothing to reset");
        assert_eq!(storage.staged.get(), 0);
        for t in [0, 1, 2] {
            assert!(record_in(&storage, Output::PlainCopy, t));
        }
        // Read at once, as the next capture does; nothing written yet.
        assert!(storage.load().quick_copy_tip.shows(false));
        assert_eq!((storage.staged.get(), storage.saves.get()), (3, 0));
        assert!(change(&storage, |tip, _| tip.showing()));
        assert_eq!(storage.load().quick_copy_tip.shown, 1);
        assert!(change(&storage, |tip, _| tip.end()));
        assert!(!change(&storage, |tip, _| tip.end()));
        assert!(!record_in(&storage, Output::PlainCopy, 3));
        assert!(storage.load().quick_copy_tip.done);

        // Quick copy on in the prefs ends it.
        let on = FakeStorage::default();
        on.prefs.borrow_mut().quick_copy = true;
        record_in(&on, Output::PlainCopy, 0);
        assert!(on.load().quick_copy_tip.done);

        // Without a store nothing is kept (logged), and so nothing is written either.
        assert!(!record_in(&prefs::Unsaved, Output::PlainCopy, 0));
    }

    #[test]
    fn a_nonsense_copy_time_counts_as_old() {
        let mut tip = Tip {
            copies: vec![i64::MIN, i64::MAX, 2 * MIN],
            ..Tip::default()
        };
        tip.record(Output::PlainCopy, false, 3 * MIN);
        assert_eq!(tip.copies, [2 * MIN, 3 * MIN]);
        assert!(!recent(i64::MIN, i64::MAX) && !recent(i64::MAX, i64::MIN));
        assert!(recent(0, WINDOW_MS - 1) && !recent(0, WINDOW_MS));
    }
}
