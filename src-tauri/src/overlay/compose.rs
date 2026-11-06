//! Puts the annotations on the cropped selection, pure.
//!
//! The page sends copy and save as one raw IPC request: the body is the annotation layer as
//! straight-alpha RGBA (never JSON), and the [`HEADER`] holds the session, the selection rect
//! and where the layer goes, plus the blur rects. See `docs/annotations.md`.
//!
//! Blur is destructive: each rect is pixelated in large blocks before anything else is drawn,
//! so nothing under it survives in the image (see issue #16).

use crate::capture::{CaptureError, PixelRect};
use image::{Rgba, RgbaImage};
use serde::Deserialize;
use tauri::http::HeaderMap;
use tauri::ipc::InvokeBody;

/// The request header with the JSON [`Output`] description.
pub const HEADER: &str = "x-xcreenshot-output";

/// Smallest pixelation block, in physical px. Large enough that text under it is unreadable.
pub const BLOCK: u32 = 16;

/// What the page asks to copy or save.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    session: u64,
    rect: PixelRect,
    #[serde(default)]
    annotations: Option<Layout>,
}

/// Where the annotation layer goes and what to blur, in px relative to the selection rect.
#[derive(Debug, Deserialize)]
struct Layout {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    #[serde(default)]
    blurs: Vec<PixelRect>,
}

/// The annotations to put on the crop.
#[derive(Debug, Default, PartialEq)]
pub struct Marks {
    /// The drawn shapes and where their top-left corner goes.
    pub layer: Option<(u32, u32, RgbaImage)>,
    pub blurs: Vec<PixelRect>,
}

/// A copy or save request: the session, the selection in native px, and its annotations.
#[derive(Debug, PartialEq)]
pub struct Output {
    pub session: u64,
    pub rect: PixelRect,
    pub marks: Marks,
}

fn invalid(e: impl std::fmt::Display) -> CaptureError {
    CaptureError::Invalid(e.to_string())
}

/// Reads a copy or save request. The body is the layer's RGBA (empty without one).
pub fn parse(body: &InvokeBody, headers: &HeaderMap) -> Result<Output, CaptureError> {
    let header = headers
        .get(HEADER)
        .ok_or_else(|| invalid(format!("missing {HEADER}")))?;
    let request: Request = serde_json::from_slice(header.as_bytes()).map_err(invalid)?;
    let bytes: &[u8] = match body {
        InvokeBody::Raw(bytes) => bytes,
        // An empty body may arrive as JSON.
        InvokeBody::Json(_) => &[],
    };
    let marks = match request.annotations {
        None => Marks::default(),
        Some(l) => {
            let layer = if l.width == 0 || l.height == 0 {
                None
            } else {
                let image =
                    RgbaImage::from_raw(l.width, l.height, bytes.to_vec()).ok_or_else(|| {
                        invalid(format!(
                            "{} bytes for a {}x{} layer",
                            bytes.len(),
                            l.width,
                            l.height
                        ))
                    })?;
                Some((l.x, l.y, image))
            };
            Marks {
                layer,
                blurs: l.blurs,
            }
        }
    };
    Ok(Output {
        session: request.session,
        rect: request.rect,
        marks,
    })
}

/// Pixelates the blur rects, then draws the layer over the crop. Anything outside the crop is
/// left out.
pub fn apply(image: &mut RgbaImage, marks: &Marks) {
    for &rect in &marks.blurs {
        pixelate(image, rect);
    }
    if let Some((x, y, layer)) = &marks.layer {
        composite(image, layer, *x, *y);
    }
}

/// The part of `rect` on the image, as (x0, y0, x1, y1), or `None` if there is none.
fn clip(image: &RgbaImage, rect: PixelRect) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = image.dimensions();
    let x1 = rect.x.saturating_add(rect.width).min(w);
    let y1 = rect.y.saturating_add(rect.height).min(h);
    (rect.x < x1 && rect.y < y1).then_some((rect.x, rect.y, x1, y1))
}

/// Replaces `rect` with blocks of their average color. Blocks are at least [`BLOCK`] px and
/// grow with the rect, so even a large blur leaves no detail.
pub fn pixelate(image: &mut RgbaImage, rect: PixelRect) {
    let Some((x0, y0, x1, y1)) = clip(image, rect) else {
        return;
    };
    let block = BLOCK.max((x1 - x0).min(y1 - y0) / 8);
    for by in (y0..y1).step_by(block as usize) {
        for bx in (x0..x1).step_by(block as usize) {
            let (ex, ey) = ((bx + block).min(x1), (by + block).min(y1));
            let mut sum = [0u64; 4];
            for y in by..ey {
                for x in bx..ex {
                    for (s, c) in sum.iter_mut().zip(image.get_pixel(x, y).0) {
                        *s += u64::from(c);
                    }
                }
            }
            let n = u64::from((ex - bx) * (ey - by));
            let avg = Rgba(sum.map(|s| ((s + n / 2) / n) as u8));
            for y in by..ey {
                for x in bx..ex {
                    image.put_pixel(x, y, avg);
                }
            }
        }
    }
}

/// Draws `layer` (straight alpha) over `image` at (x, y), source-over.
pub fn composite(image: &mut RgbaImage, layer: &RgbaImage, x: u32, y: u32) {
    let rect = PixelRect {
        x,
        y,
        width: layer.width(),
        height: layer.height(),
    };
    let Some((x0, y0, x1, y1)) = clip(image, rect) else {
        return;
    };
    for iy in y0..y1 {
        for ix in x0..x1 {
            let src = layer.get_pixel(ix - x, iy - y).0;
            let dst = image.get_pixel_mut(ix, iy);
            dst.0 = over(src, dst.0);
        }
    }
}

