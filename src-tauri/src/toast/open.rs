//! The Saved toast's buttons: Open (the file, in its default app) and Show in Finder (Show in
//! folder on Windows: the folder, with the file selected). The app that opens comes to the front;
//! xcreenshot does not.

use std::path::Path;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

/// What a Saved toast's button does with the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    /// In its default app.
    Open,
    /// Selected in Finder or Explorer.
    Reveal,
}

/// Does `action` with the file at `path`, which the app saved (never a path from a page).
pub fn run(action: Action, path: &Path) -> Result<(), String> {
    launch(action, path).map_err(|e| format!("could not {action:?} {}: {e}", path.display()))
}

/// The command that shows `path` selected in its folder.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn reveal(path: &Path) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("/usr/bin/open");
        command.arg("-R").arg(path);
        command
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // Explorer wants the path quoted inside the one argument, which `arg` would quote whole.
        // A Windows path can't hold a `"`, so it can't end the quote early.
        let mut command = Command::new("explorer.exe");
        command.raw_arg(format!("/select,\"{}\"", path.display()));
        command
    }
}

#[cfg(not(test))]
fn launch(action: Action, path: &Path) -> std::io::Result<()> {
    match action {
        Action::Open => ::open::that_detached(path),
        // No standard way to select a file: the folder it is in.
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        Action::Reveal => ::open::that_detached(path.parent().unwrap_or(path)),
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        Action::Reveal => {
            let mut child = reveal(path).spawn()?;
            // Reaped off the main thread. Explorer exits with 1 even when it worked.
            std::thread::spawn(move || child.wait());
            Ok(())
        }
    }
}

/// Tests open nothing: they see what would have been.
#[cfg(test)]
fn launch(action: Action, path: &Path) -> std::io::Result<()> {
    if path.as_os_str().is_empty() {
        return Err(std::io::Error::other("no file"));
    }
    tests::LAUNCHED.with(|l| l.borrow_mut().push((action, path.to_path_buf())));
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        pub(crate) static LAUNCHED: RefCell<Vec<(Action, PathBuf)>> = const { RefCell::new(Vec::new()) };
    }

    /// What tests on this thread opened, emptied.
    pub(crate) fn launched() -> Vec<(Action, PathBuf)> {
        LAUNCHED.with(|l| l.take())
    }

    #[test]
    fn the_page_names_the_action() {
        let action = |s| serde_json::from_value::<Action>(serde_json::json!(s)).ok();
        assert_eq!(action("open"), Some(Action::Open));
        assert_eq!(action("reveal"), Some(Action::Reveal));
        assert_eq!(action("delete"), None);
    }

    #[test]
    fn runs_the_action_and_says_when_it_could_not() {
        let path = PathBuf::from("/shots/a b.png");
        assert_eq!(run(Action::Reveal, &path), Ok(()));
        assert_eq!(launched(), [(Action::Reveal, path)]);
        let failed = run(Action::Open, Path::new("")).unwrap_err();
        assert!(failed.starts_with("could not Open"), "{failed}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finder_shows_the_file_selected() {
        let command = reveal(Path::new("/Users/me/Desktop/shot 1.png"));
        assert_eq!(command.get_program(), "/usr/bin/open");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["-R", "/Users/me/Desktop/shot 1.png"]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn explorer_shows_the_file_selected() {
        let command = reveal(Path::new(r"C:\Users\me\Pictures\shot 1.png"));
        assert_eq!(command.get_program(), "explorer.exe");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, [r#"/select,"C:\Users\me\Pictures\shot 1.png""#]);
    }
}
