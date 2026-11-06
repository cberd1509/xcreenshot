//! Measures capture latency on this machine: `cargo run --release --example capture`.
//! Needs Screen Recording permission for the terminal on macOS.

use std::time::Instant;
use xcreenshot_lib::capture::{self, CaptureError, PngSpeed};

fn main() -> Result<(), CaptureError> {
    if !capture::has_permission() {
        eprintln!("No Screen Recording permission; asking (restart the terminal after granting).");
        capture::request_permission();
        return Err(CaptureError::PermissionDenied);
    }
    for run in 1..=5 {
        let start = Instant::now();
        let frames = capture::capture_all()?;
        println!(
            "run {run}: {} display(s) in {:?}",
            frames.len(),
            start.elapsed()
        );
        if run == 1 {
            for f in &frames {
                println!(
                    "  display {} at ({}, {}), {}x{} pt, scale {}, {}x{} px",
                    f.id,
                    f.x,
                    f.y,
                    f.width,
                    f.height,
                    f.scale_factor,
                    f.image.width(),
                    f.image.height()
                );
                for speed in [PngSpeed::Fast, PngSpeed::Small] {
                    let t = Instant::now();
                    let png = capture::encode_png(&f.image, speed)?;
                    println!(
                        "  PNG {speed:?}: {} KB in {:?}",
                        png.len() / 1024,
                        t.elapsed()
                    );
                }
            }
        }
    }
    Ok(())
}
