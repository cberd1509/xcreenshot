fn main() {
    // App commands get `allow-<command>` permissions, so only the overlay windows can call them.
    let commands = &[
        "overlay_ready",
        "overlay_frame",
        "overlay_painted",
        "close_overlay",
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
/// manifest through the linker covers every binary, tests included.
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
