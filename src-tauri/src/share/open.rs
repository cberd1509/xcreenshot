//! The share sheets still open: kept until their session ends, they time out, or newer ones
//! push them out. Shared by the macOS and Windows glue, which run it on the main thread.

use std::time::{Duration, Instant};

/// An open sheet whose service never reports back is dropped after this long.
pub const TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// At most this many sheets are kept; the oldest go first.
pub const MAX_OPEN: usize = 8;

/// An open share sheet as [`Open`] sees it.
pub trait Entry {
    /// Its session has ended; nothing will call it any more.
    fn is_over(&self) -> bool;
    /// Gives up on it: the service never reported back.
    fn expire(&self);
}

/// The open sheets. AppKit only holds weak references to its picker delegates, and Windows
/// reports to handlers that need the session, so they live here.
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

    #[cfg(any(test, debug_assertions, target_os = "windows"))]
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|(_, e)| e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

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
