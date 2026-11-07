//! Times each step of a save on a 3840x2160 selection (a 4K display at 150%) made from a real
//! screenshot, `cargo run --release --example save`. No screen needed.

use image::{ImageBuffer, Rgba, RgbaImage, imageops};
use std::time::{Duration, Instant};
use xcreenshot_lib::capture::{self, CaptureError, Density, PixelRect};
use xcreenshot_lib::output::{self, compose};

const RUNS: usize = 5;
const W: u32 = 3840;
const H: u32 = 2160;

fn median(mut all: Vec<Duration>) -> f64 {
    all.sort();
    all[all.len() / 2].as_secs_f64() * 1000.0
}

fn main() -> Result<(), CaptureError> {
    let shot = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/media/overlay.png");
    let shot = image::open(shot)
        .map_err(|e| CaptureError::Encode(e.to_string()))?
        .to_rgba8();
    // Nearest keeps the text sharp, like a real capture.
    let frame = imageops::resize(&shot, W, H, imageops::FilterType::Nearest);
    let rect = PixelRect {
        x: 0,
        y: 0,
        width: W,
        height: H,
    };
    // A full-size annotation layer with one stroke, and one blur.
    let mut layer = RgbaImage::new(W, H);
    for x in 200..3000 {
        for y in 1000..1012 {
            layer.put_pixel(x, y, Rgba([239, 68, 68, 255]));
        }
    }
    let density = Density::new(1.5, 1.5);
    let dir = tempfile::tempdir().map_err(|e| CaptureError::Write(e.to_string()))?;
    let path = dir.path().join("shot.png");
    let mut steps: [Vec<Duration>; 3] = Default::default();
    let mut size = 0;
    for _ in 0..RUNS {
        let t = Instant::now();
        let mut image = capture::crop_pixels(&frame, rect)?;
        let marks = compose::Marks {
            layer: Some(compose::Layer {
                x: 0,
                y: 0,
                pixels: ImageBuffer::from_raw(W, H, layer.as_raw().as_slice()).expect("size"),
            }),
            blurs: vec![PixelRect {
                x: 100,
                y: 100,
                width: 600,
                height: 200,
            }],
        };
        compose::apply(&mut image, &marks);
        steps[0].push(t.elapsed());
        let t = Instant::now();
        let png = capture::encode_png(&image, density)?;
        steps[1].push(t.elapsed());
        size = png.len();
        let t = Instant::now();
        output::write_file(&path, &png).map_err(|e| CaptureError::Write(e.to_string()))?;
        steps[2].push(t.elapsed());
    }
    let [crop, encode, write] = steps.map(median);
    println!(
        "{W}x{H} px, PNG {:.1} MB, median of {RUNS} runs:",
        size as f64 / 1e6
    );
    println!("  crop + annotations  {crop:>6.1} ms  (before the dialog)");
    println!("  encode_png          {encode:>6.1} ms");
    println!("  write_file          {write:>6.1} ms");
    println!(
        "  dialog to close: before {:.1} ms (encode + write), now {write:.1} ms (write; the PNG is made during the dialog)",
        encode + write
    );

    Ok(())
}
