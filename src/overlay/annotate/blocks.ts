// The blur preview's blocks, pure. They mirror Rust's pixelation in `src-tauri/src/output/
// compose.rs` (`BLOCK`, `block_size`, `spans`): the same block sizes, lined up with the blur
// rect, so the preview shows the blocks the copied image will have. Keep the two in step.

import type { Point, Rect, Scale } from '../geometry'

/** Smallest block, in physical px. Rust's `BLOCK`. */
export const BLOCK = 16

/** The block size for a `width × height` px blur: it grows with the rect. Rust's `block_size`. */
export function blockSize(width: number, height: number): number {
  return Math.max(BLOCK, Math.floor(Math.min(width, height) / 8))
}

/**
 * `len` px from `start` cut into `block`-sized spans, the last one taking any remainder: at
 * least one span, each at least `block` long unless `len` is shorter. Rust's `spans`.
 */
export function spans(start: number, len: number, block: number): [number, number][] {
  const n = Math.max(1, Math.floor(len / block))
  return Array.from({ length: n }, (_, i) => [
    start + i * block,
    start + (i + 1 === n ? len : (i + 1) * block),
  ])
}

/** The frame shrunk for the preview: RGBA, `width × height` px, each `factor` frame px. */
export type Shrunk = { data: Uint8ClampedArray; width: number; height: number; factor: Scale }

export type RGB = [number, number, number]

/** One block of the preview: a rect in frame px and its color. */
export type Block = Rect & { color: string }

/** The shrunk px covering frame px `from` to `to`, at `f` frame px each: at least one. */
function cells(from: number, to: number, f: number, max: number): [number, number] {
  const a = Math.min(Math.floor(from / f), max - 1)
  return [a, Math.min(Math.max(a + 1, Math.ceil(to / f)), max)]
}

/** The average color of `r` (frame px), from the shrunk frame. */
export function average(s: Shrunk, r: Rect): RGB {
  const [x0, x1] = cells(r.x, r.x + r.width, s.factor.x, s.width)
  const [y0, y1] = cells(r.y, r.y + r.height, s.factor.y, s.height)
  const sum = [0, 0, 0]
  for (let y = y0; y < y1; y++) {
    for (let x = x0; x < x1; x++) {
      const i = (y * s.width + x) * 4
      for (let c = 0; c < 3; c++) sum[c] += s.data[i + c]
    }
  }
  const n = (x1 - x0) * (y1 - y0)
  return sum.map((v) => Math.round(v / n)) as RGB
}

/** The shrunk px whose centers are in frame px `from` to `to`, at `f` frame px each. */
function centers(from: number, to: number, f: number, max: number): [number, number] {
  const first = (v: number) => Math.min(Math.max(Math.ceil(v / f - 0.5), 0), max)
  return [first(from), first(to)]
}

/** Paints `r` (frame px) in `color` on the shrunk frame, as Rust paints a block. */
export function paint(s: Shrunk, r: Rect, color: RGB) {
  const [x0, x1] = centers(r.x, r.x + r.width, s.factor.x, s.width)
  const [y0, y1] = centers(r.y, r.y + r.height, s.factor.y, s.height)
  for (let y = y0; y < y1; y++) {
    for (let x = x0; x < x1; x++) s.data.set(color, (y * s.width + x) * 4)
  }
}

/**
 * The blocks Rust paints over the blur `r` (whole px relative to the crop, as Rust gets it),
 * in frame px: `crop` is where the crop is on the frame. Each takes the average color of the
 * shrunk frame under it, and is painted on it, so a later blur over this one averages its
 * blocks, as in Rust.
 */
export function blocks(r: Rect, crop: Point, s: Shrunk): Block[] {
  const block = blockSize(r.width, r.height)
  const out: Block[] = []
  for (const [y0, y1] of spans(r.y, r.height, block)) {
    for (const [x0, x1] of spans(r.x, r.width, block)) {
      const b = { x: crop.x + x0, y: crop.y + y0, width: x1 - x0, height: y1 - y0 }
      const rgb = average(s, b)
      paint(s, b, rgb)
      out.push({ ...b, color: `rgb(${rgb.join(', ')})` })
    }
  }
  return out
}
