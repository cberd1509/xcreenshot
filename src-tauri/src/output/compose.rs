//! Puts the annotations on the cropped selection, pure.
//!
//! Blur is destructive: each rect is pixelated in large blocks before anything else is drawn,
//! so nothing under it survives in the image (see issue #16). Then the page's annotation layer
//! (straight-alpha RGBA) is drawn over the crop.

use crate::capture::{PixelRect, clamp_rect};
use image::{ImageBuffer, Rgba, RgbaImage};

/// Smallest pixelation block, in physical px. Large enough that text under it is unreadable.
pub const BLOCK: u32 = 16;

/// The page's annotation layer, read in place from the request, and where it goes on the crop.
#[derive(Debug, PartialEq)]
pub struct Layer<'a> {
    pub x: u32,
    pub y: u32,
    pub pixels: ImageBuffer<Rgba<u8>, &'a [u8]>,
}

/// The annotations to put on the crop, in px relative to it.
#[derive(Debug, Default, PartialEq)]
pub struct Marks<'a> {
    pub layer: Option<Layer<'a>>,
    pub blurs: Vec<PixelRect>,
}

/// Pixelates the blur rects, then draws the layer over the crop. Anything outside the crop is
/// left out.
pub fn apply(image: &mut RgbaImage, marks: &Marks) {
    for &rect in &marks.blurs {
        pixelate(image, rect);
    }
    if let Some(layer) = &marks.layer {
        composite(image, layer);
    }
}

/// Draws the layer over the image, source-over, clipped to the image.
///
/// Not `imageops::overlay`: its blend truncates, so a half-transparent stroke over the opaque
/// screenshot comes out with alpha 254, and the PNG would be slightly see-through there.
fn composite(image: &mut RgbaImage, layer: &Layer) {
    let (w, h) = image.dimensions();
    let (lw, lh) = layer.pixels.dimensions();
    let at = PixelRect {
        x: layer.x,
        y: layer.y,
        width: lw,
        height: lh,
    };
    let Some(r) = clamp_rect(at, w, h) else {
        return;
    };
    let (stride, len) = (w as usize * 4, r.width as usize * 4);
    let pixels: &mut [u8] = image;
    for y in 0..r.height as usize {
        let dst = (r.y as usize + y) * stride + r.x as usize * 4;
        let src = y * lw as usize * 4;
        let rows = pixels[dst..dst + len].as_chunks_mut::<4>().0;
        for (d, s) in rows
            .iter_mut()
            .zip(layer.pixels.as_raw()[src..src + len].as_chunks::<4>().0)
        {
            *d = over(*s, *d);
        }
    }
}

/// `src` over `dst`, both straight alpha, rounded to the nearest value.
fn over(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let sa = u32::from(src[3]);
    match sa {
        255 => return src,
        0 => return dst,
        _ => {}
    }
    let da = u32::from(dst[3]);
    // Alpha out, times 255.
    let oa = sa * 255 + da * (255 - sa);
    let mut out = [0u8; 4];
    for i in 0..3 {
        let c = u32::from(src[i]) * sa * 255 + u32::from(dst[i]) * da * (255 - sa);
        out[i] = ((c + oa / 2) / oa) as u8;
    }
    out[3] = ((oa + 127) / 255) as u8;
    out
}

/// Replaces `rect` with flat blocks. Blocks are at least [`BLOCK`] px (a smaller rect is one
/// block) and grow with the rect, so even a large blur leaves no detail. A remainder narrower
/// than a block joins the block before it. Each block's color is its average, nudged until it
/// matches none of the pixels it covers, so every pixel under a blur changes, even in a flat
/// area or a 1 px block.
pub fn pixelate(image: &mut RgbaImage, rect: PixelRect) {
    let (w, h) = image.dimensions();
    let Some(r) = clamp_rect(rect, w, h) else {
        return;
    };
    let block = block_size(r.width, r.height) as usize;
    let stride = w as usize * 4;
    let pixels: &mut [u8] = image;
    for (by, ey) in spans(r.y as usize, r.height as usize, block) {
        for (bx, ex) in spans(r.x as usize, r.width as usize, block) {
            let row = |y: usize| y * stride + bx * 4..y * stride + ex * 4;
            let covered = || (by..ey).flat_map(|y| pixels[row(y)].as_chunks::<4>().0.iter());
            let mut sum = [0u64; 4];
            for px in covered() {
                for (s, &c) in sum.iter_mut().zip(px) {
                    *s += u64::from(c);
                }
            }
            let n = ((ex - bx) * (ey - by)) as u64;
            let mut fill = sum.map(|s| ((s + n / 2) / n) as u8);
            // A block has fewer pixels than there are colors, so this ends.
            while covered().any(|px| *px == fill) {
                fill[2] = fill[2].wrapping_add(1);
                if fill[2] == 0 {
                    fill[1] = fill[1].wrapping_add(1);
                    if fill[1] == 0 {
                        fill[0] = fill[0].wrapping_add(1);
                    }
                }
            }
            for y in by..ey {
                for px in pixels[row(y)].as_chunks_mut::<4>().0 {
                    *px = fill;
                }
            }
        }
    }
}