/// `src` over `dst`, both straight alpha, rounded to the nearest value.
fn over(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let sa = u32::from(src[3]);
    if sa == 255 {
        return src;
    }
    if sa == 0 {
        return dst;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::http::HeaderValue;

    fn px(x: u32, y: u32, width: u32, height: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }

    /// Pixel (x, y) is (x, y, 9, 255): every pixel differs.
    fn gradient(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([x as u8, y as u8, 9, 255]))
    }

    fn headers(json: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(HEADER, HeaderValue::from_str(json).unwrap());
        h
    }

    #[test]
    fn parses_a_plain_request() {
        let h = headers(r#"{"session":3,"rect":{"x":1,"y":2,"width":3,"height":4}}"#);
        let out = parse(&InvokeBody::Raw(vec![]), &h).unwrap();
        assert_eq!(
            out,
            Output {
                session: 3,
                rect: px(1, 2, 3, 4),
                marks: Marks::default(),
            }
        );
        // An empty body may come as JSON.
        let json = parse(&InvokeBody::Json(serde_json::Value::Null), &h).unwrap();
        assert_eq!(json.marks, Marks::default());
    }

    #[test]
    fn parses_the_layer_and_blurs() {
        let h = headers(
            r#"{"session":3,"rect":{"x":0,"y":0,"width":9,"height":9},
                "annotations":{"x":1,"y":2,"width":2,"height":1,
                "blurs":[{"x":0,"y":0,"width":4,"height":4}]}}"#
                .replace('\n', "")
                .as_str(),
        );
        let out = parse(&InvokeBody::Raw(vec![1, 2, 3, 4, 5, 6, 7, 8]), &h).unwrap();
        let layer = RgbaImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(out.marks.layer, Some((1, 2, layer)));
        assert_eq!(out.marks.blurs, [px(0, 0, 4, 4)]);
    }

    #[test]
    fn blurs_alone_have_no_layer() {
        let h = headers(
            r#"{"session":1,"rect":{"x":0,"y":0,"width":9,"height":9},"annotations":{"x":0,"y":0,"width":0,"height":0,"blurs":[{"x":0,"y":0,"width":4,"height":4}]}}"#,
        );
        let out = parse(&InvokeBody::Raw(vec![]), &h).unwrap();
        assert_eq!(out.marks.layer, None);
        assert_eq!(out.marks.blurs.len(), 1);
    }

    #[test]
    fn rejects_a_bad_request() {
        let body = InvokeBody::Raw(vec![1, 2, 3]);
        let missing = parse(&body, &HeaderMap::new()).unwrap_err();
        assert!(matches!(missing, CaptureError::Invalid(_)), "{missing}");
        assert!(parse(&body, &headers("{")).is_err());
        // The body must hold exactly the layer's pixels.
        let short = headers(
            r#"{"session":1,"rect":{"x":0,"y":0,"width":9,"height":9},"annotations":{"x":0,"y":0,"width":2,"height":2}}"#,
        );
        let e = parse(&body, &short).unwrap_err();
        assert!(e.to_string().contains("3 bytes for a 2x2 layer"), "{e}");
    }

    #[test]
    fn composites_straight_alpha_over_the_crop() {
        let mut image = RgbaImage::from_pixel(3, 1, Rgba([0, 0, 255, 255]));
        let layer =
            RgbaImage::from_raw(3, 1, vec![255, 0, 0, 255, 255, 0, 0, 128, 255, 0, 0, 0]).unwrap();
        composite(&mut image, &layer, 0, 0);
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
    fn composite_touches_nothing_outside_the_crop_or_the_layer() {
        let original = gradient(4, 4);
        let mut image = original.clone();
        let layer = RgbaImage::from_pixel(3, 3, Rgba([1, 1, 1, 255]));
        // Placed at (2, 2): only its top-left 2x2 is on the crop.
        composite(&mut image, &layer, 2, 2);
        for (x, y, p) in image.enumerate_pixels() {
            let inside = x >= 2 && y >= 2;
            assert_eq!(*p == Rgba([1, 1, 1, 255]), inside, "({x}, {y})");
            if !inside {
                assert_eq!(p, original.get_pixel(x, y));
            }
        }
        // Wholly outside: nothing happens.
        composite(&mut image, &layer, 9, 9);
    }

    #[test]
    fn pixelate_destroys_the_pixels_under_the_blur() {
        let original = gradient(64, 64);
        let mut image = original.clone();
        let rect = px(8, 8, 40, 40);
        pixelate(&mut image, rect);
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
        // The block is the average of what it covered.
        assert_eq!(block.0, [16, 16, 9, 255]);
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
    fn apply_blurs_first_then_draws_the_layer() {
        let mut image = gradient(32, 32);
        let layer = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255]));
        let marks = Marks {
            layer: Some((0, 0, layer)),
            blurs: vec![px(0, 0, 16, 16)],
        };
        apply(&mut image, &marks);
        // The layer is on top of the blur, not blurred into it.
        assert_eq!(*image.get_pixel(0, 0), Rgba([255, 255, 255, 255]));
        assert_eq!(image.get_pixel(1, 1), image.get_pixel(15, 15));
        // Nothing to do.
        let before = image.clone();
        apply(&mut image, &Marks::default());
        assert_eq!(image, before);
    }
}
