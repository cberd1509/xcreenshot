//! App wiring only: menu bar, global shortcut and windows. Logic lives in the modules below.

mod about;
mod actions;
/// Public for `examples/capture.rs`.
pub mod capture;
mod focus;
mod login;
#[cfg(all(target_os = "macos", not(test)))]
mod macos;
mod notice;
mod onboarding;
/// Public for `examples/save.rs`.
pub mod output;
mod overlay;
mod permission;
/// Public for `examples/windows.rs`.
pub mod pick;
mod prefs;
mod print_screen;
mod region;
mod settings;
/// Public so the overlay (and later commands) can call it; see `docs/share.md`.
pub mod share;
mod shortcut;
mod toast;
mod updates;
mod windows;

use actions::Action;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Listener, Manager, WebviewUrl, WebviewWindow};
use tauri_plugin_global_shortcut::ShortcutState;
use windows::Source;

const SETTINGS_WINDOW: &str = "settings";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // Must be first: a second launch focuses the running app instead.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let _ = windows::open_from(app, Source::SecondLaunch, open_settings);
        }))
        // Local log file only (nothing is sent anywhere); stderr is invisible in Windows release builds.
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir { file_name: None },
                ))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, pressed, event| {
                    if event.state() == ShortcutState::Pressed
                        && app.state::<shortcut::Registered>().is_capture(pressed)
                    {
                        handle(app, Action::Capture);
                    }
                })
                .build(),
        )
        // Used from Rust only: the pages get no updater, dialog, store or autostart permissions.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build());
    // macOS and Windows write the clipboard themselves (`output::os_clipboard`), and macOS
    // uses SMAppService for the login item (`login`).
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let builder = builder.plugin(tauri_plugin_clipboard_manager::init());
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_autostart::Builder::new().build());
    #[cfg(target_os = "macos")]
    let builder = builder.manage(login::System::default());
    // Hands focus back to the app the user was in when a capture or a notice closes.
    #[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
    let builder = builder.manage(focus::Focus::os());
    builder
        .manage(overlay::Overlay::default())
        .manage(pick::Windows::default())
        .manage(notice::Notices::default())
        .manage(toast::Toasts::default())
        .manage(shortcut::Registered::default())
        .manage(actions::MenuShows::default())
        .manage(overlay::global_escape::<tauri::Wry>())
        .invoke_handler(tauri::generate_handler![
            overlay::window::overlay_ready,
            overlay::window::overlay_frame,
            overlay::window::overlay_painted,
            overlay::window::overlay_escaped,
            overlay::window::overlay_escape,
            overlay::export::overlay_copy,
            overlay::export::overlay_save,
            overlay::export::overlay_share,
            overlay::window::close_overlay,
            pick::overlay_window_at,
            notice::notice,
            notice::notice_answer,
            permission::permission_granted,
            permission::permission_pane,
            #[cfg(target_os = "macos")]
            permission::open_permission_settings,
            permission::restart_app,
            onboarding::onboarding_print_screen,
            onboarding::onboarding_tried,
            #[cfg(target_os = "windows")]
            onboarding::open_keyboard_settings,
            shortcut::capture_shortcut,
            shortcut::pause_capture_shortcut,
            shortcut::resume_capture_shortcut,
            settings::get_prefs,
            settings::set_pref,
            settings::set_capture_option,
            login::login_item_needs_approval,
            #[cfg(target_os = "macos")]
            login::open_login_items,
            about::open_repo,
            toast::window::toast_ready,
            toast::window::toast_idle,
        ])
        // A page recording a new shortcut pauses the capture shortcut: take it back if that
        // page reloads or its window closes mid-recording.
        .on_page_load(|webview, payload| {
            shortcut::page_gone(webview.app_handle(), webview.label());
            toast::on_page_load(webview, payload);
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                shortcut::page_gone(window.app_handle(), window.label());
            }
        })
        .setup(|app| {
            // Menu bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            #[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
            focus::watch(app.handle());
            let (capture_shortcut, print_screen_moved) = settings::init(app.handle());
            build_tray(app.handle(), &capture_shortcut)?;
            // First, so the hotkey is fast from the start.
            overlay::prewarm(app.handle());
            toast::prewarm(app.handle());
            let onboarding_shown = onboarding::on_startup(app.handle());
            if let Some(reason) =
                print_screen_moved.filter(|r| onboarding::needs_notice(*r, onboarding_shown))
            {
                print_screen::tell(app.handle(), reason, |app| {
                    windows::open_from(app, Source::PrintScreenNotice, open_settings)
                });
            }
            updates::start(app.handle(), offer_update);
            share::init(app.handle());
            #[cfg(debug_assertions)]
            overlay::debug::schedule(app.handle());
            #[cfg(debug_assertions)]
            share::debug::schedule(app.handle(), open_settings);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building xcreenshot")
        .run(|app, event| {
            // Opening the app again (Finder, Spotlight) shows Settings.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: visible_windows,
                ..
            } = event
            {
                let _ = windows::open_from(app, Source::Reopen { visible_windows }, open_settings);
            }
            let _ = (app, event);
        });
}

