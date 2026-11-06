//! App wiring only: menu bar, global shortcut and windows. Logic lives in the modules below.

mod actions;
/// Public for `examples/capture.rs`.
pub mod capture;
mod overlay;
mod shortcut;

use actions::Action;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const SETTINGS_WINDOW: &str = "settings";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
        .manage(overlay::Overlay::default())
        .invoke_handler(tauri::generate_handler![
            overlay::window::overlay_frame,
            overlay::window::overlay_painted,
            overlay::window::close_overlay,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && overlay::close_requested(window)
            {
                api.prevent_close();
            }
        })
        .setup(|app| {
            // Menu bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            build_tray(app.handle(), shortcut::DEFAULT)?;
            if let Err(e) = app
                .global_shortcut()
                .on_shortcut(shortcut::DEFAULT, |app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        handle(app, Action::Capture);
                    }
                })
            {
                // Another app may own the shortcut; the menu bar item still works.
                log::warn!(
                    "could not register capture shortcut {}: {e}",
                    shortcut::DEFAULT
                );
            }
            overlay::prewarm(app.handle());
            #[cfg(debug_assertions)]
            debug_capture(app.handle());
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
    let item = |a: Action| {
        MenuItem::with_id(
            app,
            a.id(),
            a.label(),
            true,
            a.accelerator(capture_accelerator),
        )
    };
    let menu = Menu::with_items(
        app,
        &[
            &item(Action::Capture)?,
            &PredefinedMenuItem::separator(app)?,
            &item(Action::OpenSettings)?,
            &PredefinedMenuItem::separator(app)?,
            &item(Action::Quit)?,
        ],
    )?;
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
        Action::Capture => start_capture(app),
        Action::OpenSettings => {
            if let Err(e) = open_settings(app) {
                log::error!("could not open settings: {e}");
            }
        }
        Action::Quit => app.exit(0),
    }
}

/// Shows the overlay. Ignored while one is open: the menu item shows the same accelerator as
/// the global shortcut, so both can fire at once.
fn start_capture(app: &AppHandle) {
    overlay::start_capture(app);
}

/// Dev builds only: `XCREENSHOT_DEBUG_CAPTURE=<n>` captures n times after startup (each one
/// closed after 2 s), to measure hotkey-to-overlay time without pressing the hotkey.
#[cfg(debug_assertions)]
fn debug_capture(app: &AppHandle) {
    let Some(n) = std::env::var("XCREENSHOT_DEBUG_CAPTURE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let pause = std::time::Duration::from_secs(2);
        for _ in 0..n {
            std::thread::sleep(pause);
            let a = app.clone();
            let _ = app.run_on_main_thread(move || start_capture(&a));
            std::thread::sleep(pause);
            let a = app.clone();
            let _ = app.run_on_main_thread(move || overlay::close(&a));
        }
    });
}

/// The Settings webview is created on demand and destroyed on close,
/// so the idle menu bar app holds no webview.
fn open_settings(app: &AppHandle) -> tauri::Result<()> {
    let window = match app.get_webview_window(SETTINGS_WINDOW) {
        Some(w) => w,
        None => WebviewWindowBuilder::new(app, SETTINGS_WINDOW, WebviewUrl::default())
            .title("xcreenshot Settings")
            .inner_size(880.0, 640.0)
            .min_inner_size(640.0, 480.0)
            .center()
            .build()?,
    };
    window.unminimize()?;
    window.show()?;
    window.set_focus()
}