/// The block size for a `width × height` px blur: [`BLOCK`], or larger for a large rect.
///
/// The overlay's blur preview mirrors this and [`spans`] in `src/overlay/annotate/blocks.ts`,
/// so it shows the blocks painted here. Keep the two in step (their tests pin the same values).
pub fn block_size(width: u32, height: u32) -> u32 {
    BLOCK.max(width.min(height) / 8)
}

/// `len` px from `start` cut into `block`-sized spans, the last one taking any remainder: at
/// least one span, each at least `block` long unless `len` itself is shorter.
fn spans(start: usize, len: usize, block: usize) -> impl Iterator<Item = (usize, usize)> {
    let n = (len / block).max(1);
    (0..n).map(move |i| {
        let end = if i + 1 == n { len } else { (i + 1) * block };
        (start + i * block, start + end)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::tests::px;

    /// Pixel (x, y) is (x, y, 9, 255): every pixel differs.
    fn gradient(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([x as u8, y as u8, 9, 255]))
    }

    fn layer(x: u32, y: u32, w: u32, h: u32, bytes: &[u8]) -> Layer<'_> {
        Layer {
            x,
            y,
            pixels: ImageBuffer::from_raw(w, h, bytes).unwrap(),
        }
    }

    #[test]
    fn draws_the_layer_with_straight_alpha() {
        let mut image = RgbaImage::from_pixel(3, 1, Rgba([0, 0, 255, 255]));
        let bytes = [255, 0, 0, 255, 255, 0, 0, 128, 255, 0, 0, 0];
        let marks = Marks {
            layer: Some(layer(0, 0, 3, 1, &bytes)),
            blurs: vec![],
        };
        apply(&mut image, &marks);
        // Opaque replaces, half blends, transparent leaves the pixel.
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [128, 0, 127, 255]);
        assert_eq!(image.get_pixel(2, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn blends_over_a_transparent_pixel() {
        assert_eq!(over([200, 100, 0, 128], [0, 0, 0, 0]), [200, 100, 0, 128]);
        assert_eq!(over([0, 0, 0, 0], [1, 2, 3, 4]), [1, 2, 3, 4]);
    }

    #[test]
    fn the_layer_touches_nothing_outside_the_crop_or_itself() {
        let original = gradient(4, 4);
        let mut image = original.clone();
        let bytes = [1u8, 1, 1, 255].repeat(9);
        // Placed at (2, 2): only its top-left 2x2 is on the crop.
        let marks = Marks {
            layer: Some(layer(2, 2, 3, 3, &bytes)),
            blurs: vec![],
        };
        apply(&mut image, &marks);
        for (x, y, p) in image.enumerate_pixels() {
            let inside = x >= 2 && y >= 2;
            assert_eq!(*p == Rgba([1, 1, 1, 255]), inside, "({x}, {y})");
            if !inside {
                assert_eq!(p, original.get_pixel(x, y));
            }
        }
        // Wholly outside: nothing happens.
        let off = Marks {
            layer: Some(layer(9, 9, 3, 3, &bytes)),
            blurs: vec![],
        };
        apply(&mut image, &off);
    }

    #[test]
    fn pixelate_destroys_the_pixels_under_the_blur() {
        let original = gradient(64, 64);
        let mut image = original.clone();
        pixelate(&mut image, px(8, 8, 40, 40));
        let mut changed = 0;
        for (x, y, p) in image.enumerate_pixels() {
            let inside = (8..48).contains(&x) && (8..48).contains(&y);
            if !inside {
                assert_eq!(
                    p,
                    original.get_pixel(x, y),
                    "({x}, {y}) is outside the blur"
                );
            } else if p != original.get_pixel(x, y) {
                changed += 1;
            }
        }
        // Nearly every pixel changes; a block is one flat color.
        assert!(changed > 40 * 40 * 9 / 10, "{changed}");
        let block = image.get_pixel(8, 8);
        for y in 8..8 + BLOCK {
            for x in 8..8 + BLOCK {
                assert_eq!(image.get_pixel(x, y), block);
            }
        }
        // The block is the average of what it covered, (16, 16, 9), nudged off pixel (16, 16).
        assert_eq!(block.0, [16, 16, 10, 255]);
    }

    /// Asserts every pixel inside `rect` (clipped to the image) changed, and none outside.
    fn assert_destroyed(original: &RgbaImage, rect: PixelRect) {
        let mut image = original.clone();
        pixelate(&mut image, rect);
        let (w, h) = image.dimensions();
        let r = clamp_rect(rect, w, h).unwrap();
        for (x, y, p) in image.enumerate_pixels() {
            let inside = (r.x..r.x + r.width).contains(&x) && (r.y..r.y + r.height).contains(&y);
            let same = p == original.get_pixel(x, y);
            assert_eq!(same, !inside, "({x}, {y}) in {rect:?}");
        }
    }

    #[test]
    fn pixelate_changes_every_pixel_under_awkward_sizes() {
        let image = gradient(80, 80);
        // Remainders of 1 px, a lone pixel, a rect smaller than a block, and odd sizes.
        for rect in [
            px(3, 5, 17, 33),
            px(10, 10, 1, 1),
            px(0, 0, 33, 1),
            px(7, 9, 5, 3),
            px(1, 1, 49, 65),
        ] {
            assert_destroyed(&image, rect);
        }
    }

    #[test]
    fn pixelate_changes_every_pixel_at_the_crop_edge() {
        let image = gradient(40, 30);
        for rect in [
            px(23, 13, 17, 17),
            px(39, 29, 1, 1),
            px(0, 0, 40, 30),
            px(30, 20, 99, 99),
        ] {
            assert_destroyed(&image, rect);
        }
    }

    #[test]
    fn pixelate_changes_every_pixel_of_a_flat_area() {
        let flat = RgbaImage::from_pixel(50, 50, Rgba([200, 200, 200, 255]));
        for rect in [px(0, 0, 50, 50), px(5, 5, 17, 33), px(49, 49, 1, 1)] {
            assert_destroyed(&flat, rect);
        }
        // Even when nudging has to carry over into green.
        let blue = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255]));
        assert_destroyed(&blue, px(0, 0, 4, 4));
    }

    // The same values as `src/overlay/annotate/blocks.test.ts`.
    #[test]
    fn block_size_grows_with_the_rect() {
        let sizes = [
            (1, 1, 16),
            (10, 300, 16),
            (200, 120, 16),
            (256, 256, 32),
            (800, 400, 50),
            (3024, 1964, 245),
        ];
        for (w, h, block) in sizes {
            assert_eq!(block_size(w, h), block, "{w}×{h}");
        }
    }

    #[test]
    fn spans_merge_a_short_remainder() {
        assert_eq!(spans(0, 33, 16).collect::<Vec<_>>(), [(0, 16), (16, 33)]);
        assert_eq!(spans(5, 1, 16).collect::<Vec<_>>(), [(5, 6)]);
        assert_eq!(spans(0, 32, 16).collect::<Vec<_>>(), [(0, 16), (16, 32)]);
    }

    #[test]
    fn pixelate_grows_blocks_with_the_rect_and_clips_to_the_image() {
        let mut image = gradient(300, 300);
        pixelate(&mut image, px(0, 0, 256, 256));
        // 256 / 8 = 32 px blocks.
        assert_eq!(image.get_pixel(0, 0), image.get_pixel(31, 31));
        assert_ne!(image.get_pixel(0, 0), image.get_pixel(32, 32));
        let mut small = gradient(4, 4);
        pixelate(&mut small, px(2, 2, 100, 100));
        assert_eq!(small.get_pixel(2, 2), small.get_pixel(3, 3));
        assert_eq!(*small.get_pixel(0, 0), Rgba([0, 0, 9, 255]));
        pixelate(&mut small, px(9, 9, 1, 1));
    }

    #[test]
    fn blurs_first_then_draws_the_layer() {
        let mut image = gradient(32, 32);
        let white = [255u8; 4];
        let marks = Marks {
            layer: Some(layer(0, 0, 1, 1, &white)),
            blurs: vec![px(0, 0, 16, 16)],
        };
        apply(&mut image, &marks);
        // The layer is on top of the blur, not blurred into it.
        assert_eq!(*image.get_pixel(0, 0), Rgba(white));
        assert_eq!(image.get_pixel(1, 1), image.get_pixel(15, 15));
        // Nothing to do.
        let before = image.clone();
        apply(&mut image, &Marks::default());
        assert_eq!(image, before);
    }

    /// Composing a full Retina screen's layer: run with `--ignored --nocapture` for timing.
    #[test]
    #[ignore = "timing only"]
    fn time_a_full_screen_layer() {
        let (w, h) = (3024, 1964);
        let mut image = gradient(w, h);
        let bytes = [255u8, 0, 0, 128].repeat((w * h) as usize);
        let marks = Marks {
            layer: Some(layer(0, 0, w, h, &bytes)),
            blurs: vec![px(0, 0, 1000, 600)],
        };
        let started = std::time::Instant::now();
        apply(&mut image, &marks);
        println!("composed {w}x{h} in {:?}", started.elapsed());
    }
}
