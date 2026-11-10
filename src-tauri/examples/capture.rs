//! Measures capture latency on this machine: `cargo run --release --example capture`.
//! Needs Screen Recording permission for the terminal on macOS.
//!
//! Times each backend (xcap, ScreenCaptureKit) on the first display and on all displays, each in
//! a fresh process so the first call is really cold, then compares what the two return and
//! times the crop and PNG steps. `-- xcap all` (or `sck one`, ...) runs only that row.
//!
//! `-- first <seconds>` times the first capture after the startup warm-up, as the app does it:
//! warm up, wait, then capture every display and copy each frame for its page. `-- memory <n>`
//! prints this process's footprint over `n` such captures and while idle after them.

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use std::process::Command;
use std::time::{Duration, Instant};
use xcreenshot_lib::capture::{self, Backend, CaptureError, DisplayFrame, PixelRect, Rect};
use xcreenshot_lib::memory;

const WARM_RUNS: usize = 10;

fn main() -> Result<(), CaptureError> {
    if !capture::has_permission() {
        eprintln!("No Screen Recording permission; asking (restart the terminal after granting).");
        capture::request_permission();
        return Err(CaptureError::PermissionDenied);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, seconds] if mode == "first" => return first(seconds.parse().unwrap_or(2)),
        [mode, runs] if mode == "memory" => return memory(runs.parse().unwrap_or(30)),
        [backend, scope] => return bench_row(backend, scope),
        _ => {}
    }
    let ids = ids()?;
    println!("{} display(s)", ids.len());
    println!("| Backend | Displays | First call | Warm (median, min–max of {WARM_RUNS}) |");
    println!("| --- | --- | --- | --- |");
    let me = std::env::current_exe().expect("own path");
    for backend in ["xcap", "sck"] {
        for scope in ["one", "all"] {
            // A fresh process per row, so every first call is cold.
            let status = Command::new(&me).args([backend, scope]).status();
            if !status.is_ok_and(|s| s.success()) {
                println!("| {backend} | {scope} | failed | |");
            }
        }
    }
    compare(&ids)?;
    let t = Instant::now();
    let frames = capture::capture_displays(&ids)?;
    println!(
        "\nauto (xcap, ScreenCaptureKit if it fails): {:?}",
        t.elapsed()
    );
    crop_and_png(&frames)
}

fn ids() -> Result<Vec<u32>, CaptureError> {
    Ok(capture::displays()?.iter().map(|d| d.id).collect())
}

