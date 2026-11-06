// The annotation model, pure and serializable. Shapes are in logical px relative to the
// selection's top-left corner, so moving or resizing the selection keeps them in place on it.

import type { Point, Rect, Size } from '../geometry'

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

const clamp = (v: number, min: number, max: number) => Math.min(Math.max(v, min), max)

/** A point clamped to a box of `size` at the origin (the selection, in its own space). */
export function clampTo(p: Point, size: Size): Point {
  return { x: clamp(p.x, 0, size.width), y: clamp(p.y, 0, size.height) }
}

/**
 * The shape a drag from `a` to `b` draws with `tool`, or `null` if the tool draws nothing yet
 * or the drag is too short.
 */
export function draw(tool: Tool, id: string, a: Point, b: Point, style: Style): Shape | null {
  if (Math.abs(b.x - a.x) < MIN_SIZE && Math.abs(b.y - a.y) < MIN_SIZE) return null
  if (tool === 'arrow') return { id, kind: 'arrow', style, points: [a.x, a.y, b.x, b.y] }
  if (tool === 'rect' || tool === 'blur') {
    const x = Math.min(a.x, b.x)
    const y = Math.min(a.y, b.y)
    return { id, kind: tool, style, x, y, width: Math.abs(b.x - a.x), height: Math.abs(b.y - a.y) }
  }
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
  const width = shape.width * t.scaleX
  const height = shape.height * t.scaleY
  return {
    ...shape,
    x: Math.min(t.x, t.x + width),
    y: Math.min(t.y, t.y + height),
    width: Math.abs(width),
    height: Math.abs(height),
  }
}

/**
 * Blur rects in physical px of an image of `size` (the crop), rounded out to whole pixels and
 * clipped to the image. `scale` is physical px per logical px.
 */
export function blurRects(shapes: readonly Shape[], size: Size, scale: number): Rect[] {
  const out: Rect[] = []
  for (const s of shapes) {
    if (s.kind !== 'blur') continue
    const x0 = clamp(Math.floor(s.x * scale), 0, size.width)
    const y0 = clamp(Math.floor(s.y * scale), 0, size.height)
    const x1 = clamp(Math.ceil((s.x + s.width) * scale), 0, size.width)
    const y1 = clamp(Math.ceil((s.y + s.height) * scale), 0, size.height)
    if (x1 > x0 && y1 > y0) out.push({ x: x0, y: y0, width: x1 - x0, height: y1 - y0 })
  }
  return out
}
