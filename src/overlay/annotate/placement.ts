// Where the toolbar goes, pure. All values are logical px on the display.

import type { Point, Rect, Size } from '../geometry'

const clamp = (v: number, min: number, max: number) =>
  Math.min(Math.max(v, min), Math.max(min, max))

const overlaps = (a: Rect, b: Rect) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height

/**
 * Where to put a toolbar of `size` for the selection `r`: below it, right-aligned, else above,
 * else inside along the bottom, else inside along the top. It stays on the display and moves
 * aside (or to the next spot) to keep clear of the size badge `badge`.
 */
export function toolbarPosition(r: Rect, size: Size, bounds: Size, badge: Rect, gap = 6): Point {
  const ys = [
    r.y + r.height + gap,
    r.y - gap - size.height,
    r.y + r.height - gap - size.height,
    r.y + gap,
  ]
  const fits = (y: number) => y >= 0 && y + size.height <= bounds.height
  const maxX = bounds.width - size.width
  const right = clamp(r.x + r.width - size.width, 0, maxX)
  for (const y of ys.filter(fits)) {
    for (const x of [right, badge.x + badge.width + gap, badge.x - gap - size.width]) {
      if (x < 0 || x > maxX) continue
      if (!overlaps({ x, y, ...size }, badge)) return { x, y }
    }
  }
  // Nowhere clear of the badge: it is small, so cover it rather than leave the screen.
  return { x: right, y: clamp(ys.find(fits) ?? ys[0], 0, bounds.height - size.height) }
}
