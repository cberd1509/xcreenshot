//! Measures capture latency on this machine: `cargo run --release --example capture`.
//! Needs Screen Recording permission for the terminal on macOS.

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use std::time::Instant;
use xcreenshot_lib::capture::{self, CaptureError, PixelRect, Rect};

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
        // The overlay gets a copy of each frame; Rust keeps the original for the crop.
        for f in &frames {
            let t = Instant::now();
            let copy = f.image.as_raw().to_vec();
            println!("  copy {} MB in {:?}", copy.len() >> 20, t.elapsed());
        }
        if run == 1 {
            for f in &frames {
                println!(
                    "  display {}: logical {:?}, physical {:?}, scale {}",
                    f.id, f.logical, f.physical, f.scale_factor
                );
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
        }
    }
    Ok(())
}
