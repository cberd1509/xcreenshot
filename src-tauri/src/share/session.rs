//! One open share sheet: turns what the picker reports into the caller's outcome (once) and
//! the temp file's release (once).

use std::cell::Cell;
use std::time::Duration;

/// What the user did with the share sheet. Sent once, on the main thread, as soon as the menu
/// closes: before the chosen service shows its own UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareOutcome {
    /// A service was picked; its menu title, e.g. "AirDrop".
    Chosen(String),
    /// The sheet was dismissed without a choice.
    Cancelled,
}

/// What the picker reports. `Chosen` or `Cancelled` when the menu closes; after `Chosen`,
/// `Finished` once the service reports success or failure (some never do). An open picker that
/// expires sends `Finished` too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Chosen(String),
    Cancelled,
    Finished,
}

pub type OnDone = Box<dyn FnOnce(ShareOutcome) + Send + 'static>;
/// Deletes the temp file after the given delay.
pub type Release = Box<dyn FnOnce(Duration) + Send + 'static>;

/// Callbacks are taken out before they run, so one that reenters (e.g. the overlay closing
/// itself and AppKit dismissing the picker) finds nothing left to call.
pub struct Session {
    on_done: Cell<Option<OnDone>>,
    release: Cell<Option<Release>>,
    /// How long the file is kept after the service reports back: some read it late.
    grace: Duration,
}

impl Session {
    pub fn new(on_done: OnDone, release: Release, grace: Duration) -> Self {
        Self {
            on_done: Cell::new(Some(on_done)),
            release: Cell::new(Some(release)),
            grace,
        }
    }

    pub fn handle(&self, event: Event) {
        match event {
            Event::Chosen(name) => {
                self.report(ShareOutcome::Chosen(name));
            }
            // Only a dismissal before any choice frees the file at once: after a choice the
            // service may still be reading it.
            Event::Cancelled => {
                if self.report(ShareOutcome::Cancelled) {
                    self.release(Duration::ZERO);
                }
            }
            // Finished with no outcome (an open picker expired): it counts as dismissed, so
            // `on_done` always runs.
            Event::Finished => {
                if self.report(ShareOutcome::Cancelled) {
                    self.release(Duration::ZERO);
                } else {
                    self.release(self.grace);
                }
            }
        }
    }

    /// Whether the temp file has been handed off for deletion.
    pub fn is_over(&self) -> bool {
        let release = self.release.take();
        let over = release.is_none();
        self.release.set(release);
        over
    }

    /// Sends `outcome` unless one was sent already. Returns whether it was sent.
    fn report(&self, outcome: ShareOutcome) -> bool {
        self.on_done.take().map(|f| f(outcome)).is_some()
    }

    fn release(&self, delay: Duration) {
        if let Some(release) = self.release.take() {
            release(delay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const GRACE: Duration = Duration::from_secs(30);

    /// A session recording what it reported and when it released.
    fn session() -> (Session, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (a, b) = (log.clone(), log.clone());
        let session = Session::new(
            Box::new(move |o| a.lock().unwrap().push(format!("{o:?}"))),
            Box::new(move |d| b.lock().unwrap().push(format!("release {d:?}"))),
            GRACE,
        );
        (session, log)
    }

    fn run(events: &[Event]) -> Vec<String> {
        let (session, log) = session();
        for e in events {
            session.handle(e.clone());
        }
        log.lock().unwrap().clone()
    }

    #[test]
    fn cancelling_reports_and_releases_at_once() {
        assert_eq!(run(&[Event::Cancelled]), ["Cancelled", "release 0ns"]);
    }

    #[test]
    fn choosing_reports_at_once_and_releases_after_the_grace_period() {
        assert_eq!(
            run(&[Event::Chosen("AirDrop".into()), Event::Finished]),
            [r#"Chosen("AirDrop")"#, "release 30s"]
        );
    }

    #[test]
    fn a_service_that_never_reports_back_keeps_the_file() {
        let (session, log) = session();
        session.handle(Event::Chosen("Notes".into()));
        assert!(!session.is_over());
        assert_eq!(*log.lock().unwrap(), [r#"Chosen("Notes")"#]);
    }

    #[test]
    fn each_callback_runs_once() {
        let events = [
            Event::Chosen("Mail".into()),
            Event::Cancelled,
            Event::Chosen("Messages".into()),
            Event::Finished,
            Event::Finished,
        ];
        assert_eq!(run(&events), [r#"Chosen("Mail")"#, "release 30s"]);
        assert_eq!(
            run(&[Event::Cancelled, Event::Cancelled, Event::Finished]),
            ["Cancelled", "release 0ns"]
        );
    }

    #[test]
    fn a_picker_that_ends_without_an_answer_counts_as_dismissed() {
        assert_eq!(run(&[Event::Finished]), ["Cancelled", "release 0ns"]);
    }

    #[test]
    fn over_once_released() {
        let (session, _) = session();
        assert!(!session.is_over());
        session.handle(Event::Cancelled);
        assert!(session.is_over());
    }
}
