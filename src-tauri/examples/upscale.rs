//! Times Copy and Save at 2×, 3× and 4× on a 1000x700 selection made from a real screenshot,
//! `cargo run --release --example upscale`. No screen needed.
//!
//! Each step is what `overlay/export.rs` does: crop, then `compose::apply` (blur at 1×, upscale
//! with Lanczos3, draw the layer the page rendered at that size), then encode the PNG.

use image::{Rgba, RgbaImage, imageops};
use std::time::{Duration, Instant};
use xcreenshot_lib::capture::{self, CaptureError, Density, PixelRect};
use xcreenshot_lib::output::{compose, upscale};

const RUNS: usize = 7;
const W: u32 = 1000;
const H: u32 = 700;

fn median(mut all: Vec<Duration>) -> f64 {
    all.sort();
    all[all.len() / 2].as_secs_f64() * 1000.0
}

fn main() -> Result<(), CaptureError> {
    let shot = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/media/overlay.png");
    let shot = image::open(shot)
        .map_err(|e| CaptureError::Encode(e.to_string()))?
        .to_rgba8();
    let frame = imageops::crop_imm(&shot, 0, 0, W.min(shot.width()), H.min(shot.height()));
    let frame = imageops::resize(&frame.to_image(), W, H, imageops::FilterType::Nearest);
    let rect = PixelRect {
        x: 0,
        y: 0,
        width: W,
        height: H,
    };
    let blur = PixelRect {
        x: 100,
        y: 100,
        width: 300,
        height: 100,
    };
    let density = Density::new(2.0, 2.0);
    println!("{W}x{H} px selection, median of {RUNS} runs:");
    for factor in upscale::FACTORS {
        let (ow, oh) = upscale::scaled(W, H, factor).expect("fits");
        // A full-size layer with one stroke, rendered at the output size as the page does.
        let mut layer = RgbaImage::new(ow, oh);
        for x in 50 * factor..900 * factor {
            for y in 400 * factor..406 * factor {
                layer.put_pixel(x, y, Rgba([239, 68, 68, 255]));
            }
        }
        let mut steps: [Vec<Duration>; 3] = Default::default();
        let mut size = 0;
        for _ in 0..RUNS {
            let t = Instant::now();
            let image = capture::crop_pixels(&frame, rect)?;
            steps[0].push(t.elapsed());
            let t = Instant::now();
            let marks = compose::Marks {
                layer: compose::Layer::new(0, 0, ow, oh, layer.as_raw().as_slice()),
                blurs: vec![blur],
            };
            let image = compose::apply(image, &marks, factor)?;
            steps[1].push(t.elapsed());
            let t = Instant::now();
            let png = capture::encode_png(&image, density)?;
            steps[2].push(t.elapsed());
            size = png.len();
        }
        let [crop, annotate, encode] = steps.map(median);
        println!(
            "  {factor}x -> {ow}x{oh}: crop {crop:.1} ms, blur + upscale + layer {annotate:.1} ms, \
             PNG {encode:.1} ms ({:.1} MB)",
            size as f64 / 1e6
        );
    }
    Ok(())
}
