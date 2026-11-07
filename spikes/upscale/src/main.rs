//! Upscaler spike for issue #107. Not part of the app.
//!
//!   upscale-spike compare <inputs> <out>   side-by-side PNGs at 2x and 4x, PSNR vs ground truth
//!   upscale-spike time <inputs>            median timings per method, size and factor
//!   upscale-spike quirks                   MetalFX formats, modes, factors, limits, alpha

mod cpu;
mod label;
#[cfg(all(target_os = "macos", feature = "metalfx"))]
mod metalfx;

use std::path::Path;
use std::time::{Duration, Instant};

use image::{Rgba, RgbaImage};
use label::{Labeller, fill};

type Upscale<'a> = Box<dyn Fn(&RgbaImage, u32) -> RgbaImage + 'a>;

/// Test images made by scripts/make-inputs.sh, with the point to centre the crop on.
const CASES: &[(&str, (f32, f32))] = &[
    ("ui_text_300", (0.5, 0.5)),
    ("ui_text_1000", (0.2, 0.3)),
    ("icons_300", (0.5, 0.5)),
    ("icons_1000", (0.3, 0.5)),
    ("code_1x_300", (0.5, 0.5)),
    ("code_1x_1000", (0.15, 0.35)),
    ("code_2x_1000", (0.25, 0.3)),
    ("photo_300", (0.5, 0.5)),
    ("photo_1000", (0.45, 0.45)),
];

const TILE_W: u32 = 1200;
const TILE_H: u32 = 800;

fn methods<'a>(#[allow(unused)] gpu: Option<&'a GpuHandle>) -> Vec<(&'static str, Upscale<'a>)> {
    #[allow(unused_mut)]
    let mut list: Vec<(&'static str, Upscale<'a>)> = vec![
        ("Nearest", Box::new(cpu::nearest)),
        ("Lanczos3", Box::new(cpu::lanczos3)),
        ("Lanczos3 + sharpen", Box::new(cpu::lanczos3_sharpen)),
    ];
    #[cfg(all(target_os = "macos", feature = "metalfx"))]
    if let Some(gpu) = gpu {
        list.push((
            "MetalFX spatial",
            Box::new(move |img, f| {
                gpu.upscale(
                    img,
                    f,
                    objc2_metal::MTLPixelFormat::RGBA8Unorm,
                    metalfx::Mode::Perceptual,
                )
                .unwrap()
            }),
        ));
    }
    list
}

#[cfg(all(target_os = "macos", feature = "metalfx"))]
type GpuHandle = metalfx::Gpu;
#[cfg(not(all(target_os = "macos", feature = "metalfx")))]
type GpuHandle = ();

fn gpu() -> Option<GpuHandle> {
    #[cfg(all(target_os = "macos", feature = "metalfx"))]
    return metalfx::Gpu::new()
        .map_err(|e| eprintln!("MetalFX unavailable: {e}"))
        .ok();
    #[cfg(not(all(target_os = "macos", feature = "metalfx")))]
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("compare") => compare(Path::new(&args[2]), Path::new(&args[3])),
        Some("time") => time(Path::new(&args[2])),
        #[cfg(all(target_os = "macos", feature = "metalfx"))]
        Some("quirks") => quirks(),
        _ => eprintln!("usage: upscale-spike compare <inputs> <out> | time <inputs> | quirks"),
    }
}

