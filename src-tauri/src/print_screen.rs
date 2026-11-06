//! Windows 11 can give PrintScreen to Snipping Tool ("Use the Print screen key to open screen
//! capture" in Settings > Accessibility > Keyboard). Then the default capture shortcut never
//! reaches the app, so at startup it falls back to [`FALLBACK`] and tells the user once. It
//! does the same if another app already has PrintScreen.
//!
//! The setting is `HKCU\Control Panel\Keyboard\PrintScreenKeyForSnippingEnabled` (DWORD,
//! 1 = on). Windows 11 treats a missing value as on. The registry read is [`setting`]; the
//! decision is [`decide`], pure and tested.

use crate::prefs::Prefs;
use crate::shortcut::{self, Registered, Registrar};
use crate::windows::Open;
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogResult};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

/// Used instead of bare PrintScreen when it won't reach the app.
pub const FALLBACK: &str = "Control+Shift+PrintScreen";
const OK: &str = "OK";
const OPEN_SETTINGS: &str = "Open Settings";

/// What to do with a capture shortcut of bare PrintScreen.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// It reaches the app: keep it.
    Keep,
    /// Use [`FALLBACK`]; `save` it as the shortcut when the user never chose one.
    Fallback { save: bool, reason: Reason },
}

/// Why PrintScreen won't reach the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    SnippingTool,
    /// It didn't register: another app has it.
    InUse,
}

impl Reason {
    fn notice(self) -> &'static str {
        match self {
            Reason::SnippingTool => {
                "PrintScreen opens Snipping Tool, so xcreenshot uses Ctrl+Shift+PrintScreen. \
                 You can change this in Settings."
            }
            Reason::InUse => {
                "Another app is using PrintScreen, so xcreenshot uses Ctrl+Shift+PrintScreen. \
                 You can change this in Settings."
            }
        }
    }
}

/// `setting` is `PrintScreenKeyForSnippingEnabled` (`None` if missing), `registered` whether
/// PrintScreen registered, and `chosen` whether the user picked the shortcut in Settings.
///
/// A missing setting counts as on (the Windows 11 default), except for a PrintScreen the user
/// chose: the recorder got the key, so Snipping Tool doesn't have it (Windows 10 leaves it off).
pub fn decide(setting: Option<u32>, registered: bool, chosen: bool) -> Decision {
    let snipping = match setting {
        Some(value) => value != 0,
        None => !chosen,
    };
    let save = !chosen;
    match (snipping, registered) {
        (true, _) => Decision::Fallback {
            save,
            reason: Reason::SnippingTool,
        },
        (false, false) => Decision::Fallback {
            save,
            reason: Reason::InUse,
        },
        (false, true) => Decision::Keep,
    }
}

/// Whether `accelerator` is the bare PrintScreen key.
pub fn is_print_screen(accelerator: &str) -> bool {
    accelerator
        .parse::<Shortcut>()
        .is_ok_and(|s| s.matches(Modifiers::empty(), Code::PrintScreen))
}

/// The shortcut "Reset to default" picks: [`shortcut::DEFAULT`], or [`FALLBACK`] when the
/// default is PrintScreen and Snipping Tool has it.
pub fn default_shortcut(setting: Option<u32>) -> &'static str {
    if is_print_screen(shortcut::DEFAULT) && setting != Some(0) {
        FALLBACK
    } else {
        shortcut::DEFAULT
    }
}

/// Startup, after registering `prefs.shortcut` (`registered` says if it worked): swaps a bare
/// PrintScreen that won't reach the app for [`FALLBACK`]. Saves it in `prefs` if the user never
/// chose a shortcut. Returns the reason to tell the user, once (it marks the notice shown).
/// `setting` reads the Snipping Tool setting; `None` where there is no Snipping Tool.
pub fn fall_back(
    shortcuts: &impl Registrar,
    current: &Registered,
    prefs: &mut Prefs,
    registered: bool,
    setting: Option<impl FnOnce() -> Option<u32>>,
) -> Option<Reason> {
    let setting = setting.filter(|_| is_print_screen(&prefs.shortcut))?;
    let Decision::Fallback { save, reason } = decide(setting(), registered, prefs.shortcut_chosen)
    else {
        return None;
    };
    log::warn!("PrintScreen won't reach xcreenshot ({reason:?}); using {FALLBACK}");
    if let Err(e) = shortcut::swap(shortcuts, current, FALLBACK) {
        log::warn!("could not use {FALLBACK}: {e}");
        return None;
    }
    if save {
        prefs.shortcut = FALLBACK.to_owned();
    }
    (!std::mem::replace(&mut prefs.print_screen_notice_shown, true)).then_some(reason)
}

