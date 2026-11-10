//! The Windows share sheet's decisions (`DataTransferManager`): what its notices mean for the
//! session, the title it shows, and which windows were taken out of the topmost band while it
//! is open. Kept out of the WinRT glue (`windows.rs`) so they can be tested on any platform.

use super::session::Event;
use std::collections::HashSet;
use std::path::Path;

/// What the Windows share sheet reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// An app was picked (`TargetApplicationChosen`), with its name.
    Chosen(String),
    /// The app has taken the data (`ShareCompleted`), with its name if known. Some never say.
    Completed(Option<String>),
    /// The sheet was dismissed (`ShareCanceled`), or its data could not be filled in.
    Canceled,
}

/// The session events for `notice`. A completion also counts as a choice, for a target that
/// only reports that; the session ignores a second choice.
pub fn events(notice: Notice) -> Vec<Event> {
    match notice {
        Notice::Chosen(app) => vec![Event::Chosen(app)],
        Notice::Completed(app) => vec![Event::Chosen(app.unwrap_or_default()), Event::Finished],
        Notice::Canceled => vec![Event::Cancelled],
    }
}

/// The title the sheet shows: the file's name. Windows won't share without a title.
pub fn title(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "xcreenshot".to_owned())
}

/// The windows taken out of the topmost band for the sheet, by label. A topmost window (the
/// overlay) could cover the sheet, so it waits as a normal window while the sheet is open.
#[derive(Debug, Default)]
pub struct Lowered(HashSet<String>);

impl Lowered {
    /// Whether to take `window` out of the topmost band before showing the sheet.
    pub fn lower(&mut self, window: &str, topmost: bool) -> bool {
        if topmost {
            self.0.insert(window.to_owned());
        }
        topmost
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

    #[test]
    fn a_pick_is_a_choice() {
        assert_eq!(
            events(Notice::Chosen("Mail".into())),
            [Event::Chosen("Mail".into())]
        );
    }

    #[test]
    fn a_completion_is_a_choice_that_finished() {
        assert_eq!(
            events(Notice::Completed(Some("Mail".into()))),
            [Event::Chosen("Mail".into()), Event::Finished]
        );
        assert_eq!(
            events(Notice::Completed(None)),
            [Event::Chosen(String::new()), Event::Finished]
        );
    }

    #[test]
    fn a_dismissal_cancels() {
        assert_eq!(events(Notice::Canceled), [Event::Cancelled]);
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
        assert!(lowered.lower("overlay-1", true));
        assert!(lowered.restore("overlay-1", false));
        assert!(!lowered.restore("overlay-1", false), "only once");
    }

    #[test]
    fn a_normal_window_is_left_alone() {
        let mut lowered = Lowered::default();
        assert!(!lowered.lower("settings", false));
        assert!(!lowered.restore("settings", false));
    }

    #[test]
    fn a_window_raised_by_someone_else_is_left_alone() {
        let mut lowered = Lowered::default();
        lowered.lower("overlay-1", true);
        assert!(!lowered.restore("overlay-1", true));
        assert!(!lowered.restore("overlay-1", false), "forgotten");
    }

    #[test]
    fn windows_are_tracked_separately() {
        let mut lowered = Lowered::default();
        lowered.lower("overlay-1", true);
        lowered.lower("overlay-2", true);
        assert!(lowered.restore("overlay-2", false));
        assert!(lowered.restore("overlay-1", false));
    }
}
