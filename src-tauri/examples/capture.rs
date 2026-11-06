//! Measures capture latency on this machine: `cargo run --release --example capture`.
//! Needs Screen Recording permission for the terminal on macOS.

use std::time::Instant;
use xcreenshot_lib::capture::{self, CaptureError, Rect};

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
                    "  display {}: logical {:?}, physical {:?}, scale {}",
                    f.id, f.logical, f.physical, f.scale_factor
                );
                assert_eq!(f.image.dimensions(), (f.physical.width, f.physical.height));
                let half = Rect {
                    x: 0.0,
                    y: 0.0,
                    width: f64::from(f.logical.width) / 2.0,
                    height: f64::from(f.logical.height) / 2.0,
                };
                let t = Instant::now();
                let crop = capture::crop(f, half)?;
                let cropped = t.elapsed();
                let png = capture::encode_png(&crop)?;
                println!(
                    "  crop {}x{} px in {cropped:?}, PNG {} KB in {:?}",
                    crop.width(),
                    crop.height(),
                    png.len() / 1024,
                    t.elapsed() - cropped
                );
            }
        }
    }
    Ok(())
}
