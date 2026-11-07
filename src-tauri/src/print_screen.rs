//! The Windows default capture shortcut is bare PrintScreen, but Windows 11 can give that key
//! to Snipping Tool ("Use the Print screen key to open screen capture" in Settings >
//! Accessibility > Keyboard), and another app can hold it. Then the default is [`FALLBACK`]:
//! at startup, on "Reset to default", and the recorder refuses PrintScreen while Snipping Tool
//! has it. So the saved shortcut is always one that works.
//!
//! The setting is `HKCU\Control Panel\Keyboard\PrintScreenKeyForSnippingEnabled` (DWORD,
//! 1 = on). A missing value is on in Windows 11 (build 22000 and later) and off in Windows 10.
//! The registry reads are the `os` module; the decisions here are pure and tested.

use crate::notice::{self, Notice};
use crate::prefs::Prefs;
use crate::shortcut::{self, Registered, Registrar};
use crate::windows::Open;
use tauri::{AppHandle, Runtime};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

/// Used instead of bare PrintScreen when it won't reach the app.
pub const FALLBACK: &str = "Control+Shift+PrintScreen";
/// The recorder's error for PrintScreen while Snipping Tool has it.
pub const TAKEN_BY_SNIPPING_TOOL: &str =
    "Snipping Tool uses PrintScreen. Turn that off in Windows Settings, or pick another shortcut.";
/// First Windows 11 build: a missing setting is on from here.
const WINDOWS_11: u32 = 22000;

/// Whether Snipping Tool has PrintScreen, read when needed. `None` where there is no Snipping
/// Tool (outside Windows): PrintScreen is then never replaced.
pub type SnippingTool = Option<fn() -> bool>;

#[cfg(target_os = "windows")]
pub const SNIPPING_TOOL: SnippingTool = Some(os::has_print_screen);
#[cfg(not(target_os = "windows"))]
pub const SNIPPING_TOOL: SnippingTool = None;

/// Why PrintScreen won't reach the app. The notice page has the copy for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    SnippingTool,
    /// It didn't register: another app has it.
    InUse,
}

/// Why PrintScreen won't reach the app, or `None` if it will. `snipping` is whether Snipping
/// Tool has it; `registered` whether registering it worked.
pub fn decide(snipping: bool, registered: bool) -> Option<Reason> {
    if snipping {
        Some(Reason::SnippingTool)
    } else if !registered {
        Some(Reason::InUse)
    } else {
        None
    }
}

/// Whether Snipping Tool has PrintScreen, from `PrintScreenKeyForSnippingEnabled` (`None` if
/// missing) and the Windows build (`None` if unknown, taken as recent).
#[cfg_attr(
    not(any(target_os = "windows", test)),
    expect(dead_code, reason = "used by the Windows os module")
)]
fn snipping_owns(setting: Option<u32>, build: Option<u32>) -> bool {
    match setting {
        Some(value) => value != 0,
        None => build.is_none_or(|b| b >= WINDOWS_11),
    }
}

/// The build number from the registry's `CurrentBuildNumber` (UTF-16, may end in NULs).
#[cfg_attr(
    not(any(target_os = "windows", test)),
    expect(dead_code, reason = "used by the Windows os module")
)]
fn parse_build(utf16: &[u16]) -> Option<u32> {
    let len = utf16.iter().position(|&c| c == 0).unwrap_or(utf16.len());
    String::from_utf16(&utf16[..len]).ok()?.trim().parse().ok()
}

/// Whether `accelerator` is the bare PrintScreen key.
pub fn is_print_screen(accelerator: &str) -> bool {
    accelerator
        .parse::<Shortcut>()
        .is_ok_and(|s| s.matches(Modifiers::empty(), Code::PrintScreen))
}

fn has_print_screen(snipping: SnippingTool) -> bool {
    snipping.is_some_and(|read| read())
}