/// Tells the user why the shortcut moved off PrintScreen, with a button to open Settings.
pub fn tell<R: Runtime>(app: &AppHandle<R>, reason: Reason, open_settings: Open<R>) {
    let handle = app.clone();
    app.dialog()
        .message(reason.notice())
        .title("xcreenshot")
        .buttons(MessageDialogButtons::OkCancelCustom(
            OK.into(),
            OPEN_SETTINGS.into(),
        ))
        .show_with_result(move |answer| {
            if wants_settings(&answer)
                && let Err(e) = open_settings(&handle)
            {
                log::error!("could not open settings: {e}");
            }
        });
}

fn wants_settings(answer: &MessageDialogResult) -> bool {
    matches!(answer, MessageDialogResult::Custom(label) if label == OPEN_SETTINGS)
}

/// `PrintScreenKeyForSnippingEnabled`, or `None` if it is missing. `Some(0)` (off) outside
/// Windows, which has no Snipping Tool.
#[cfg(target_os = "windows")]
pub fn setting() -> Option<u32> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::w;

    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `data` is a u32 and `size` says so; RegGetValueW writes at most that.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Control Panel\\Keyboard"),
            w!("PrintScreenKeyForSnippingEnabled"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast()),
            Some(&raw mut size),
        )
    };
    status.ok().ok().map(|()| data)
}