/// One table row: the first capture in this process, then [`WARM_RUNS`] more.
fn bench_row(name: &str, scope: &str) -> Result<(), CaptureError> {
    let backend = match name {
        "xcap" => Backend::Xcap,
        "sck" => Backend::ScreenCaptureKit,
        _ => return Err(CaptureError::Invalid(format!("unknown backend {name:?}"))),
    };
    let mut ids = ids()?;
    if scope == "one" {
        ids.truncate(1);
    }
    let (cold, warm) = cold_and_warm(|| {
        let t = Instant::now();
        capture::capture_displays_with(backend, &ids)?;
        Ok(t.elapsed())
    })?;
    println!(
        "| {backend} | {} ({scope}) | {:.0} ms | {warm} |",
        ids.len(),
        ms(cold),
    );
    Ok(())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// One timed call, then [`WARM_RUNS`] more as "median ms (min–max)".
fn cold_and_warm(
    mut time: impl FnMut() -> Result<Duration, CaptureError>,
) -> Result<(Duration, String), CaptureError> {
    let cold = time()?;
    let mut warm = (0..WARM_RUNS)
        .map(|_| time())
        .collect::<Result<Vec<_>, _>>()?;
    warm.sort_unstable();
    let spread = format!(
        "{:.0} ms ({:.0}–{:.0})",
        ms(warm[warm.len() / 2]),
        ms(warm[0]),
        ms(warm[warm.len() - 1]),
    );
    Ok((cold, spread))
}

/// What the app does for one capture: every display, and a copy of each frame for its page
/// (`overlay_frame`). Both are freed on return.
fn capture_and_copy(ids: &[u32]) -> Result<Duration, CaptureError> {
    let t = Instant::now();
    let frames = capture::capture_displays(ids)?;
    let _copies: Vec<Vec<u8>> = frames.iter().map(|f| f.image.as_raw().to_vec()).collect();
    Ok(t.elapsed())
}

/// The first capture `seconds` after the warm-up (in a fresh process), then [`WARM_RUNS`] more.
fn first(seconds: u64) -> Result<(), CaptureError> {
    let ids = ids()?;
    let t = Instant::now();
    capture::warm_up()?;
    let warm_up = t.elapsed();
    std::thread::sleep(Duration::from_secs(seconds));
    let (cold, warm) = cold_and_warm(|| capture_and_copy(&ids))?;
    println!(
        "warm-up {:.0} ms, first {seconds} s later {:.0} ms, warm {warm}",
        ms(warm_up),
        ms(cold),
    );
    Ok(())
}

/// This process's memory over `runs` captures (each as in [`first`]) and while idle after, as
/// the app logs it.
fn memory(runs: usize) -> Result<(), CaptureError> {
    let usage = || memory::now().map_or_else(|| "unknown".to_owned(), |u| u.to_string());
    let ids = ids()?;
    capture::warm_up()?;
    println!("after the warm-up: {}", usage());
    for run in 1..=runs {
        capture_and_copy(&ids)?;
        if run == 1 || run % 10 == 0 {
            println!("after {run} capture(s): {}", usage());
        }
    }
    for idle in [1, 10, 60] {
        std::thread::sleep(Duration::from_secs(idle));
        println!("then idle {idle} s: {}", usage());
    }
    Ok(())
}

/// Both backends on the same screen: same geometry, and how far the pixels are apart (the
/// screen can change between the two captures, a clock for example).
fn compare(ids: &[u32]) -> Result<(), CaptureError> {
    let xcap = capture::capture_displays_with(Backend::Xcap, ids)?;
    let sck = capture::capture_displays_with(Backend::ScreenCaptureKit, ids)?;
    for a in &xcap {
        let Some(b) = sck.iter().find(|b| b.id == a.id) else {
            println!("\ndisplay {}: not captured by ScreenCaptureKit", a.id);
            continue;
        };
        println!(
            "\ndisplay {}: logical {:?}, physical {:?}, scale {}",
            a.id, a.logical, a.physical, a.scale_factor
        );
        let same =
            |what: &str, ok: bool| println!("  {what}: {}", if ok { "same" } else { "DIFFERENT" });
        same("size", a.image.dimensions() == b.image.dimensions());
        same("bounds", (a.logical, a.physical) == (b.logical, b.physical));
        if a.image.dimensions() != b.image.dimensions() {
            continue;
        }
        let (mut total, mut off) = (0u64, 0u64);
        for (p, q) in a.image.pixels().zip(b.image.pixels()) {
            let d = [0, 1, 2].map(|c| u64::from(p.0[c].abs_diff(q.0[c])));
            total += d.iter().sum::<u64>();
            off += u64::from(d.iter().any(|&v| v > 2));
        }
        let n = u64::from(a.image.width()) * u64::from(a.image.height());
        println!(
            "  pixels: mean difference {:.3} per channel, {:.2}% differ by more than 2",
            total as f64 / (n * 3) as f64,
            off as f64 * 100.0 / n as f64
        );
    }
    Ok(())
}

fn crop_and_png(frames: &[DisplayFrame]) -> Result<(), CaptureError> {
    for f in frames {
        // The overlay gets a copy of each frame; Rust keeps the original for the crop.
        let t = Instant::now();
        let copy = f.image.as_raw().to_vec();
        println!("  copy {} MB in {:?}", copy.len() >> 20, t.elapsed());
        assert_eq!(f.image.dimensions(), (f.physical.width, f.physical.height));
        let density =
            capture::Density::of(f.image.dimensions(), (f.logical.width, f.logical.height));
        let half = Rect {
            x: 0.0,
            y: 0.0,
            width: f64::from(f.logical.width) / 2.0,
            height: f64::from(f.logical.height) / 2.0,
        };
        let t = Instant::now();
        let crop = capture::crop(f, half)?;
        let cropped = t.elapsed();
        let png = capture::encode_png(&crop, density)?;
        println!(
            "  crop {}x{} px in {cropped:?}, PNG {} KB in {:?}",
            crop.width(),
            crop.height(),
            png.len() / 1024,
            t.elapsed() - cropped
        );
        // Copy and save encode at most the whole display. Compare the PNG levels.
        let full = PixelRect {
            x: 0,
            y: 0,
            width: f.physical.width,
            height: f.physical.height,
        };
        let t = Instant::now();
        let crop = capture::crop_pixels(&f.image, full)?;
        println!(
            "  full {}x{} px: crop {:?}",
            crop.width(),
            crop.height(),
            t.elapsed()
        );
        for (name, level) in [
            ("Fast", CompressionType::Fast),
            ("Level(1)", CompressionType::Level(1)),
            ("Level(2)", CompressionType::Level(2)),
            ("Level(3)", CompressionType::Level(3)),
            ("Level(4)", CompressionType::Level(4)),
            ("Default", CompressionType::Default),
        ] {
            let t = Instant::now();
            let mut png = Vec::new();
            PngEncoder::new_with_quality(&mut png, level, FilterType::Adaptive)
                .write_image(
                    crop.as_raw(),
                    crop.width(),
                    crop.height(),
                    ExtendedColorType::Rgba8,
                )
                .map_err(|e| CaptureError::Encode(e.to_string()))?;
            println!(
                "    PNG {name}: {} KB in {:?}",
                png.len() / 1024,
                t.elapsed()
            );
        }
        let t = Instant::now();
        let png = capture::encode_png(&crop, density)?;
        println!(
            "    encode_png (copy and save): {} KB in {:?}",
            png.len() / 1024,
            t.elapsed()
        );
    }
    Ok(())
}