/// The default capture shortcut: [`shortcut::DEFAULT`], or [`FALLBACK`] when that is
/// PrintScreen and Snipping Tool has it or it can't be registered.
pub fn default_shortcut(snipping: bool, can_register: bool) -> &'static str {
    default_from(shortcut::DEFAULT, snipping, can_register)
}

fn default_from(base: &'static str, snipping: bool, can_register: bool) -> &'static str {
    if is_print_screen(base) && decide(snipping, can_register).is_some() {
        FALLBACK
    } else {
        base
    }
}

/// The recorder picked `accelerator`: refuses PrintScreen while Snipping Tool has it.
pub fn check_pick(accelerator: &str, snipping: SnippingTool) -> Result<(), &'static str> {
    if is_print_screen(accelerator) && has_print_screen(snipping) {
        return Err(TAKEN_BY_SNIPPING_TOOL);
    }
    Ok(())
}

/// "Reset to default": registers the default shortcut (see [`default_shortcut`]) in place of
/// the current one and returns it.
pub fn use_default(
    shortcuts: &impl Registrar,
    current: &Registered,
    snipping: SnippingTool,
) -> Result<&'static str, &'static str> {
    use_default_from(default_shortcut, shortcuts, current, snipping)
}

/// [`use_default`] with the default from `default` (as [`default_shortcut`]).
fn use_default_from(
    default: fn(bool, bool) -> &'static str,
    shortcuts: &impl Registrar,
    current: &Registered,
    snipping: SnippingTool,
) -> Result<&'static str, &'static str> {
    // No Snipping Tool: the plain default, even if PrintScreen is taken.
    let Some(read) = snipping else {
        let base = default(false, true);
        return shortcut::swap(shortcuts, current, base).map(|()| base);
    };
    let snipping = read();
    let first = default(snipping, true);
    match shortcut::swap(shortcuts, current, first) {
        Err(shortcut::IN_USE) if default(snipping, false) != first => {
            shortcut::swap(shortcuts, current, FALLBACK).map(|()| FALLBACK)
        }
        result => result.map(|()| first),
    }
}

/// Startup, after registering `prefs.shortcut` (`registered` says if it worked): replaces a
/// PrintScreen that won't reach the app with [`FALLBACK`], in `prefs` too so Settings shows the
/// shortcut that works. Returns the reason to tell the user, once (it marks the notice shown).
pub fn at_startup(
    shortcuts: &impl Registrar,
    current: &Registered,
    prefs: &mut Prefs,
    registered: bool,
    snipping: SnippingTool,
) -> Option<Reason> {
    let read = snipping.filter(|_| is_print_screen(&prefs.shortcut))?;
    let reason = decide(read(), registered)?;
    log::warn!("PrintScreen won't reach xcreenshot ({reason:?}); using {FALLBACK}");
    if let Err(e) = shortcut::swap(shortcuts, current, FALLBACK) {
        log::warn!("could not use {FALLBACK}: {e}");
        return None;
    }
    prefs.shortcut = FALLBACK.to_owned();
    (!std::mem::replace(&mut prefs.print_screen_notice_shown, true)).then_some(reason)
}

/// Tells the user why the shortcut moved off PrintScreen, with a button to open Settings.
/// Returns at once; the task ends when the notice is answered.
pub fn tell<R: Runtime>(
    app: &AppHandle<R>,
    reason: Reason,
    open_settings: Open<R>,
) -> tauri::async_runtime::JoinHandle<()> {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let notice = Notice::ShortcutChanged {
            reason,
            accelerator: FALLBACK.to_owned(),
        };
        // "Open Settings" is the second button; "OK" the main one.
        if notice::ask(&app, notice).await != Some(false) {
            return;
        }
        let handle = app.clone();
        let opened = app.run_on_main_thread(move || {
            if let Err(e) = open_settings(&handle) {
                log::error!("could not open settings: {e}");
            }
        });
        if let Err(e) = opened {
            log::error!("could not open settings: {e}");
        }
    })
}

