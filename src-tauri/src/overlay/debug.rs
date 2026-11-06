//! Dev builds only: `XCREENSHOT_DEBUG_CAPTURE=<n>` captures n times after startup, each closed
//! after 2 s, to measure hotkey-to-overlay time without pressing the hotkey.

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Runtime};

const PAUSE: Duration = Duration::from_secs(2);

pub fn schedule<R: Runtime>(app: &AppHandle<R>) {
    if let Some(runs) = runs(std::env::var("XCREENSHOT_DEBUG_CAPTURE").ok()) {
        repeat(app, runs, PAUSE, super::start_capture);
    }
}

fn runs(value: Option<String>) -> Option<u32> {
    value?.parse().ok().filter(|&n| n > 0)
}

/// Runs `start` on the main thread `runs` times, closing each capture it started (never one
/// the user opened) after `pause`.
fn repeat<R: Runtime>(
    app: &AppHandle<R>,
    runs: u32,
    pause: Duration,
    start: fn(&AppHandle<R>) -> Option<u64>,
) -> JoinHandle<()> {
    let app = app.clone();
    std::thread::spawn(move || {
        for _ in 0..runs {
            std::thread::sleep(pause);
            let (tx, rx) = mpsc::channel();
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                let _ = tx.send(start(&handle));
            });
            let Ok(Some(session)) = rx.recv() else {
                continue;
            };
            std::thread::sleep(pause);
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || super::window::close_session(&handle, session));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

    #[test]
    fn runs_come_from_a_positive_number() {
        assert_eq!(runs(Some("5".into())), Some(5));
        for v in [None, Some("0".into()), Some("x".into()), Some("-1".into())] {
            assert_eq!(runs(v.clone()), None, "{v:?}");
        }
    }

    static STARTS: AtomicU32 = AtomicU32::new(0);

    /// Starts nothing on the first call (the overlay was already open); later calls report a
    /// session id that is not the open one.
    fn start(_: &AppHandle<MockRuntime>) -> Option<u64> {
        let n = STARTS.fetch_add(1, Ordering::Relaxed);
        (n > 0).then_some(u64::MAX)
    }

    #[test]
    fn starts_each_run_and_closes_only_its_own_session() {
        let app = mock_builder()
            .manage(super::super::Overlay::default())
            .build(mock_context(noop_assets()))
            .unwrap();
        let h = app.handle();
        // Without XCREENSHOT_DEBUG_CAPTURE, nothing is scheduled.
        schedule(h);
        // The user's own capture is open.
        assert!(super::super::window::phase(h).begin(42, std::time::Instant::now()));
        repeat(h, 3, Duration::ZERO, start).join().unwrap();
        assert_eq!(STARTS.load(Ordering::Relaxed), 3);
        assert_eq!(
            super::super::window::phase(h).session(),
            Some(42),
            "left alone"
        );
    }
}
