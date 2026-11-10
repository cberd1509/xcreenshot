//! The Windows share sheet's decisions (`DataTransferManager`): what its notices mean for the
//! session, the title it shows, when its handlers go, and which windows were taken out of the
//! topmost band while it is open. Kept out of the WinRT glue (`windows.rs`) so they can be
//! tested on any platform.

use super::open::Entry;
use super::session::{Event, Session};
use std::cell::Cell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

/// What the Windows share sheet reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// An app was picked (`TargetApplicationChosen`), with its name (empty if unknown).
    Chosen(String),
    /// The app has taken the data (`ShareCompleted`). Some never say.
    Completed,
    /// The sheet was dismissed (`ShareCanceled`), or there was no data to fill in.
    Canceled,
}

/// The session events for `notice`. A completion also counts as a choice, for a target that
/// only reports that; the session ignores a second choice.
fn events(notice: Notice) -> Vec<Event> {
    match notice {
        Notice::Chosen(app) => vec![Event::Chosen(app)],
        Notice::Completed => vec![Event::Chosen(String::new()), Event::Finished],
        Notice::Canceled => vec![Event::Cancelled],
    }
}

/// The title the sheet shows: the file's name. Windows won't share without a title.
pub fn title(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "xcreenshot".to_owned())
}

/// The handlers a sheet registered on its window's `DataTransferManager`.
pub trait Handlers {
    /// Unregisters them.
    fn remove(self);
}

/// One share, on the main thread: its session and, until the sheet closes, its handlers.
pub struct Sheet<H: Handlers> {
    pub id: u64,
    session: Session,
    handlers: Cell<Option<H>>,
    /// The sheet never showed: the session is dropped without a word.
    abandoned: Cell<bool>,
}

impl<H: Handlers> Sheet<H> {
    pub fn new(id: u64, session: Session, handlers: H) -> Self {
        Self {
            id,
            session,
            handlers: Cell::new(Some(handlers)),
            abandoned: Cell::new(false),
        }
    }

    /// Any notice means the sheet has closed: its handlers go first, so the window's next
    /// share doesn't reach this one. Then the session hears what happened.
    pub fn notify(&self, notice: Notice) {
        self.detach();
        for event in events(notice) {
            self.session.handle(event);
        }
    }

    /// The sheet could not be shown: nothing is reported, and it can be dropped.
    pub fn abandon(&self) {
        self.abandoned.set(true);
        self.detach();
    }

    fn detach(&self) {
        if let Some(handlers) = self.handlers.take() {
            handlers.remove();
        }
    }
}

impl<H: Handlers> Entry for Rc<Sheet<H>> {
    fn is_over(&self) -> bool {
        self.abandoned.get() || self.session.is_over()
    }

    fn expire(&self) {
        self.detach();
        self.session.handle(Event::Finished);
    }
}

/// The windows taken out of the topmost band for the sheet, by label. A topmost window (the
/// overlay) could cover the sheet, so it waits as a normal window while the sheet is open.
#[derive(Debug, Default)]
pub struct Lowered(HashSet<String>);

impl Lowered {
    /// Records that `window` was taken out of the topmost band for the sheet.
    pub fn lower(&mut self, window: &str) {
        self.0.insert(window.to_owned());
    }

    /// Whether to put `window` back in the topmost band: we took it out, and nobody has put it
    /// back since.
    pub fn restore(&mut self, window: &str, topmost: bool) -> bool {
        self.0.remove(window) && !topmost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    type Log = Arc<Mutex<Vec<String>>>;

    /// Records its removal in the log.
    struct Fake(Log);

    impl Handlers for Fake {
        fn remove(self) {
            self.0.lock().unwrap().push("removed".into());
        }
    }

    /// A sheet logging its handlers' removal, the outcome and the file's release.
    fn sheet() -> (Rc<Sheet<Fake>>, Log) {
        let log = Log::default();
        let (a, b) = (log.clone(), log.clone());
        let session = Session::new(
            Box::new(move |o| a.lock().unwrap().push(format!("{o:?}"))),
            Box::new(move |d| b.lock().unwrap().push(format!("release {d:?}"))),
            Duration::from_secs(30),
        );
        (Rc::new(Sheet::new(7, session, Fake(log.clone()))), log)
    }

    fn run(notices: &[Notice]) -> (Vec<String>, bool) {
        let (sheet, log) = sheet();
        for n in notices {
            sheet.notify(n.clone());
        }
        let log = log.lock().unwrap().clone();
        (log, sheet.is_over())
    }

    #[test]
    fn a_dismissal_removes_the_handlers_then_cancels() {
        assert_eq!(
            run(&[Notice::Canceled]),
            (
                vec!["removed".into(), "Cancelled".into(), "release 0ns".into()],
                true
            )
        );
    }

    #[test]
    fn a_pick_reports_the_app_and_keeps_the_file_until_it_completes() {
        let (log, over) = run(&[Notice::Chosen("Mail".into())]);
        assert_eq!(log, ["removed", r#"Chosen("Mail")"#]);
        assert!(!over);
        let (log, over) = run(&[Notice::Chosen("Mail".into()), Notice::Completed]);
        assert_eq!(log, ["removed", r#"Chosen("Mail")"#, "release 30s"]);
        assert!(over);
    }

    #[test]
    fn a_completion_alone_counts_as_a_choice() {
        let (log, over) = run(&[Notice::Completed]);
        assert_eq!(log, ["removed", r#"Chosen("")"#, "release 30s"]);
        assert!(over);
    }

    #[test]
    fn an_abandoned_sheet_is_over_without_a_word() {
        let (sheet, log) = sheet();
        assert_eq!(sheet.id, 7);
        assert!(!sheet.is_over());
        sheet.abandon();
        assert!(sheet.is_over());
        assert_eq!(*log.lock().unwrap(), ["removed"]);
    }

    #[test]
    fn an_expired_sheet_counts_as_dismissed() {
        let (sheet, log) = sheet();
        sheet.expire();
        assert!(sheet.is_over());
        assert_eq!(
            *log.lock().unwrap(),
            ["removed", "Cancelled", "release 0ns"]
        );
    }

    #[test]
    fn the_title_is_the_file_name() {
        let path = Path::new("cache")
            .join("share")
            .join("xcreenshot-2026-11-06-13.22.05.png");
        assert_eq!(title(&path), "xcreenshot-2026-11-06-13.22.05.png");
        assert_eq!(title(Path::new("")), "xcreenshot");
    }

    #[test]
    fn a_topmost_window_is_lowered_and_put_back_once() {
        let mut lowered = Lowered::default();
        lowered.lower("overlay-1");
        assert!(lowered.restore("overlay-1", false));
        assert!(!lowered.restore("overlay-1", false), "only once");
    }

    #[test]
    fn a_window_never_lowered_is_left_alone() {
        let mut lowered = Lowered::default();
        assert!(!lowered.restore("settings", false));
    }

    #[test]
    fn a_window_raised_by_someone_else_is_left_alone() {
        let mut lowered = Lowered::default();
        lowered.lower("overlay-1");
        assert!(!lowered.restore("overlay-1", true));
        assert!(!lowered.restore("overlay-1", false), "forgotten");
    }

    #[test]
    fn windows_are_tracked_separately() {
        let mut lowered = Lowered::default();
        lowered.lower("overlay-1");
        lowered.lower("overlay-2");
        assert!(lowered.restore("overlay-2", false));
        assert!(lowered.restore("overlay-1", false));
    }
}