#[cfg(target_os = "windows")]
mod os {
    use std::ffi::c_void;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD,
        RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::core::{PCWSTR, w};

    pub fn has_print_screen() -> bool {
        super::snipping_owns(setting(), build())
    }

    fn setting() -> Option<u32> {
        let mut data = 0u32;
        read(
            HKEY_CURRENT_USER,
            w!("Control Panel\\Keyboard"),
            w!("PrintScreenKeyForSnippingEnabled"),
            RRF_RT_REG_DWORD,
            (&raw mut data).cast(),
            size_of::<u32>(),
        )?;
        Some(data)
    }

    fn build() -> Option<u32> {
        let mut buf = [0u16; 16];
        read(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            buf.as_mut_ptr().cast(),
            size_of_val(&buf),
        )?;
        super::parse_build(&buf)
    }

    fn read(
        key: HKEY,
        subkey: PCWSTR,
        value: PCWSTR,
        kind: REG_ROUTINE_FLAGS,
        data: *mut c_void,
        len: usize,
    ) -> Option<()> {
        let mut size = len as u32;
        // SAFETY: `data` points to `len` writable bytes and `size` says so; RegGetValueW
        // writes at most that (strings come back NUL-terminated).
        let status = unsafe {
            RegGetValueW(
                key,
                subkey,
                value,
                kind,
                None,
                Some(data),
                Some(&raw mut size),
            )
        };
        status.ok().ok()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::shortcut::tests::FakeRegistrar;

    const PRINT_SCREEN: &str = "PrintScreen";
    pub const ON: SnippingTool = Some(|| true);
    pub const OFF: SnippingTool = Some(|| false);

    #[test]
    fn print_screen_works_only_when_it_registered_and_snipping_tool_is_off() {
        assert_eq!(decide(false, true), None);
        assert_eq!(decide(true, true), Some(Reason::SnippingTool));
        assert_eq!(decide(true, false), Some(Reason::SnippingTool));
        assert_eq!(decide(false, false), Some(Reason::InUse));
    }

    #[test]
    fn a_missing_setting_is_on_from_windows_11() {
        for build in [None, Some(19045), Some(22000), Some(26100)] {
            assert!(snipping_owns(Some(1), build));
            assert!(!snipping_owns(Some(0), build));
        }
        assert!(!snipping_owns(None, Some(19045)), "Windows 10");
        assert!(!snipping_owns(None, Some(21999)));
        assert!(snipping_owns(None, Some(22000)), "Windows 11");
        assert!(snipping_owns(None, Some(26100)));
        assert!(snipping_owns(None, None), "unknown build: recent");
    }

    #[test]
    fn parses_the_registry_build() {
        let utf16 = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        assert_eq!(parse_build(&utf16("26100\0\0\0")), Some(26100));
        assert_eq!(parse_build(&utf16(" 19045 ")), Some(19045));
        assert_eq!(parse_build(&utf16("")), None);
        assert_eq!(parse_build(&utf16("x\0")), None);
        assert_eq!(parse_build(&[0xD800]), None, "not UTF-16");
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
    fn the_default_skips_a_print_screen_that_wont_work() {
        assert_eq!(default_from(PRINT_SCREEN, false, true), PRINT_SCREEN);
        assert_eq!(default_from(PRINT_SCREEN, true, true), FALLBACK);
        assert_eq!(default_from(PRINT_SCREEN, false, false), FALLBACK);
        // macOS: never PrintScreen, nothing to skip.
        assert_eq!(default_from("Alt+S", true, false), "Alt+S");
        // The production default goes the same way.
        assert_eq!(default_shortcut(false, true), shortcut::DEFAULT);
        let skipped = default_from(shortcut::DEFAULT, true, true);
        assert_eq!(default_shortcut(true, true), skipped);
    }

    #[test]
    fn the_recorder_refuses_print_screen_only_while_snipping_tool_has_it() {
        assert_eq!(check_pick(PRINT_SCREEN, ON), Err(TAKEN_BY_SNIPPING_TOOL));
        assert_eq!(check_pick(PRINT_SCREEN, OFF), Ok(()));
        assert_eq!(check_pick(PRINT_SCREEN, None), Ok(()), "no Snipping Tool");
        assert_eq!(check_pick(FALLBACK, ON), Ok(()));
    }

    /// The default as on Windows, whatever this platform is.
    fn windows_default(snipping: bool, can_register: bool) -> &'static str {
        default_from(PRINT_SCREEN, snipping, can_register)
    }

    fn holding(accelerator: &str) -> (FakeRegistrar, Registered) {
        let current = Registered::default();
        current.set(accelerator, true);
        (FakeRegistrar::holding(accelerator), current)
    }

    #[test]
    fn reset_picks_print_screen_or_the_fallback() {
        let (shortcuts, current) = holding("Alt+S");
        assert_eq!(
            use_default_from(windows_default, &shortcuts, &current, OFF),
            Ok(PRINT_SCREEN)
        );
        assert_eq!(shortcuts.held(), [PRINT_SCREEN]);

        let (shortcuts, current) = holding("Alt+S");
        assert_eq!(
            use_default_from(windows_default, &shortcuts, &current, ON),
            Ok(FALLBACK)
        );
        assert_eq!(shortcuts.held(), [FALLBACK]);

        // Another app has PrintScreen.
        let (mut shortcuts, current) = holding("Alt+S");
        shortcuts.taken.insert(PRINT_SCREEN.into());
        assert_eq!(
            use_default_from(windows_default, &shortcuts, &current, OFF),
            Ok(FALLBACK)
        );
        assert_eq!(current.active().as_deref(), Some(FALLBACK));

        // Both taken: the old one stays.
        let (mut shortcuts, current) = holding("Alt+S");
        shortcuts
            .taken
            .extend([PRINT_SCREEN.into(), FALLBACK.into()]);
        assert_eq!(
            use_default_from(windows_default, &shortcuts, &current, OFF),
            Err(shortcut::IN_USE)
        );
        assert_eq!(current.active().as_deref(), Some("Alt+S"));
    }

    #[test]
    fn reset_without_snipping_tool_is_the_plain_default() {
        let (shortcuts, current) = holding("Alt+S");
        assert_eq!(
            use_default_from(windows_default, &shortcuts, &current, None),
            Ok(PRINT_SCREEN)
        );
        // The production wiring: this platform's default.
        let (shortcuts, current) = holding("Alt+S");
        let expected = default_shortcut(has_print_screen(SNIPPING_TOOL), true);
        assert_eq!(
            use_default(&shortcuts, &current, SNIPPING_TOOL),
            Ok(expected)
        );
    }

    fn print_screen_prefs() -> Prefs {
        Prefs {
            shortcut: PRINT_SCREEN.into(),
            ..Prefs::default()
        }
    }

    /// Registers the saved shortcut like startup does, then replaces it if needed.
    fn start(
        shortcuts: &FakeRegistrar,
        prefs: &mut Prefs,
        snipping: SnippingTool,
    ) -> (Registered, Option<Reason>) {
        let current = Registered::default();
        let registered = shortcuts.register(&prefs.shortcut).is_ok();
        current.set(&prefs.shortcut, registered);
        let tell = at_startup(shortcuts, &current, prefs, registered, snipping);
        (current, tell)
    }

    #[test]
    fn snipping_tool_moves_print_screen_to_the_fallback_and_tells_once() {
        let shortcuts = FakeRegistrar::default();
        let mut prefs = print_screen_prefs();
        let (current, tell) = start(&shortcuts, &mut prefs, ON);
        assert_eq!(tell, Some(Reason::SnippingTool));
        assert_eq!(shortcuts.held(), [FALLBACK], "PrintScreen let go");
        assert_eq!(current.active().as_deref(), Some(FALLBACK));
        assert_eq!(prefs.shortcut, FALLBACK, "saved");
        assert!(prefs.print_screen_notice_shown);

        // PrintScreen again later (an old file): replaced without telling.
        let shortcuts = FakeRegistrar::default();
        prefs.shortcut = PRINT_SCREEN.into();
        let (_, tell) = start(&shortcuts, &mut prefs, ON);
        assert_eq!(tell, None);
        assert_eq!(shortcuts.held(), [FALLBACK]);
    }

    #[test]
    fn a_taken_print_screen_is_replaced_and_blames_another_app() {
        let mut shortcuts = FakeRegistrar::default();
        shortcuts.taken.insert(PRINT_SCREEN.into());
        let mut prefs = print_screen_prefs();
        let (current, tell) = start(&shortcuts, &mut prefs, OFF);
        assert_eq!(tell, Some(Reason::InUse));
        assert_eq!(current.active().as_deref(), Some(FALLBACK));
        assert_eq!(prefs.shortcut, FALLBACK);
    }

    #[test]
    fn a_working_print_screen_or_another_shortcut_is_left_alone() {
        let shortcuts = FakeRegistrar::default();
        let mut prefs = print_screen_prefs();
        let (current, tell) = start(&shortcuts, &mut prefs, OFF);
        assert_eq!(tell, None);
        assert_eq!(current.active().as_deref(), Some(PRINT_SCREEN));
        assert_eq!(prefs, print_screen_prefs());

        // Another shortcut: Snipping Tool is not even checked.
        let mut prefs = Prefs {
            shortcut: "Alt+S".into(),
            ..Prefs::default()
        };
        let before = prefs.clone();
        let tell = at_startup(
            &shortcuts,
            &current,
            &mut prefs,
            true,
            Some(|| panic!("checked Snipping Tool")),
        );
        assert_eq!(tell, None);
        assert_eq!(prefs, before);

        // No Snipping Tool (Linux): a taken PrintScreen stays as it is.
        let mut prefs = print_screen_prefs();
        assert_eq!(
            at_startup(&shortcuts, &current, &mut prefs, false, None),
            None
        );
        assert_eq!(prefs, print_screen_prefs());
    }

    #[test]
    fn a_taken_fallback_keeps_print_screen_and_tells_nothing() {
        let mut shortcuts = FakeRegistrar::default();
        shortcuts.taken.insert(FALLBACK.into());
        let mut prefs = print_screen_prefs();
        let (current, tell) = start(&shortcuts, &mut prefs, ON);
        assert_eq!(tell, None);
        assert_eq!(current.active().as_deref(), Some(PRINT_SCREEN));
        assert_eq!(prefs, print_screen_prefs(), "tried again next launch");
    }

    #[test]
    fn the_notice_shows_the_fallback_and_open_settings_opens_settings() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use tauri::Manager;
        use tauri::async_runtime::block_on;
        use tauri::test::MockRuntime;
        static OPENED: AtomicU32 = AtomicU32::new(0);
        let app = notice::tests::app();
        let h = app.handle();
        // Counts the calls; failing also covers the logged error.
        let open: Open<MockRuntime> = |_| {
            OPENED.fetch_add(1, Ordering::SeqCst);
            Err(tauri::Error::WindowNotFound)
        };
        let answer = |reason, primary| {
            let told = tell(h, reason, open);
            let shown = Notice::ShortcutChanged {
                reason,
                accelerator: FALLBACK.into(),
            };
            let label = notice::tests::opened(h, &shown);
            notice::notice_answer(h.get_webview_window(&label).unwrap(), primary).unwrap();
            block_on(told).unwrap();
        };

        answer(Reason::InUse, true);
        assert_eq!(OPENED.load(Ordering::SeqCst), 0, "OK");
        answer(Reason::SnippingTool, false);
        assert_eq!(OPENED.load(Ordering::SeqCst), 1, "Open Settings");
    }
}
