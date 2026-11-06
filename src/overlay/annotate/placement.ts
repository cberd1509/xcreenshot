// Where the tool rail and the action bar go, pure. All values are logical px on the display.

import { clamp, type Point, type Rect, type Size } from '../geometry'

/** Space between the selection and the bars, as in the design. */
export const GAP = 12

/**
 * The action bar: below the selection `r`, right-aligned; above it if there is no room below;
 * else along the bottom inside it. Always on the display. It may cover the size badge.
 */
export function toolbarPosition(r: Rect, size: Size, bounds: Size, gap = GAP): Point {
  const x = clamp(r.x + r.width - size.width, 0, Math.max(0, bounds.width - size.width))
  const maxY = Math.max(0, bounds.height - size.height)
  const below = r.y + r.height + gap
  const above = r.y - gap - size.height
  const y = below <= maxY || above < 0 ? below : above
  return { x, y: clamp(y, 0, maxY) }
}

/**
 * The tool rail: right of the selection `r`, top-aligned; left of it if there is no room on the
 * right; else along the right edge inside it. Always on the display.
 */
export function railPosition(r: Rect, size: Size, bounds: Size, gap = GAP): Point {
  const maxX = Math.max(0, bounds.width - size.width)
  const right = r.x + r.width + gap
  const left = r.x - gap - size.width
  const x = right <= maxX ? right : left >= 0 ? left : r.x + r.width - gap - size.width
  return { x: clamp(x, 0, maxX), y: clamp(r.y, 0, Math.max(0, bounds.height - size.height)) }
}
