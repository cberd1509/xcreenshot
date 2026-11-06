// Where the toolbar goes, pure. All values are logical px on the display.

import { clamp, type Point, type Rect, type Size } from '../geometry'

/**
 * Below the selection `r`, right-aligned; above it if there is no room below; else along the
 * bottom inside it. Always on the display. It may cover the size badge.
 */
export function toolbarPosition(r: Rect, size: Size, bounds: Size, gap = 6): Point {
  const x = clamp(r.x + r.width - size.width, 0, Math.max(0, bounds.width - size.width))
  const maxY = Math.max(0, bounds.height - size.height)
  const below = r.y + r.height + gap
  const above = r.y - gap - size.height
  const y = below <= maxY || above < 0 ? below : above
  return { x, y: clamp(y, 0, maxY) }
}