#[cfg(not(target_os = "windows"))]
pub fn setting() -> Option<u32> {
    Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcut::tests::FakeRegistrar;

    const PRINT_SCREEN: &str = "PrintScreen";

    #[test]
    fn keeps_print_screen_only_when_it_registered_and_snipping_tool_is_off() {
        use Reason::{InUse, SnippingTool};
        let fallback = |save, reason| Decision::Fallback { save, reason };
        assert_eq!(decide(Some(0), true, false), Decision::Keep);
        assert_eq!(decide(Some(0), true, true), Decision::Keep);
        for (setting, registered, reason) in [
            (Some(1), true, SnippingTool),
            (Some(2), true, SnippingTool),
            (None, true, SnippingTool),
            (Some(1), false, SnippingTool),
            (None, false, SnippingTool),
            // Snipping Tool is off: another app has PrintScreen.
            (Some(0), false, InUse),
        ] {
            assert_eq!(
                decide(setting, registered, false),
                fallback(true, reason),
                "{setting:?} {registered}"
            );
        }
        // A chosen PrintScreen falls back for the run only, and a missing setting doesn't
        // count against it.
        for (setting, registered, reason) in [
            (Some(1), true, SnippingTool),
            (Some(0), false, InUse),
            (None, false, InUse),
        ] {
            assert_eq!(
                decide(setting, registered, true),
                fallback(false, reason),
                "{setting:?} {registered}"
            );
        }
        assert_eq!(decide(None, true, true), Decision::Keep, "Windows 10");
    }

    #[test]
    fn the_notice_names_the_reason() {
        assert!(
            Reason::SnippingTool
                .notice()
                .starts_with("PrintScreen opens Snipping Tool")
        );
        let in_use = Reason::InUse.notice();
        assert_eq!(
            in_use,
            "Another app is using PrintScreen, so xcreenshot uses Ctrl+Shift+PrintScreen. \
             You can change this in Settings."
        );
        assert!(!in_use.contains("Snipping"));
    }

    #[test]
    fn only_bare_print_screen_counts() {
        assert!(is_print_screen(PRINT_SCREEN));
        assert!(!is_print_screen(FALLBACK));
        assert!(!is_print_screen("Control+Shift+Super+S"));
        assert!(!is_print_screen("not a shortcut"));
        assert_eq!(shortcut::validate(FALLBACK), Ok(()));
    }

    #[test]
    fn reset_skips_print_screen_only_where_it_is_the_default() {
        assert_eq!(default_shortcut(Some(0)), shortcut::DEFAULT);
        let snipping = if is_print_screen(shortcut::DEFAULT) {
            FALLBACK
        } else {
            shortcut::DEFAULT
        };
        assert_eq!(default_shortcut(Some(1)), snipping);
        assert_eq!(default_shortcut(None), snipping);
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn snipping_tool_is_off_outside_windows() {
        assert_eq!(setting(), Some(0));
    }

    fn print_screen_prefs(chosen: bool) -> Prefs {
        Prefs {
            shortcut: PRINT_SCREEN.into(),
            shortcut_chosen: chosen,
            ..Prefs::default()
        }
    }

    /// Registers PrintScreen like startup does, then falls back if needed.
    fn start(
        shortcuts: &FakeRegistrar,
        prefs: &mut Prefs,
        setting: Option<u32>,
    ) -> (Registered, Option<Reason>) {
        let current = Registered::default();
        let registered = shortcuts.register(&prefs.shortcut).is_ok();
        current.set(&prefs.shortcut, registered);
        let tell = fall_back(shortcuts, &current, prefs, registered, Some(|| setting));
        (current, tell)
    }

    #[test]
    fn snipping_tool_moves_the_default_to_the_fallback_and_tells_once() {
        let shortcuts = FakeRegistrar::default();
        let mut prefs = print_screen_prefs(false);
        let (current, tell) = start(&shortcuts, &mut prefs, None);
        assert_eq!(tell, Some(Reason::SnippingTool));
        assert_eq!(shortcuts.held(), [FALLBACK], "PrintScreen let go");
        assert_eq!(current.active().as_deref(), Some(FALLBACK));
        assert_eq!(prefs.shortcut, FALLBACK, "saved");
        assert!(prefs.print_screen_notice_shown);

        // Reset to PrintScreen later: it falls back again without telling.
        let shortcuts = FakeRegistrar::default();
        prefs.shortcut = PRINT_SCREEN.into();
        let (_, tell) = start(&shortcuts, &mut prefs, Some(1));
        assert_eq!(tell, None);
        assert_eq!(shortcuts.held(), [FALLBACK]);
    }

    #[test]
    fn a_taken_print_screen_falls_back_too() {
        let mut shortcuts = FakeRegistrar::default();
        shortcuts.taken.insert(PRINT_SCREEN.into());
        let mut prefs = print_screen_prefs(false);
        let (current, tell) = start(&shortcuts, &mut prefs, Some(0));
        assert_eq!(tell, Some(Reason::InUse), "Snipping Tool is off");
        assert_eq!(current.active().as_deref(), Some(FALLBACK));
        assert_eq!(prefs.shortcut, FALLBACK);
    }

    #[test]
    fn a_chosen_print_screen_falls_back_for_this_run_only() {
        let shortcuts = FakeRegistrar::default();
        let mut prefs = print_screen_prefs(true);
        let (current, tell) = start(&shortcuts, &mut prefs, Some(1));
        assert_eq!(tell, Some(Reason::SnippingTool));
        assert_eq!(current.active().as_deref(), Some(FALLBACK));
        assert_eq!(prefs.shortcut, PRINT_SCREEN, "the user's choice is kept");
    }

    #[test]
    fn print_screen_that_works_or_another_shortcut_is_left_alone() {
        let shortcuts = FakeRegistrar::default();
        let mut prefs = print_screen_prefs(false);
        let (current, tell) = start(&shortcuts, &mut prefs, Some(0));
        assert_eq!(tell, None);
        assert_eq!(current.active().as_deref(), Some(PRINT_SCREEN));
        assert_eq!(prefs, print_screen_prefs(false));

        // Another shortcut: the setting is not even read.
        let mut prefs = Prefs {
            shortcut: "Alt+S".into(),
            ..Prefs::default()
        };
        let before = prefs.clone();
        let tell = fall_back(
            &shortcuts,
            &current,
            &mut prefs,
            true,
            Some(|| panic!("read the setting")),
        );
        assert_eq!(tell, None);
        assert_eq!(prefs, before);
        // No Snipping Tool (Linux): a taken PrintScreen stays as it is.
        let mut prefs = print_screen_prefs(false);
        let tell = fall_back(&shortcuts, &current, &mut prefs, false, None::<fn() -> _>);
        assert_eq!(tell, None);
        assert_eq!(prefs, print_screen_prefs(false));
    }

    #[test]
    fn a_taken_fallback_keeps_print_screen_and_tells_nothing() {
        let mut shortcuts = FakeRegistrar::default();
        shortcuts.taken.insert(FALLBACK.into());
        let mut prefs = print_screen_prefs(false);
        let (current, tell) = start(&shortcuts, &mut prefs, Some(1));
        assert_eq!(tell, None);
        assert_eq!(current.active().as_deref(), Some(PRINT_SCREEN));
        assert_eq!(prefs, print_screen_prefs(false), "tried again next launch");
    }

    #[test]
    fn only_open_settings_opens_settings() {
        assert!(wants_settings(&MessageDialogResult::Custom(
            OPEN_SETTINGS.into()
        )));
        assert!(!wants_settings(&MessageDialogResult::Custom(OK.into())));
        assert!(!wants_settings(&MessageDialogResult::Cancel));
        assert!(!wants_settings(&MessageDialogResult::Ok));
    }
}
