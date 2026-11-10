//! Measures the window list a double-click picks from, and whether listing it during the
//! capture slows the capture: `cargo run --release --example windows`. Needs Screen Recording
//! permission for the terminal on macOS.

use std::time::{Duration, Instant};
use xcreenshot_lib::capture::{self, CaptureError};
use xcreenshot_lib::pick;

const RUNS: u32 = 10;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The median and the slowest of `runs`, in ms.
fn summary(mut runs: Vec<f64>) -> String {
    runs.sort_by(f64::total_cmp);
    format!(
        "median {:.1} ms, max {:.1} ms",
        runs[runs.len() / 2],
        runs[runs.len() - 1]
    )
}

fn main() -> Result<(), CaptureError> {
    if !capture::has_permission() {
        eprintln!("No Screen Recording permission; asking (restart the terminal after granting).");
        capture::request_permission();
        return Err(CaptureError::PermissionDenied);
    }
    let ids: Vec<u32> = capture::displays()?.iter().map(|d| d.id).collect();
    capture::warm_up()?;

    let mut list = Vec::new();
    let mut count = 0;
    for _ in 0..RUNS {
        let t = Instant::now();
        count = pick::list().len();
        list.push(ms(t.elapsed()));
    }
    println!("list {count} windows: {}", summary(list));

    let mut alone = Vec::new();
    let mut parallel = Vec::new();
    let mut listed = Vec::new();
    for _ in 0..RUNS {
        let t = Instant::now();
        capture::capture_displays(&ids)?;
        alone.push(ms(t.elapsed()));

        // As the hotkey does: the list on its own thread while the capture runs.
        let t = Instant::now();
        let lister = std::thread::spawn(move || {
            pick::list();
            t.elapsed()
        });
        capture::capture_displays(&ids)?;
        parallel.push(ms(t.elapsed()));
        listed.push(ms(lister.join().expect("the list panicked")));
    }
    println!("capture alone: {}", summary(alone));
    println!("capture with the list: {}", summary(parallel));
    println!("  list done after: {}", summary(listed));
    Ok(())
}
