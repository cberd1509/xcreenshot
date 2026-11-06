// Selection geometry, pure. The selection lives in logical (CSS) px relative to the display;
// physical px are the frame's real pixels, used for the size badge, the loupe and the crop.

export type Point = { x: number; y: number }
export type Size = { width: number; height: number }
export type Rect = Point & Size
/** Physical px per logical px, on each axis. */
export type Scale = Point

export type Handle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w'
export const HANDLES: readonly Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']

export const CURSORS: Record<Handle | 'move', string> = {
  n: 'ns-resize',
  s: 'ns-resize',
  e: 'ew-resize',
  w: 'ew-resize',
  ne: 'nesw-resize',
  sw: 'nesw-resize',
  nw: 'nwse-resize',
  se: 'nwse-resize',
  move: 'move',
}

/** Arrow keys as unit steps. */
const ARROWS: Record<string, Point> = {
  ArrowLeft: { x: -1, y: 0 },
  ArrowRight: { x: 1, y: 0 },
  ArrowUp: { x: 0, y: -1 },
  ArrowDown: { x: 0, y: 1 },
}

const clamp = (v: number, min: number, max: number) => Math.min(Math.max(v, min), max)
/** A logical edge in whole physical pixels, on the frame. */
const pixelEdge = (v: number, s: number, max: number) => clamp(Math.round(v * s), 0, max)

/** The rect spanning two points (any order), clamped to the display. */
export function fromPoints(a: Point, b: Point, bounds: Size): Rect {
  const x0 = clamp(Math.min(a.x, b.x), 0, bounds.width)
  const x1 = clamp(Math.max(a.x, b.x), 0, bounds.width)
  const y0 = clamp(Math.min(a.y, b.y), 0, bounds.height)
  const y1 = clamp(Math.max(a.y, b.y), 0, bounds.height)
  return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
}

export const isEmpty = (r: Rect) => r.width <= 0 || r.height <= 0

export function contains(r: Rect, p: Point): boolean {
  return p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

/** Moves the rect by (dx, dy), keeping it whole and inside the display. */
export function move(r: Rect, dx: number, dy: number, bounds: Size): Rect {
  const width = Math.min(r.width, bounds.width)
  const height = Math.min(r.height, bounds.height)
  return {
    x: clamp(r.x + dx, 0, bounds.width - width),
    y: clamp(r.y + dy, 0, bounds.height - height),
    width,
    height,
  }
}

/**
 * Arrow-key nudge by `pixels` physical px in the arrow's direction, or `null` for any other
 * key.
 */
export function nudge(
  r: Rect,
  key: string,
  pixels: number,
  scale: Scale,
  bounds: Size,
): Rect | null {
  const d = ARROWS[key]
  return d ? move(r, (d.x * pixels) / scale.x, (d.y * pixels) / scale.y, bounds) : null
}

/**
 * Drags a handle of `r` (the rect when the drag started) by (dx, dy). Dragging past the
 * opposite edge flips the rect instead of making it negative.
 */
export function resize(r: Rect, handle: Handle, dx: number, dy: number, bounds: Size): Rect {
  let [left, top, right, bottom] = [r.x, r.y, r.x + r.width, r.y + r.height]
  if (handle.includes('w')) left += dx
  if (handle.includes('e')) right += dx
  if (handle.includes('n')) top += dy
  if (handle.includes('s')) bottom += dy
  return fromPoints({ x: left, y: top }, { x: right, y: bottom }, bounds)
}

/**
 * Physical pixel rect, rounding each edge to the nearest pixel and clamping to the frame.
 * Same rule as `to_pixels` in Rust, so the badge shows exactly what gets cropped.
 */
export function toPhysical(r: Rect, scale: Scale, frame: Size): Rect {
  const x = pixelEdge(r.x, scale.x, frame.width)
  const y = pixelEdge(r.y, scale.y, frame.height)
  return {
    x,
    y,
    width: pixelEdge(r.x + r.width, scale.x, frame.width) - x,
    height: pixelEdge(r.y + r.height, scale.y, frame.height) - y,
  }
}

function toLogical(p: Rect, scale: Scale): Rect {
  return {
    x: p.x / scale.x,
    y: p.y / scale.y,
    width: p.width / scale.x,
    height: p.height / scale.y,
  }
}

/** The other side for an aspect-locked edit, rounded to whole pixels (at least 1). */
export function lockedSize(edited: keyof Size, value: number, ratio: number): Size {
  return edited === 'width'
    ? { width: value, height: Math.max(1, Math.round(value / ratio)) }
    : { width: Math.max(1, Math.round(value * ratio)), height: value }
}

/**
 * Resizes to an exact physical size from the top-left corner, clamped to the frame. With
 * `keepRatio`, a size that does not fit is scaled down as a whole, so the ratio holds.
 */
export function resizeTo(r: Rect, size: Size, scale: Scale, frame: Size, keepRatio = false): Rect {
  const p = toPhysical(r, scale, frame)
  const max = { width: frame.width - p.x, height: frame.height - p.y }
  let { width, height } = size
  if (keepRatio) {
    const k = Math.min(1, max.width / width, max.height / height)
    width = Math.round(width * k)
    height = Math.round(height * k)
  }
  width = clamp(Math.round(width), 1, max.width)
  height = clamp(Math.round(height), 1, max.height)
  return toLogical({ x: p.x, y: p.y, width, height }, scale)
}

/** `#RRGGBB` for the first pixel of raw RGBA. */
export function hex(rgba: ArrayLike<number>): string {
  const byte = (i: number) => rgba[i].toString(16).padStart(2, '0')
  return `#${byte(0)}${byte(1)}${byte(2)}`.toUpperCase()
}

/** Where to put a box of `size` above `r` (or below if there is no room, else inside). */
export function badgePosition(r: Rect, size: Size, bounds: Size, gap = 6): Point {
  const x = clamp(r.x, 0, Math.max(0, bounds.width - size.width))
  if (r.y - gap - size.height >= 0) return { x, y: r.y - gap - size.height }
  const below = r.y + r.height + gap
  if (below + size.height <= bounds.height) return { x, y: below }
  return { x, y: r.y + gap }
}

/** Where to put the loupe: down-right of the cursor, flipped near the right or bottom edge. */
export function loupePosition(p: Point, size: Size, bounds: Size, gap = 16): Point {
  const x = p.x + gap + size.width <= bounds.width ? p.x + gap : p.x - gap - size.width
  const y = p.y + gap + size.height <= bounds.height ? p.y + gap : p.y - gap - size.height
  return { x: Math.max(0, x), y: Math.max(0, y) }
}
