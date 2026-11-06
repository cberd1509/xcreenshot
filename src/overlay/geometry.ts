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

export const clamp = (v: number, min: number, max: number) => Math.min(Math.max(v, min), max)
/** A logical edge in whole physical pixels, on the frame. */
const pixelEdge = (v: number, s: number, max: number) => clamp(Math.round(v * s), 0, max)

/** The rect spanning two points (any order), anywhere, negative coordinates included. */
export function span(a: Point, b: Point): Rect {
  const x = Math.min(a.x, b.x)
  const y = Math.min(a.y, b.y)
  return { x, y, width: Math.abs(b.x - a.x), height: Math.abs(b.y - a.y) }
}

/** The rect spanning two points (any order), clamped to the display. */
export function fromPoints(a: Point, b: Point, bounds: Size): Rect {
  const corner = (p: Point) => ({ x: clamp(p.x, 0, bounds.width), y: clamp(p.y, 0, bounds.height) })
  return span(corner(a), corner(b))
}

/** The smallest rect holding both. */
export function union(a: Rect, b: Rect): Rect {
  return span(
    { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y) },
    { x: Math.max(a.x + a.width, b.x + b.width), y: Math.max(a.y + a.height, b.y + b.height) },
  )
}

/** Where two rects overlap, or `null` if they do not. */
export function intersect(a: Rect, b: Rect): Rect | null {
  const x = Math.max(a.x, b.x)
  const y = Math.max(a.y, b.y)
  const right = Math.min(a.x + a.width, b.x + b.width)
  const bottom = Math.min(a.y + a.height, b.y + b.height)
  return right > x && bottom > y ? { x, y, width: right - x, height: bottom - y } : null
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

/** -1 for a negative size, else 1 (also for none). */
const sign = (v: number | null) => (v !== null && v < 0 ? -1 : 1)

/** Where a handle drags one axis from: its low edge, its high edge, or (edge handles) both. */
type Anchor = 'low' | 'high' | 'middle'

/**
 * `resize`, keeping the aspect ratio `r` had. A corner follows whichever side the drag grew
 * more; an edge handle sets one side and grows the other evenly around its middle. A size that
 * does not fit the display is scaled down as a whole, so the ratio holds.
 */
export function resizeLocked(r: Rect, handle: Handle, dx: number, dy: number, bounds: Size): Rect {
  const ratio = r.width / r.height
  if (!(Number.isFinite(ratio) && ratio > 0)) return resize(r, handle, dx, dy, bounds)
  // Signed: a drag past the opposite edge flips the rect, as `resize` does.
  const dragW = handle.includes('w') ? r.width - dx : handle.includes('e') ? r.width + dx : null
  const dragH = handle.includes('n') ? r.height - dy : handle.includes('s') ? r.height + dy : null
  let width: number
  let height: number
  if (dragW !== null && (dragH === null || Math.abs(dragW) / ratio >= Math.abs(dragH))) {
    width = dragW
    height = (sign(dragH) * Math.abs(dragW)) / ratio
  } else {
    height = dragH!
    width = sign(dragW) * Math.abs(height) * ratio
  }
  const ax: Anchor = handle.includes('e') ? 'low' : handle.includes('w') ? 'high' : 'middle'
  const ay: Anchor = handle.includes('s') ? 'low' : handle.includes('n') ? 'high' : 'middle'
  const x = axis(ax, r.x, r.width, width, bounds.width)
  const y = axis(ay, r.y, r.height, height, bounds.height)
  const k = Math.min(1, x.room, y.room)
  const [left, right] = x.edges(k)
  const [top, bottom] = y.edges(k)
  return span({ x: left, y: top }, { x: right, y: bottom })
}

/**
 * One axis of `resizeLocked`: how much of `size` fits (as a fraction, at most 1 matters) and
 * its low and high edges at a fraction `k` of it.
 */
function axis(anchor: Anchor, start: number, length: number, size: number, max: number) {
  const from = anchor === 'low' ? start : anchor === 'high' ? start + length : start + length / 2
  // A low anchor grows up the axis, a high one down it; a negative size flips that.
  const up = anchor === 'low' ? size >= 0 : size < 0
  const free = anchor === 'middle' ? 2 * Math.min(from, max - from) : up ? max - from : from
  const abs = Math.abs(size)
  return {
    room: abs > 0 ? Math.max(0, free) / abs : 1,
    edges: (k: number): [number, number] => {
      const d = abs * k
      if (anchor === 'middle') return [from - d / 2, from + d / 2]
      return up ? [from, from + d] : [from - d, from]
    },
  }
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
  // A ratio from an empty size (0, NaN, Infinity) keeps the sides equal instead.
  if (!(Number.isFinite(ratio) && ratio > 0)) ratio = 1
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
export function badgePosition(r: Rect, size: Size, bounds: Size, gap = 8): Point {
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
