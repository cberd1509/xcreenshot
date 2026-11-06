fn main() {
    // App commands get `allow-<command>` permissions, so only the overlay windows can call them.
    let commands = &[
        "overlay_ready",
        "overlay_frame",
        "overlay_painted",
        "close_overlay",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("failed to run tauri-build");
}
