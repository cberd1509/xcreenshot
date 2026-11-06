// The annotation model, pure and serializable. Shapes are in logical px relative to the display,
// like the selection, so they stay on the same pixels when the selection moves or resizes. The
// selection only clips what is shown and exported.

import { clamp, span, union, type Point, type Rect, type Scale } from '../geometry'

export type Tool = 'pen' | 'line' | 'arrow' | 'rect' | 'ellipse' | 'highlighter' | 'text' | 'blur'

/** Toolbar order, with each tool's shortcut and label. */
export const TOOLS: readonly { tool: Tool; key: string; label: string }[] = [
  { tool: 'pen', key: 'P', label: 'Pen' },
  { tool: 'line', key: 'L', label: 'Line' },
  { tool: 'arrow', key: 'A', label: 'Arrow' },
  { tool: 'rect', key: 'R', label: 'Rectangle' },
  { tool: 'ellipse', key: 'E', label: 'Ellipse' },
  { tool: 'highlighter', key: 'H', label: 'Highlighter' },
  { tool: 'text', key: 'T', label: 'Text' },
  { tool: 'blur', key: 'B', label: 'Blur to hide info' },
]

/** Tools that draw something today. The rest come in #14–#16. */
export const READY: ReadonlySet<Tool> = new Set<Tool>(['rect', 'arrow'])

/** The tool for a single-letter shortcut, ignoring case. */
export function toolForKey(key: string): Tool | null {
  const k = key.toUpperCase()
  return TOOLS.find((t) => t.key === k)?.tool ?? null
}

export type Style = { color: string; size: number }

export const COLORS = ['#FF3B30', '#FF9500', '#FFCC00', '#34C759', '#007AFF', '#000000', '#FFFFFF']
export const SIZES = [2, 4, 8]
export const DEFAULT_STYLE: Style = { color: COLORS[0], size: SIZES[1] }

type Base = { id: string; style: Style }
export type RectShape = Base & Rect & { kind: 'rect' }
/** Blur hides what is under it; Rust pixelates its rect when composing the image. */
export type BlurShape = Base & Rect & { kind: 'blur' }
/** From (x1, y1) to (x2, y2), the head at the end. */
export type ArrowShape = Base & { kind: 'arrow'; points: [number, number, number, number] }
export type Shape = RectShape | BlurShape | ArrowShape

/** Shapes smaller than this on both axes are a click, not a drag, and are dropped. */
export const MIN_SIZE = 3
/** How much of a shape stays inside the selection when it is dragged out. */
export const MIN_VISIBLE = 8

/** A point clamped to a rect. */
export function clampTo(p: Point, r: Rect): Point {
  return { x: clamp(p.x, r.x, r.x + r.width), y: clamp(p.y, r.y, r.y + r.height) }
}

/**
 * The shape a drag from `a` to `b` draws with `tool`, or `null` if the tool draws nothing yet
 * or the drag is too short.
 */
export function draw(tool: Tool, id: string, a: Point, b: Point, style: Style): Shape | null {
  if (Math.abs(b.x - a.x) < MIN_SIZE && Math.abs(b.y - a.y) < MIN_SIZE) return null
  if (tool === 'arrow') return { id, kind: 'arrow', style, points: [a.x, a.y, b.x, b.y] }
  if (tool === 'rect' || tool === 'blur') return { id, kind: tool, style, ...span(a, b) }
  return null
}

/** Where a shape's Konva node sits: a rect's corner or an arrow's start. */
export function origin(shape: Shape): Point {
  return shape.kind === 'arrow' ? { x: shape.points[0], y: shape.points[1] } : shape
}

/**
 * A node's move or resize, as Konva reports it: its new position (the shape's origin) and
 * scale. Negative scale means it was flipped.
 */
export type Transform = { x: number; y: number; scaleX: number; scaleY: number }

/** The shape after its node moved to (x, y) and was scaled by (scaleX, scaleY). */
export function transform(shape: Shape, t: Transform): Shape {
  if (shape.kind === 'arrow') {
    const [x1, y1, x2, y2] = shape.points
    return {
      ...shape,
      points: [t.x, t.y, t.x + (x2 - x1) * t.scaleX, t.y + (y2 - y1) * t.scaleY],
    }
  }
  const far = { x: t.x + shape.width * t.scaleX, y: t.y + shape.height * t.scaleY }
  return { ...shape, ...span(t, far) }
}

export function translate(shape: Shape, dx: number, dy: number): Shape {
  if (shape.kind !== 'arrow') return { ...shape, x: shape.x + dx, y: shape.y + dy }
  const [x1, y1, x2, y2] = shape.points
  return { ...shape, points: [x1 + dx, y1 + dy, x2 + dx, y2 + dy] }
}

/** The shape's geometry as a box, strokes left out. */
export function bounds(shape: Shape): Rect {
  if (shape.kind !== 'arrow')
    return { x: shape.x, y: shape.y, width: shape.width, height: shape.height }
  const [x1, y1, x2, y2] = shape.points
  return span({ x: x1, y: y1 }, { x: x2, y: y2 })
}

/** The box a shape paints, strokes and arrow heads included. */
export function paintBounds(shape: Shape): Rect {
  const b = bounds(shape)
  const pad = shape.kind === 'arrow' ? shape.style.size * 4 : shape.style.size
  return { x: b.x - pad, y: b.y - pad, width: b.width + 2 * pad, height: b.height + 2 * pad }
}

/** How far to move a span at `pos` of `size` to keep `MIN_VISIBLE` of it in another span. */
function pullBack(pos: number, size: number, start: number, length: number) {
  const m = Math.min(MIN_VISIBLE, size, length)
  return clamp(pos, start + m - size, start + length - m) - pos
}

/** Moves the shape back so at least `MIN_VISIBLE` px of it (or all, if smaller) is in `r`. */
export function keepInside(shape: Shape, r: Rect): Shape {
  const b = bounds(shape)
  const dx = pullBack(b.x, b.width, r.x, r.width)
  const dy = pullBack(b.y, b.height, r.y, r.height)
  return dx === 0 && dy === 0 ? shape : translate(shape, dx, dy)
}

/** The box all the shapes paint, or `null` without shapes. */
export function paintedBounds(shapes: readonly Shape[]): Rect | null {
  return shapes.length ? shapes.map(paintBounds).reduce(union) : null
}

/**
 * Blur rects in physical px of the crop: `crop` is the physical rect being captured and
 * `scale` is physical px per logical px. Rounded out to whole pixels and clipped to the crop.
 */
export function blurRects(shapes: readonly Shape[], crop: Rect, scale: Scale): Rect[] {
  const out: Rect[] = []
  for (const s of shapes) {
    if (s.kind !== 'blur') continue
    const r = physicalIn(s, crop, scale)
    if (r) out.push(r)
  }
  return out
}

/** A logical rect in whole physical px relative to the crop, clipped to it, or `null`. */
export function physicalIn(r: Rect, crop: Rect, scale: Scale): Rect | null {
  const x0 = clamp(Math.floor(r.x * scale.x) - crop.x, 0, crop.width)
  const y0 = clamp(Math.floor(r.y * scale.y) - crop.y, 0, crop.height)
  const x1 = clamp(Math.ceil((r.x + r.width) * scale.x) - crop.x, 0, crop.width)
  const y1 = clamp(Math.ceil((r.y + r.height) * scale.y) - crop.y, 0, crop.height)
  return x1 > x0 && y1 > y0 ? { x: x0, y: y0, width: x1 - x0, height: y1 - y0 } : null
}
