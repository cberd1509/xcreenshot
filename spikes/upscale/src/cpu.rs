//! CPU upscalers: nearest, Lanczos3 (two crates) and Lanczos3 + a light unsharp mask.

use fast_image_resize::{
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
    images::{Image, ImageRef},
};
use image::{RgbaImage, imageops};

/// `fast_image_resize` (SIMD). `use_alpha(false)` because region captures are opaque:
/// it skips the premultiply / unpremultiply passes.
pub fn fir(src: &RgbaImage, factor: u32, alg: ResizeAlg) -> RgbaImage {
    let (w, h) = src.dimensions();
    let src_view = ImageRef::new(w, h, src.as_raw(), PixelType::U8x4).unwrap();
    let mut dst = Image::new(w * factor, h * factor, PixelType::U8x4);
    Resizer::new()
        .resize(
            &src_view,
            &mut dst,
            &ResizeOptions::new().resize_alg(alg).use_alpha(false),
        )
        .unwrap();
    RgbaImage::from_raw(w * factor, h * factor, dst.into_vec()).unwrap()
}

pub fn nearest(src: &RgbaImage, factor: u32) -> RgbaImage {
    fir(src, factor, ResizeAlg::Nearest)
}

pub fn lanczos3(src: &RgbaImage, factor: u32) -> RgbaImage {
    fir(src, factor, ResizeAlg::Convolution(FilterType::Lanczos3))
}

/// The `image` crate's own resize: the app already depends on `image`, so this one is free.
pub fn lanczos3_image_crate(src: &RgbaImage, factor: u32) -> RgbaImage {
    let (w, h) = src.dimensions();
    imageops::resize(src, w * factor, h * factor, imageops::FilterType::Lanczos3)
}

/// Strength of the light unsharp mask applied after Lanczos3.
pub const SHARPEN_AMOUNT: f32 = 0.6;

/// Blur radius scales with the factor, so the mask works at the size of a source pixel.
pub fn sharpen_sigma(factor: u32) -> f32 {
    0.4 * factor as f32
}

pub fn lanczos3_sharpen(src: &RgbaImage, factor: u32) -> RgbaImage {
    let mut out = lanczos3(src, factor);
    unsharp(&mut out, sharpen_sigma(factor), SHARPEN_AMOUNT);
    out
}

/// out = img + amount * (img - blur(img)), colour channels only.
/// The blur is two box passes (close to a Gaussian); `imageops::fast_blur` was 10x slower.
pub fn unsharp(img: &mut RgbaImage, sigma: f32, amount: f32) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let r = (sigma.round() as usize).max(1);
    let mut blurred = img.as_raw().clone();
    let mut tmp = vec![0u8; blurred.len()];
    for _ in 0..2 {
        box_h(&blurred, &mut tmp, w, h, r);
        box_v(&tmp, &mut blurred, w, h, r);
    }
    let lut: Vec<i32> = (0..511)
        .map(|d| (amount * (d as f32 - 255.0)).round() as i32)
        .collect();
    for (p, b) in img
        .as_mut()
        .chunks_exact_mut(4)
        .zip(blurred.chunks_exact(4))
    {
        for c in 0..3 {
            let d = p[c] as i32 - b[c] as i32;
            p[c] = (p[c] as i32 + lut[(d + 255) as usize]).clamp(0, 255) as u8;
        }
    }
}

fn box_h(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let n = (2 * r + 1) as u32;
    // Row padded with r + 1 copies of the edge pixel on each side, so the loop has no clamps.
    let mut padded = vec![0u8; (w + 2 * r + 2) * 4];
    for y in 0..h {
        let row = &src[y * w * 4..(y + 1) * w * 4];
        let out = &mut dst[y * w * 4..(y + 1) * w * 4];
        for i in 0..=r {
            padded[i * 4..i * 4 + 4].copy_from_slice(&row[..4]);
            let j = r + 1 + w + i;
            padded[j * 4..j * 4 + 4].copy_from_slice(&row[(w - 1) * 4..]);
        }
        padded[(r + 1) * 4..(r + 1 + w) * 4].copy_from_slice(row);
        let mut sum = [0u32; 4];
        for x in 1..=2 * r + 1 {
            for c in 0..4 {
                sum[c] += padded[x * 4 + c] as u32;
            }
        }
        for x in 0..w {
            for c in 0..4 {
                out[x * 4 + c] = ((sum[c] + n / 2) / n) as u8;
                sum[c] = sum[c] + padded[(x + 2 * r + 2) * 4 + c] as u32
                    - padded[(x + 1) * 4 + c] as u32;
            }
        }
    }
}

fn box_v(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let n = (2 * r + 1) as u32;
    let stride = w * 4;
    let row = |y: isize| y.clamp(0, h as isize - 1) as usize * stride;
    let mut sum = vec![0u32; stride];
    for y in -(r as isize)..=r as isize {
        let o = row(y);
        sum.iter_mut()
            .zip(&src[o..o + stride])
            .for_each(|(s, &v)| *s += v as u32);
    }
    for y in 0..h {
        let out = &mut dst[y * stride..(y + 1) * stride];
        out.iter_mut()
            .zip(&sum)
            .for_each(|(d, &s)| *d = ((s + n / 2) / n) as u8);
        let (add, sub) = (
            row(y as isize + r as isize + 1),
            row(y as isize - r as isize),
        );
        for i in 0..stride {
            sum[i] = sum[i] + src[add + i] as u32 - src[sub + i] as u32;
        }
    }
}