fn build_tray(app: &AppHandle, capture_accelerator: &str) -> tauri::Result<()> {
    // The first menu counts as shown once the tray is built with it.
    app.state::<actions::MenuShows>()
        .refresh(capture_accelerator, None, || {
            build_tray_icon(app, capture_accelerator)
        })?;
    // Keep the capture item's shortcut in step with the setting.
    let tray_app = app.clone();
    app.listen(settings::CHANGED, move |_| {
        refresh_tray(&tray_app, updates::ready_version(&tray_app).as_deref())
    });
    Ok(())
}

fn build_tray_icon(app: &AppHandle, capture_accelerator: &str) -> tauri::Result<()> {
    TrayIconBuilder::with_id("main")
        .icon(tray_icon())
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("xcreenshot")
        .menu(&tray_menu(app, capture_accelerator, None)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            if let Some(action) = Action::from_id(event.id().as_ref()) {
                handle(app, action);
            }
        })
        .build(app)?;
    Ok(())
}

/// The menu bar items, laid out by `Action::menu_rows`.
fn tray_menu(
    app: &AppHandle,
    capture_accelerator: &str,
    update: Option<&str>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |a: Action| {
        let label = match (a, update) {
            (Action::RestartToUpdate, Some(version)) => format!("{} (v{version})", a.label()),
            _ => a.label().to_owned(),
        };
        MenuItem::with_id(app, a.id(), label, true, a.accelerator(capture_accelerator))
    };
    let menu = Menu::new(app)?;
    for row in Action::menu_rows(update.is_some()) {
        match row {
            Some(action) => menu.append(&item(action)?)?,
            None => menu.append(&PredefinedMenuItem::separator(app)?)?,
        }
    }
    Ok(menu)
}

/// Adds "Restart to update (vX)" once an update is downloaded.
fn offer_update(app: &AppHandle, version: &str) {
    refresh_tray(app, Some(version));
}

/// Rebuilds the menu bar items with the current capture shortcut and pending update.
fn refresh_tray(app: &AppHandle, update: Option<&str>) {
    let accelerator = app.state::<shortcut::Registered>().get().accelerator;
    let installed = app
        .state::<actions::MenuShows>()
        .refresh(&accelerator, update, || {
            let menu = tray_menu(app, &accelerator, update)?;
            match app.tray_by_id("main") {
                Some(tray) => tray.set_menu(Some(menu)),
                None => Ok(()),
            }
        });
    if let Err(e) = installed {
        log::error!("could not update the menu bar: {e}");
    }
}

/// macOS recolors the black template icon for the menu bar; other platforms get the color app icon.
fn tray_icon() -> tauri::image::Image<'static> {
    if cfg!(target_os = "macos") {
        tauri::include_image!("icons/tray-template@2x.png")
    } else {
        tauri::include_image!("icons/32x32.png")
    }
}

fn handle(app: &AppHandle, action: Action) {
    match action {
        // Ignored while the overlay is open: the menu item shows the same accelerator as the
        // global shortcut, so both can fire at once.
        Action::Capture => {
            overlay::start_capture(app);
        }
        Action::OpenSettings => {
            let _ = windows::open_from(app, Source::Menu, open_settings);
        }
        Action::CheckForUpdates => updates::check_now(app),
        Action::RestartToUpdate => updates::restart_now(app),
        Action::Quit => app.exit(0),
    }
}

/// The Settings webview is created on demand and destroyed on close. The only webviews kept
/// while idle are the hidden overlay windows, prewarmed so the hotkey is fast, and the toast
/// window, prewarmed so the first toast builds nothing between captures.
fn open_settings(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    windows::show_window(app, SETTINGS_WINDOW, WebviewUrl::default(), |b| {
        b.title("xcreenshot Settings")
            .inner_size(880.0, 640.0)
            .min_inner_size(640.0, 480.0)
            .center()
    })
}
