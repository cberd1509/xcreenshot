//! Labels for the comparison tiles, drawn with a system font read at run time.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};

pub struct Labeller {
    font: FontVec,
}

impl Labeller {
    pub fn new() -> Self {
        let bytes = std::fs::read("/System/Library/Fonts/Supplemental/Arial Bold.ttf")
            .expect("Arial Bold is part of macOS");
        Self {
            font: FontVec::try_from_vec(bytes).unwrap(),
        }
    }

    /// Draw `text` with its top-left corner at (x, y), blending white over the image.
    pub fn draw(&self, img: &mut RgbaImage, text: &str, x: f32, y: f32, px: f32) {
        let font = self.font.as_scaled(PxScale::from(px));
        let mut caret = x;
        let mut last = None;
        for ch in text.chars() {
            let id = font.glyph_id(ch);
            if let Some(prev) = last {
                caret += font.kern(prev, id);
            }
            let glyph = id.with_scale_and_position(px, ab_glyph::point(caret, y + font.ascent()));
            caret += font.h_advance(id);
            last = Some(id);
            let Some(outline) = self.font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let (px, py) = (
                    bounds.min.x as i32 + gx as i32,
                    bounds.min.y as i32 + gy as i32,
                );
                if px < 0 || py < 0 || px >= img.width() as i32 || py >= img.height() as i32 {
                    return;
                }
                let p = img.get_pixel_mut(px as u32, py as u32);
                for c in 0..3 {
                    p[c] = (p[c] as f32 * (1.0 - cov) + 255.0 * cov) as u8;
                }
            });
        }
    }
}

pub fn fill(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, colour: Rgba<u8>) {
    for yy in y..(y + h).min(img.height()) {
        for xx in x..(x + w).min(img.width()) {
            img.put_pixel(xx, yy, colour);
        }
    }
}
