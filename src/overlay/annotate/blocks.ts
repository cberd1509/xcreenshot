// The blur preview's blocks, pure. They mirror Rust's pixelation in `src-tauri/src/output/
// compose.rs` (`BLOCK`, `block_size`, `spans`): the same block sizes, lined up with the blur
// rect, so the preview shows the blocks the copied image will have. Keep the two in step.

import type { Rect, Scale } from '../geometry'

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

/** One block of the preview: a rect in frame px and its color. */
export type Block = Rect & { color: string }

/** The shrunk px covering frame px `from` to `to`, at `f` frame px each: at least one. */
function cells(from: number, to: number, f: number, max: number): [number, number] {
  const a = Math.min(Math.floor(from / f), max - 1)
  return [a, Math.min(Math.max(a + 1, Math.ceil(to / f)), max)]
}

/** The average color of `r` (frame px), from the shrunk frame, as `rgb(…)`. */
export function average(s: Shrunk, r: Rect): string {
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
  return `rgb(${sum.map((v) => Math.round(v / n)).join(', ')})`
}

/** The blocks Rust paints over `r` (whole frame px), each with its average color. */
export function blocks(r: Rect, s: Shrunk): Block[] {
  const block = blockSize(r.width, r.height)
  const out: Block[] = []
  for (const [y0, y1] of spans(r.y, r.height, block)) {
    for (const [x0, x1] of spans(r.x, r.width, block)) {
      const b = { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
      out.push({ ...b, color: average(s, b) })
    }
  }
  return out
}
