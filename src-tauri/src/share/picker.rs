//! The share sheet's bookkeeping: window levels, menu edge, what the picker reports and which
//! pickers are still open. Kept out of the AppKit glue (`macos.rs`) so it can be tested.

use super::session::Event;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// `NSPopUpMenuWindowLevel`: the picker's menu is a pop-up menu window.
pub const POP_UP_MENU_LEVEL: isize = 101;
/// A window above the menu (the overlay, at the screen saver level) waits here while the menu
/// is open: under the menu, still over the menu bar (24) and the Dock (20).
pub const BELOW_MENUS: isize = POP_UP_MENU_LEVEL - 1;
/// An open picker whose service never reports back is dropped after this long.
pub const TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// At most this many pickers are kept; the oldest go first.
pub const MAX_OPEN: usize = 8;

/// The edge of the anchor the menu opens from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    MinY,
    MaxY,
}

/// The anchor's bottom edge: MaxY in a flipped view (origin top left), MinY otherwise.
pub fn edge_below(flipped: bool) -> Edge {
    if flipped { Edge::MaxY } else { Edge::MinY }
}

/// `sharingServicePicker:didChooseSharingService:` gives the chosen service's title, or none
/// when the menu was dismissed.
pub fn event_for(service: Option<String>) -> Event {
    service.map_or(Event::Cancelled, Event::Chosen)
}

/// The levels of windows lowered for the menu, by window.
#[derive(Debug, Default)]
pub struct Levels {
    lowered: HashMap<usize, isize>,
}

impl Levels {
    /// The level to give `window` before showing the menu, if it would hide it. A window that
    /// an earlier share already lowered stays as it is, and keeps its original level on record.
    pub fn lower(&mut self, window: usize, current: isize) -> Option<isize> {
        (current > BELOW_MENUS).then(|| {
            self.lowered.insert(window, current);
            BELOW_MENUS
        })
    }

    /// The level to put back on `window`: its original one, if we lowered it and nobody has
    /// changed it since.
    pub fn restore(&mut self, window: usize, current: isize) -> Option<isize> {
        let original = self.lowered.remove(&window)?;
        (current == BELOW_MENUS).then_some(original)
    }
}

/// An open picker as [`Open`] sees it.
pub trait Entry {
    /// Its session has ended; nothing will call it any more.
    fn is_over(&self) -> bool;
    /// Gives up on it: the service never reported back.
    fn expire(&self);
}

/// The open pickers. AppKit only holds weak references to their delegates, so they live here
/// until their session ends, they time out, or newer ones push them out.
#[derive(Debug)]
pub struct Open<T> {
    entries: Vec<(Instant, T)>,
}

impl<T> Default for Open<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<T: Entry> Open<T> {
    pub fn push(&mut self, now: Instant, entry: T) -> Vec<T> {
        self.entries.push((now, entry));
        self.prune(now)
    }

    /// Takes out the entries that are over, expired or too many, expiring those still open.
    /// The caller drops what is returned once it no longer borrows `self`, never from inside
    /// one of their own callbacks.
    pub fn prune(&mut self, now: Instant) -> Vec<T> {
        let excess = self.entries.len().saturating_sub(MAX_OPEN);
        let (gone, kept) = std::mem::take(&mut self.entries)
            .into_iter()
            .enumerate()
            .partition::<Vec<_>, _>(|(i, (opened, e))| {
                *i < excess || e.is_over() || now.duration_since(*opened) >= TIMEOUT
            });
        self.entries = kept.into_iter().map(|(_, entry)| entry).collect();
        gone.into_iter()
            .map(|(_, (_, e))| {
                if !e.is_over() {
                    e.expire();
                }
                e
            })
            .collect()
    }

    #[cfg(any(test, debug_assertions))]
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|(_, e)| e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[cfg(target_os = "macos")]
    #[test]
    fn levels_match_appkit() {
        assert_eq!(POP_UP_MENU_LEVEL, objc2_app_kit::NSPopUpMenuWindowLevel);
        assert!(BELOW_MENUS > objc2_app_kit::NSMainMenuWindowLevel);
    }

    #[test]
    fn the_menu_opens_below_the_anchor() {
        assert_eq!(edge_below(true), Edge::MaxY);
        assert_eq!(edge_below(false), Edge::MinY);
    }

