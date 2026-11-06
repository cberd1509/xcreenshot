//! App wiring only: menu bar, global shortcut and windows. Logic lives in the modules below.

mod actions;
/// Public for `examples/capture.rs`.
pub mod capture;
mod output;
mod overlay;
mod permission;
mod prefs;
/// Public so the overlay (and later commands) can call it; see `docs/share.md`.
pub mod share;
mod shortcut;
mod updates;
mod windows;

use actions::Action;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const SETTINGS_WINDOW: &str = "settings";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // Must be first: a second launch focuses the running app instead.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            handle(app, Action::OpenSettings)
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
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Used from Rust only: the pages get no updater, dialog or store permissions.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build());
    // macOS writes the clipboard itself (`output::write_png`).
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_clipboard_manager::init());
    builder
        .manage(overlay::Overlay::default())
        .manage(shortcut::Registered::default())
        .manage(overlay::global_escape::<tauri::Wry>())
        .invoke_handler(tauri::generate_handler![
            overlay::window::overlay_ready,
            overlay::window::overlay_frame,
            overlay::window::overlay_painted,
            overlay::window::overlay_escaped,
            overlay::window::overlay_escape,
            overlay::export::overlay_copy,
            overlay::export::overlay_save,
            overlay::window::close_overlay,
            permission::permission_granted,
            #[cfg(target_os = "macos")]
            permission::open_permission_settings,
            permission::restart_app,
            shortcut::capture_shortcut,
        ])
        .setup(|app| {
            // Menu bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            build_tray(app.handle(), shortcut::DEFAULT)?;
            match app
                .global_shortcut()
                .on_shortcut(shortcut::DEFAULT, |app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        handle(app, Action::Capture);
                    }
                }) {
                Ok(()) => app
                    .state::<shortcut::Registered>()
                    .set(Some(shortcut::DEFAULT)),
                // Another app may own the shortcut; the menu bar item still works.
                Err(e) => log::warn!(
                    "could not register capture shortcut {}: {e}",
                    shortcut::DEFAULT
                ),
            }
            overlay::prewarm(app.handle());
            permission::on_startup(app.handle());
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
            if let tauri::RunEvent::Reopen { .. } = event {
                handle(app, Action::OpenSettings);
            }
            let _ = (app, event);
        });
}

fn build_tray(app: &AppHandle, capture_accelerator: &str) -> tauri::Result<()> {
    let menu = tray_menu(app, capture_accelerator, None)?;
    TrayIconBuilder::with_id("main")
        .icon(tray_icon())
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("xcreenshot")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            if let Some(action) = Action::from_id(event.id().as_ref()) {
                handle(app, action);
            }
        })
        .build(app)?;
    Ok(())
}

/// The menu bar items. "Restart to update" is only there once an update is downloaded.
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
    for action in Action::MENU {
        if action == Action::RestartToUpdate && update.is_none() {
            continue;
        }
        menu.append(&item(action)?)?;
        if action.separator_after() {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
    }
    Ok(menu)
}

/// Adds "Restart to update (vX)" once an update is downloaded.
fn offer_update(app: &AppHandle, version: &str) {
    let menu = tray_menu(app, shortcut::DEFAULT, Some(version));
    if let Err(e) = menu.and_then(|m| match app.tray_by_id("main") {
        Some(tray) => tray.set_menu(Some(m)),
        None => Ok(()),
    }) {
        log::error!("could not add the restart item: {e}");
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
            if let Err(e) = open_settings(app) {
                log::error!("could not open settings: {e}");
            }
        }
        Action::CheckForUpdates => updates::check_now(app),
        Action::RestartToUpdate => updates::restart_now(app),
        Action::Quit => app.exit(0),
    }
}

/// The Settings webview is created on demand and destroyed on close. The only webviews kept
/// while idle are the hidden overlay windows, prewarmed so the hotkey is fast.
fn open_settings(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    windows::show_window(app, SETTINGS_WINDOW, WebviewUrl::default(), |b| {
        b.title("xcreenshot Settings")
            .inner_size(880.0, 640.0)
            .min_inner_size(640.0, 480.0)
            .center()
    })
}
