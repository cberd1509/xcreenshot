fn main() {
    // App commands get `allow-<command>` permissions, so each window gets only its own.
    let commands = &[
        "overlay_ready",
        "overlay_frame",
        "overlay_painted",
        "overlay_escaped",
        "overlay_escape",
        "overlay_copy",
        "overlay_save",
        "overlay_share",
        "close_overlay",
        "notice",
        "notice_answer",
        "permission_granted",
        "open_permission_settings",
        "permission_pane",
        "restart_app",
        "capture_shortcut",
        "get_prefs",
        "set_pref",
        "set_capture_option",
        "pause_capture_shortcut",
        "resume_capture_shortcut",
        "login_item_needs_approval",
        "open_login_items",
        "open_repo",
        "toast_ready",
        "toast_idle",
        "onboarding_print_screen",
        "open_keyboard_settings",
    ];
    let mut attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(commands));
    if embed_windows_manifest() {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}

/// Tauri embeds its Windows manifest (Common Controls v6) into the app binary only, so test
/// binaries that create windows fail to start (STATUS_ENTRYPOINT_NOT_FOUND). Embedding the same
/// manifest through the linker covers every binary, tests included. It also declares Per-Monitor
/// v2 DPI awareness, so every binary is DPI aware from process start.
fn embed_windows_manifest() -> bool {
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if !msvc {
        return false;
    }
    let manifest = std::env::current_dir()
        .expect("build script has a working directory")
        .join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    true
}