    #[test]
    fn a_choice_or_a_dismissal() {
        assert_eq!(
            event_for(Some("AirDrop".into())),
            Event::Chosen("AirDrop".into())
        );
        assert_eq!(event_for(None), Event::Cancelled);
    }

    const SCREEN_SAVER: isize = 1000;

    #[test]
    fn a_window_over_the_menu_is_lowered_and_put_back() {
        let mut levels = Levels::default();
        assert_eq!(levels.lower(1, SCREEN_SAVER), Some(BELOW_MENUS));
        assert_eq!(levels.restore(1, BELOW_MENUS), Some(SCREEN_SAVER));
        assert_eq!(levels.restore(1, BELOW_MENUS), None, "only once");
    }

    #[test]
    fn a_window_under_the_menu_is_left_alone() {
        let mut levels = Levels::default();
        assert_eq!(levels.lower(1, 0), None);
        assert_eq!(levels.lower(1, BELOW_MENUS), None);
        assert_eq!(levels.restore(1, BELOW_MENUS), None);
    }

    #[test]
    fn a_level_changed_by_someone_else_is_kept() {
        let mut levels = Levels::default();
        levels.lower(1, SCREEN_SAVER);
        assert_eq!(levels.restore(1, 3), None);
    }

    #[test]
    fn a_second_share_while_lowered_still_restores_the_original_level() {
        let mut levels = Levels::default();
        levels.lower(1, SCREEN_SAVER);
        // The first share's service was chosen, so its window stayed lowered.
        assert_eq!(levels.lower(1, BELOW_MENUS), None);
        assert_eq!(levels.restore(1, BELOW_MENUS), Some(SCREEN_SAVER));
    }

    #[test]
    fn windows_are_tracked_separately() {
        let mut levels = Levels::default();
        levels.lower(1, SCREEN_SAVER);
        levels.lower(2, 500);
        assert_eq!(levels.restore(2, BELOW_MENUS), Some(500));
        assert_eq!(levels.restore(1, BELOW_MENUS), Some(SCREEN_SAVER));
    }

    #[derive(Clone)]
    struct Fake {
        id: u32,
        over: Rc<Cell<bool>>,
        expired: Rc<Cell<bool>>,
    }

    impl Fake {
        fn new(id: u32) -> Self {
            Self {
                id,
                over: Rc::default(),
                expired: Rc::default(),
            }
        }
    }

    impl Entry for Fake {
        fn is_over(&self) -> bool {
            self.over.get()
        }
        fn expire(&self) {
            self.expired.set(true);
            self.over.set(true);
        }
    }

    fn ids(entries: &[Fake]) -> Vec<u32> {
        entries.iter().map(|e| e.id).collect()
    }

    #[test]
    fn finished_pickers_are_dropped_without_expiring() {
        let t0 = Instant::now();
        let mut open = Open::default();
        let (a, b) = (Fake::new(1), Fake::new(2));
        assert!(open.push(t0, a.clone()).is_empty());
        assert!(open.push(t0, b.clone()).is_empty());
        a.over.set(true);
        assert_eq!(ids(&open.prune(t0)), [1]);
        assert!(!a.expired.get());
        assert_eq!(open.iter().count(), 1);
        assert_eq!(open.iter().next().unwrap().id, 2);
    }

    #[test]
    fn pickers_that_never_report_back_expire() {
        let t0 = Instant::now();
        let mut open = Open::default();
        let old = Fake::new(1);
        open.push(t0, old.clone());
        open.push(t0 + TIMEOUT / 2, Fake::new(2));
        assert!(open.prune(t0 + TIMEOUT - Duration::from_secs(1)).is_empty());
        assert_eq!(ids(&open.prune(t0 + TIMEOUT)), [1]);
        assert!(old.expired.get());
        assert_eq!(open.iter().count(), 1);
    }

    #[test]
    fn the_list_is_bounded() {
        let t0 = Instant::now();
        let mut open = Open::default();
        let first = Fake::new(0);
        open.push(t0, first.clone());
        for id in 1..MAX_OPEN as u32 {
            assert!(open.push(t0, Fake::new(id)).is_empty());
        }
        assert_eq!(ids(&open.push(t0, Fake::new(99))), [0]);
        assert!(first.expired.get());
        assert_eq!(open.iter().count(), MAX_OPEN);
    }
}
