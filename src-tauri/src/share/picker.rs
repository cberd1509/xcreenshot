//! The macOS share sheet's bookkeeping: window levels, menu edge and what the picker reports.
//! Kept out of the AppKit glue (`macos.rs`) so it can be tested.

use super::session::Event;
use std::collections::HashMap;

/// `NSPopUpMenuWindowLevel`: the picker's menu is a pop-up menu window.
pub const POP_UP_MENU_LEVEL: isize = 101;
/// A window above the menu (the overlay, at the screen saver level) waits here while the menu
/// is open: under the menu, still over the menu bar (24) and the Dock (20).
pub const BELOW_MENUS: isize = POP_UP_MENU_LEVEL - 1;
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

/// `NSUserCancelledError` (3072, `FoundationErrors.h`), in `NSCocoaErrorDomain`: the user
/// cancelled inside the service.
const USER_CANCELLED: (&str, isize) = ("NSCocoaErrorDomain", 3072);

/// What to log for `sharingService:didFailToShareItems:error:`, given the error's domain, code
/// and description: a cancel inside the service is not a failure. A failure names its domain
/// and code, so other kinds of cancel can be told apart later.
pub fn failure_log(domain: &str, code: isize, description: &str) -> (log::Level, String) {
    if (domain, code) == USER_CANCELLED {
        (log::Level::Info, "share cancelled".into())
    } else {
        let message = format!("sharing failed: {description} ({domain} {code})");
        (log::Level::Warn, message)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn levels_match_appkit() {
        assert_eq!(POP_UP_MENU_LEVEL, objc2_app_kit::NSPopUpMenuWindowLevel);
        assert!(BELOW_MENUS > objc2_app_kit::NSMainMenuWindowLevel);
    }

    #[test]
    fn a_cancel_inside_the_service_is_not_a_failure() {
        let cancelled = "The operation was cancelled.";
        assert_eq!(
            failure_log("NSCocoaErrorDomain", 3072, cancelled),
            (log::Level::Info, "share cancelled".into())
        );
        for (domain, code) in [("NSCocoaErrorDomain", 4), ("NSPOSIXErrorDomain", 3072)] {
            assert_eq!(
                failure_log(domain, code, "Nope."),
                (
                    log::Level::Warn,
                    format!("sharing failed: Nope. ({domain} {code})")
                )
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_cancel_matches_foundation() {
        // SAFETY: a constant NSString that Foundation defines.
        let domain = unsafe { objc2_foundation::NSCocoaErrorDomain };
        assert_eq!(domain.to_string(), USER_CANCELLED.0);
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
}
