//! The app log: one local file (nothing is sent anywhere), plus stdout in dev builds. Lines
//! carry milliseconds, so steps a few ms apart (a share closing the overlay, say) can be told
//! apart.

use tauri::Runtime;
use tauri::plugin::TauriPlugin;
use tauri_plugin_log::{Builder, RotationStrategy, Target, TargetKind};

/// The log plugin. Its builder starts with stdout and the log file as targets: they are
/// replaced, not added to, or every line would be written to the file twice.
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new()
        .targets(targets(cfg!(debug_assertions)).into_iter().map(Target::new))
        .level(log::LevelFilter::Info)
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {message}",
                timestamp(chrono::Utc::now()),
                record.target(),
                record.level(),
            ));
        })
        // About 1 MB per file: the current one plus the last 3, so a test session fits.
        .max_file_size(1_000_000)
        .rotation_strategy(RotationStrategy::KeepSome(3))
        .build()
}

/// Where lines go: the log file, and stdout in dev builds (stderr and stdout are invisible in
/// Windows release builds).
fn targets(debug: bool) -> Vec<TargetKind> {
    let mut targets = vec![TargetKind::LogDir { file_name: None }];
    if debug {
        targets.push(TargetKind::Stdout);
    }
    targets
}

/// `[2026-11-10][13:22:05.123]`, in UTC like the plugin's own format.
fn timestamp(now: chrono::DateTime<chrono::Utc>) -> impl std::fmt::Display {
    now.format("[%Y-%m-%d][%H:%M:%S%.3f]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tauri::plugin::Plugin;
    use tauri::test::MockRuntime;

    #[test]
    fn one_log_file_and_stdout_only_in_dev_builds() {
        let release = targets(false);
        assert_eq!(release.len(), 1);
        assert!(matches!(release[0], TargetKind::LogDir { file_name: None }));
        let debug = targets(true);
        assert_eq!(debug.len(), 2);
        let files = debug
            .iter()
            .filter(|t| matches!(t, TargetKind::LogDir { .. }))
            .count();
        assert_eq!(files, 1, "each line written once");
        assert!(matches!(debug[1], TargetKind::Stdout));
    }

    #[test]
    fn timestamps_have_milliseconds() {
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 11, 10, 13, 22, 5)
            .unwrap()
            + chrono::Duration::milliseconds(7);
        assert_eq!(timestamp(now).to_string(), "[2026-11-10][13:22:05.007]");
    }

    #[test]
    fn builds_the_plugin() {
        assert_eq!(plugin::<MockRuntime>().name(), "log");
    }
}
