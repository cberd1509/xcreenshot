//! The About section's link to the project page.

const REPO_URL: &str = "https://github.com/cberd1509/xcreenshot";

/// Opens the project page in the default browser. The URL is fixed, so the page can't ask for
/// any other.
#[tauri::command]
pub fn open_repo() -> Result<(), String> {
    open::that_detached(REPO_URL).map_err(|e| e.to_string())
}
