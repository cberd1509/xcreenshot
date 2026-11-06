//! App wiring only: menu bar, global shortcut and windows. Logic lives in the modules below.

mod actions;
mod shortcut;

use actions::Action;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const SETTINGS_WINDOW: &str = "settings";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        handle(app, Action::Capture);
                    }
                })
                .build(),
        )
        .setup(|app| {
            // Menu bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let accelerator = shortcut::default_accelerator();
            build_tray(app.handle(), accelerator)?;
            if let Err(e) = shortcut::parse(accelerator)
                .and_then(|s| app.global_shortcut().register(s).map_err(|e| e.to_string()))
            {
                // Another app may own the shortcut; the menu bar item still works.
                eprintln!("could not register capture shortcut: {e}");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing Settings hides it; the app keeps running in the menu bar.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running xcreenshot");
}

fn build_tray(app: &AppHandle, capture_accelerator: &str) -> tauri::Result<()> {
    let item = |a: Action| {
        let accel = a.menu_accelerator().or(match a {
            Action::Capture => Some(capture_accelerator),
            _ => None,
        });
        MenuItem::with_id(app, a.id(), a.label(), true, accel)
    };
    let [capture, settings, quit] = Action::MENU.map(item);
    let menu = Menu::with_items(
        app,
        &[
            &capture?,
            &PredefinedMenuItem::separator(app)?,
            &settings?,
            &PredefinedMenuItem::separator(app)?,
            &quit?,
        ],
    )?;
    TrayIconBuilder::with_id("main")
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/tray-template@2x.png"
        ))?)
        .icon_as_template(true)
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

fn handle(app: &AppHandle, action: Action) {
    match action {
        // The capture overlay comes in #8; for now, tell the frontend.
        Action::Capture => {
            let _ = app.emit("capture-requested", ());
        }
        Action::OpenSettings => {
            if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        Action::Quit => app.exit(0),
    }
}