fn load(path: &Path) -> RgbaImage {
    image::open(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .to_rgba8()
}

/// PSNR over RGB, in dB.
fn psnr(a: &RgbaImage, b: &RgbaImage) -> f64 {
    assert_eq!(a.dimensions(), b.dimensions());
    let mut sum = 0f64;
    for (p, q) in a.pixels().zip(b.pixels()) {
        for c in 0..3 {
            let d = p[c] as f64 - q[c] as f64;
            sum += d * d;
        }
    }
    let mse = sum / (a.width() as f64 * a.height() as f64 * 3.0);
    10.0 * (255.0 * 255.0 / mse).log10()
}

fn compare(inputs: &Path, out: &Path) {
    std::fs::create_dir_all(out).unwrap();
    let gpu = gpu();
    let methods = methods(gpu.as_ref());
    let labeller = Labeller::new();
    println!(
        "| image | factor | {} |",
        methods.iter().map(|m| m.0).collect::<Vec<_>>().join(" | ")
    );
    println!("|---|---|{}", "---|".repeat(methods.len()));
    for &(name, focus) in CASES {
        let src = load(&inputs.join(format!("{name}.png")));
        for factor in [2, 4] {
            let outputs: Vec<RgbaImage> = methods.iter().map(|(_, f)| f(&src, factor)).collect();
            let reference = inputs.join(format!("{name}_ref{factor}.png"));
            let scores: Vec<Option<f64>> = if reference.exists() {
                let r = load(&reference);
                outputs.iter().map(|o| Some(psnr(o, &r))).collect()
            } else {
                vec![None; outputs.len()]
            };
            if scores[0].is_some() {
                let cells: Vec<String> = scores
                    .iter()
                    .map(|s| format!("{:.2} dB", s.unwrap()))
                    .collect();
                println!("| {name} | {factor}x | {} |", cells.join(" | "));
            }
            let grid = grid(
                &labeller, name, &src, factor, focus, &methods, &outputs, &scores,
            );
            grid.save(out.join(format!("{name}_{factor}x.png")))
                .unwrap();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn grid(
    labeller: &Labeller,
    name: &str,
    src: &RgbaImage,
    factor: u32,
    focus: (f32, f32),
    methods: &[(&str, Upscale)],
    outputs: &[RgbaImage],
    scores: &[Option<f64>],
) -> RgbaImage {
    const HEADER: u32 = 56;
    const BAR: u32 = 44;
    const GAP: u32 = 8;
    let (ow, oh) = outputs[0].dimensions();
    let (tw, th) = (ow.min(TILE_W), oh.min(TILE_H));
    let x0 = ((focus.0 * ow as f32) as i64 - tw as i64 / 2).clamp(0, (ow - tw) as i64) as u32;
    let y0 = ((focus.1 * oh as f32) as i64 - th as i64 / 2).clamp(0, (oh - th) as i64) as u32;
    let cols = 2;
    let rows = outputs.len().div_ceil(cols) as u32;
    let width = cols as u32 * tw + (cols as u32 + 1) * GAP;
    let height = HEADER + rows * (BAR + th + GAP);
    let mut img = RgbaImage::from_pixel(width, height, Rgba([32, 32, 32, 255]));
    let crop = if (tw, th) == (ow, oh) {
        "full output".to_string()
    } else {
        format!("1:1 crop {tw}x{th} of {ow}x{oh}")
    };
    let (w, h) = src.dimensions();
    labeller.draw(
        &mut img,
        &format!("{name}   {w}x{h} at {factor}x   ({crop})"),
        GAP as f32,
        14.0,
        26.0,
    );
    for (i, out) in outputs.iter().enumerate() {
        let (c, r) = ((i % cols) as u32, (i / cols) as u32);
        let x = GAP + c * (tw + GAP);
        let y = HEADER + r * (BAR + th + GAP);
        fill(&mut img, x, y, tw, BAR, Rgba([0, 0, 0, 255]));
        let text = match scores[i] {
            Some(s) => format!("{}   PSNR {s:.2} dB", methods[i].0),
            None => methods[i].0.to_string(),
        };
        labeller.draw(&mut img, &text, (x + 12) as f32, (y + 9) as f32, 24.0);
        let tile = image::imageops::crop_imm(out, x0, y0, tw, th).to_image();
        image::imageops::replace(&mut img, &tile, x as i64, (y + BAR) as i64);
    }
    img
}

fn median(mut v: Vec<Duration>) -> f64 {
    v.sort();
    v[v.len() / 2].as_secs_f64() * 1000.0
}

fn time(inputs: &Path) {
    let cold = Instant::now();
    let gpu = gpu();
    let gpu_init = cold.elapsed();
    let mut list = methods(gpu.as_ref());
    list.insert(
        2,
        (
            "Lanczos3 (image crate)",
            Box::new(cpu::lanczos3_image_crate),
        ),
    );

    let small = load(&inputs.join("ui_text_300.png"));
    let medium = load(&inputs.join("ui_text_1000.png"));
    let screen = load(&inputs.join("screen.png"));

    #[cfg(all(target_os = "macos", feature = "metalfx"))]
    if let Some(g) = gpu.as_ref() {
        let t = Instant::now();
        g.upscale(
            &medium,
            4,
            objc2_metal::MTLPixelFormat::RGBA8Unorm,
            metalfx::Mode::Perceptual,
        )
        .unwrap();
        println!(
            "MetalFX on {}: device + queue {:.1} ms, first 1000x700 4x call {:.1} ms (cold)",
            g.name(),
            gpu_init.as_secs_f64() * 1000.0,
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
    let _ = gpu_init;

    let jobs: [(&RgbaImage, u32, usize); 7] = [
        (&small, 2, 21),
        (&small, 4, 21),
        (&medium, 2, 11),
        (&medium, 3, 11),
        (&medium, 4, 11),
        (&screen, 2, 5),
        (&small, 3, 21),
    ];
    println!(
        "\n| input | factor | output | {} |",
        list.iter().map(|m| m.0).collect::<Vec<_>>().join(" | ")
    );
    println!("|---|---|---|{}", "---|".repeat(list.len()));
    for (img, factor, runs) in jobs {
        let mut cells = Vec::new();
        for (_, f) in &list {
            f(img, factor); // warm-up
            let samples = (0..runs)
                .map(|_| {
                    let t = Instant::now();
                    std::hint::black_box(f(img, factor));
                    t.elapsed()
                })
                .collect();
            cells.push(format!("{:.1} ms", median(samples)));
        }
        let (w, h) = img.dimensions();
        println!(
            "| {w}x{h} | {factor}x | {}x{} | {} |",
            w * factor,
            h * factor,
            cells.join(" | ")
        );
    }
}

#[cfg(all(target_os = "macos", feature = "metalfx"))]
fn quirks() {
    use objc2_metal::{MTLDevice, MTLPixelFormat as F};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let gpu = metalfx::Gpu::new().unwrap();
    println!("GPU: {}", gpu.name());
    let d = &gpu.device;
    println!(
        "Apple family 7+: {}, max buffer {} MiB, recommended working set {} MiB",
        d.supportsFamily(objc2_metal::MTLGPUFamily::Apple7),
        d.maxBufferLength() / (1 << 20),
        d.recommendedMaxWorkingSetSize() / (1 << 20)
    );

    // A small opaque test image with hard edges and a gradient.
    let src = RgbaImage::from_fn(64, 48, |x, y| {
        if (x / 8 + y / 8) % 2 == 0 {
            Rgba([250, 250, 250, 255])
        } else {
            Rgba([(x * 4) as u8, 40, (y * 5) as u8, 255])
        }
    });
    let lanczos = cpu::lanczos3(&src, 2);
    let mad = |a: &RgbaImage, b: &RgbaImage| {
        a.as_raw()
            .iter()
            .zip(b.as_raw())
            .map(|(x, y)| (*x as i32 - *y as i32).abs() as f64)
            .sum::<f64>()
            / a.as_raw().len() as f64
    };

    println!("\nFormats and colour modes (2x, mean abs diff vs Lanczos3 in 8-bit steps):");
    let formats = [
        ("RGBA8Unorm", F::RGBA8Unorm),
        ("BGRA8Unorm", F::BGRA8Unorm),
        ("RGBA8Unorm_sRGB", F::RGBA8Unorm_sRGB),
        ("BGRA8Unorm_sRGB", F::BGRA8Unorm_sRGB),
        ("RGBA16Float", F::RGBA16Float),
    ];
    let modes = [
        ("Perceptual", metalfx::Mode::Perceptual),
        ("Linear", metalfx::Mode::Linear),
        ("HDR", metalfx::Mode::HDR),
    ];
    for (fname, f) in formats {
        for (mname, m) in modes {
            let r = catch_unwind(AssertUnwindSafe(|| gpu.upscale(&src, 2, f, m)));
            let msg = match r {
                Ok(Ok(out)) => format!("ok, diff {:.2}", mad(&out, &lanczos)),
                Ok(Err(e)) => format!("error: {e}"),
                Err(_) => "panicked".into(),
            };
            println!("  {fname:16} {mname:10} {msg}");
        }
    }

    println!("\nFactors (64x48 input, RGBA8Unorm, Perceptual):");
    for factor in [1, 2, 3, 4, 5, 6, 8, 16] {
        let r = gpu.upscale(&src, factor, F::RGBA8Unorm, metalfx::Mode::Perceptual);
        println!(
            "  {factor}x: {}",
            r.map(|o| format!("ok {}x{}", o.width(), o.height()))
                .unwrap_or_else(|e| e)
        );
    }

    println!("\nAlpha (straight alpha: red at 50%, and a transparent area holding green):");
    let alpha = RgbaImage::from_fn(32, 32, |x, _| match x {
        0..=10 => Rgba([0, 0, 255, 255]),
        11..=21 => Rgba([255, 0, 0, 128]),
        _ => Rgba([0, 255, 0, 0]),
    });
    let out = gpu
        .upscale(&alpha, 2, F::RGBA8Unorm, metalfx::Mode::Perceptual)
        .unwrap();
    let row: Vec<String> = (0..64)
        .step_by(4)
        .map(|x| format!("{:?}", out.get_pixel(x, 32).0))
        .collect();
    println!("  row 32 every 4 px: {}", row.join(" "));

    println!("\nTexture size limits (thin strips, RGBA8Unorm):");
    // One pixel more (e.g. 4097x4 at 4x) aborts the process in Metal's texture validation.
    for (w, factor) in [(4096u32, 4u32), (8192, 2), (16384, 1)] {
        let strip = RgbaImage::from_pixel(w, 4, Rgba([128, 128, 128, 255]));
        let r = catch_unwind(AssertUnwindSafe(|| {
            gpu.upscale(&strip, factor, F::RGBA8Unorm, metalfx::Mode::Perceptual)
        }));
        let msg = match r {
            Ok(Ok(o)) => format!("ok {}x{}", o.width(), o.height()),
            Ok(Err(e)) => format!("error: {e}"),
            Err(_) => "panicked".into(),
        };
        println!(
            "  {w}x4 at {factor}x -> {}x{}: {msg}",
            w * factor,
            4 * factor
        );
    }
}
