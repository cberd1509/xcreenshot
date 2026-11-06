// The annotation model, pure and serializable. Shapes are in logical px relative to the display,
// like the selection, so they stay on the same pixels when the selection moves or resizes. The
// selection only clips what is shown and exported.

import { clamp, hex, span, union, type Point, type Rect, type Scale } from '../geometry'
import { MAX_FONT, MIN_FONT } from './text'

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

/** Tools that draw something. */
export const READY: ReadonlySet<Tool> = new Set<Tool>([
  'pen',
  'line',
  'arrow',
  'rect',
  'ellipse',
  'highlighter',
  'text',
  'blur',
])

/** Tools drawn freehand, through every point of the drag (see `freehand`). */
export const FREEHAND: ReadonlySet<Tool> = new Set<Tool>(['pen', 'highlighter'])

/** The tool for a single-letter shortcut, ignoring case. */
export function toolForKey(key: string): Tool | null {
  const k = key.toUpperCase()
  return TOOLS.find((t) => t.key === k)?.tool ?? null
}

export type Style = { color: string; size: number }

export const COLORS = ['#FF3B30', '#FF9500', '#FFCC00', '#34C759', '#007AFF', '#000000', '#FFFFFF']
export const SIZES = [2, 4, 8]
export const DEFAULT_STYLE: Style = { color: COLORS[0], size: SIZES[1] }

/** The fully saturated color at `hue` degrees, as `#RRGGBB`. */
export function hueColor(hue: number): string {
  const channel = (n: number) => {
    const k = (n + hue / 60) % 6
    return Math.round(255 * (1 - Math.max(0, Math.min(k, 4 - k, 1))))
  }
  return hex([channel(5), channel(3), channel(1)])
}

type Base = { id: string; style: Style }
export type RectShape = Base & Rect & { kind: 'rect' }
/** The ellipse inside the rect. */
export type EllipseShape = Base & Rect & { kind: 'ellipse' }
/** Blur hides what is under it; Rust pixelates its rect when composing the image. */
export type BlurShape = Base & Rect & { kind: 'blur' }
/** From (x1, y1) to (x2, y2); an arrow has its head at the end. */
export type ArrowShape = Base & { kind: 'arrow'; points: [number, number, number, number] }
export type LineShape = Base & { kind: 'line'; points: [number, number, number, number] }
/** Freehand, through `points` (x1, y1, x2, y2, …), already smoothed. */
export type PenShape = Base & { kind: 'pen' | 'highlighter'; points: number[] }
/**
 * Text in a box sized to fit it (`width`, `height`, measured by the board). The color comes from
 * the style; `style.size` is unused.
 */
export type TextShape = Base &
  Rect & { kind: 'text'; text: string; fontSize: number; background: boolean }
export type Shape =
  RectShape | EllipseShape | BlurShape | ArrowShape | LineShape | PenShape | TextShape
/** Shapes drawn through points rather than in a box. */
export type PathShape = ArrowShape | LineShape | PenShape

export const isPath = (shape: Shape): shape is PathShape => 'points' in shape

/** A highlighter is this many times wider than a pen of the same size. */
export const HIGHLIGHTER_WIDTH = 4

/** Most blurs one image takes: Rust's `MAX_BLURS` in `overlay/export.rs`. */
export const MAX_BLURS = 256

/** Shapes smaller than this on both axes are a click, not a drag, and are dropped. */
export const MIN_SIZE = 3
/** How much of a shape stays inside the selection when it is dragged out. */
export const MIN_VISIBLE = 8

/** A point clamped to a rect. */
export function clampTo(p: Point, r: Rect): Point {
  return { x: clamp(p.x, r.x, r.x + r.width), y: clamp(p.y, r.y, r.y + r.height) }
}

/**
 * The shape a drag from `a` to `b` draws with `tool`, or `null` if the tool does not draw
 * this way (freehand tools use `freehand`) or the drag is too short.
 */
export function draw(tool: Tool, id: string, a: Point, b: Point, style: Style): Shape | null {
  if (Math.abs(b.x - a.x) < MIN_SIZE && Math.abs(b.y - a.y) < MIN_SIZE) return null
  if (tool === 'arrow' || tool === 'line') {
    return { id, kind: tool, style, points: [a.x, a.y, b.x, b.y] }
  }
  if (tool === 'rect' || tool === 'ellipse' || tool === 'blur') {
    return { id, kind: tool, style, ...span(a, b) }
  }
  return null
}

/** Points closer than this to the last one kept add nothing to a freehand stroke. */
export const STEP = 2

/**
 * Chaikin corner cutting, keeping both ends. Every new point lies between two old ones, so the
 * stroke never leaves the box of the points it was drawn through.
 */
export function smooth(points: readonly number[], passes = 2): number[] {
  let p = points
  for (let n = 0; n < passes && p.length >= 6; n++) {
    const out = [p[0], p[1]]
    for (let i = 0; i + 3 < p.length; i += 2) {
      const [x0, y0, x1, y1] = [p[i], p[i + 1], p[i + 2], p[i + 3]]
      out.push(0.75 * x0 + 0.25 * x1, 0.75 * y0 + 0.25 * y1)
      out.push(0.25 * x0 + 0.75 * x1, 0.25 * y0 + 0.75 * y1)
    }
    out.push(p[p.length - 2], p[p.length - 1])
    p = out
  }
  // A copy when no pass ran, so the caller never shares the input.
  return p === points ? [...p] : (p as number[])
}

/**
 * One pass of `smooth`, kept up to date as points are added: only the segments a new point
 * touches are worked out again, so a long stroke costs the same per point as a short one.
 */
class Pass {
  /** The smoothed points, flat. */
  readonly out: number[] = []
  /** How many input points `out` was made from. */
  private n = 0

  /** Brings `out` up to date with `p`, whose points before `from` have not changed. */
  update(p: readonly number[], from: number) {
    const n = p.length / 2
    if (n < 3 || this.n < 3) from = 0
    this.n = n
    if (n < 3) {
      this.out.splice(0, this.out.length, ...p)
      return
    }
    // `out` is the first point, two per segment, then the last point.
    const start = Math.max(0, from - 1)
    this.out.length = 2 + 4 * start
    this.out[0] = p[0]
    this.out[1] = p[1]
    for (let i = 2 * start; i + 3 < p.length; i += 2) {
      const [x0, y0, x1, y1] = [p[i], p[i + 1], p[i + 2], p[i + 3]]
      this.out.push(0.75 * x0 + 0.25 * x1, 0.75 * y0 + 0.25 * y1)
      this.out.push(0.25 * x0 + 0.75 * x1, 0.25 * y0 + 0.75 * y1)
    }
    this.out.push(p[p.length - 2], p[p.length - 1])
  }
}

/**
 * A freehand stroke being drawn: it keeps a point once the pointer has moved `STEP`, and
 * smooths as it goes. The points are always `smooth` of the points kept.
 */
export class Freehand {
  private readonly kept: number[] = []
  private readonly first = new Pass()
  private readonly second = new Pass()
  private box = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity }

  /** Adds `p` unless it is within `STEP` of the last point. Returns whether it did. */
  add(p: Point): boolean {
    const n = this.kept.length
    if (n && Math.hypot(p.x - this.kept[n - 2], p.y - this.kept[n - 1]) < STEP) return false
    this.kept.push(p.x, p.y)
    const b = this.box
    this.box = {
      x0: Math.min(b.x0, p.x),
      y0: Math.min(b.y0, p.y),
      x1: Math.max(b.x1, p.x),
      y1: Math.max(b.y1, p.y),
    }
    const before = this.first.out.length / 2
    this.first.update(this.kept, n / 2)
    this.second.update(this.first.out, before - 1)
    return true
  }

  /** The smoothed points, flat. Live: they change as points are added. */
  get points(): readonly number[] {
    return this.second.out
  }

  /**
   * The stroke `tool` draws, or `null` if it is too small. With `live`, it shares the points
   * (for the preview); otherwise it has its own.
   */
  shape(tool: Tool, id: string, style: Style, live = false): PenShape | null {
    const { x0, y0, x1, y1 } = this.box
    if (!FREEHAND.has(tool) || this.kept.length < 4) return null
    if (x1 - x0 < MIN_SIZE && y1 - y0 < MIN_SIZE) return null
    const points = live ? (this.second.out as number[]) : [...this.second.out]
    return { id, kind: tool as PenShape['kind'], style, points }
  }
}

/** Shift snaps lines and arrows to steps of this many degrees. */
export const SNAP_ANGLE = 15

/** Rounded to a millionth, so flat and upright snapped lines stay exactly so. */
const tidy = (v: number) => Math.round(v * 1e6) / 1e6

/** `b` turned around `a` to the nearest `step` degrees, at the same distance. */
export function snapAngle(a: Point, b: Point, step = SNAP_ANGLE): Point {
  const unit = (step * Math.PI) / 180
  const angle = Math.round(Math.atan2(b.y - a.y, b.x - a.x) / unit) * unit
  const length = Math.hypot(b.x - a.x, b.y - a.y)
  return { x: tidy(a.x + length * Math.cos(angle)), y: tidy(a.y + length * Math.sin(angle)) }
}

const sign = (d: number) => (d < 0 ? -1 : 1)

/** The far corner of a square from `a` towards `b`, its side the drag's longer one. */
export function squareFrom(a: Point, b: Point): Point {
  const side = Math.max(Math.abs(b.x - a.x), Math.abs(b.y - a.y))
  return { x: a.x + side * sign(b.x - a.x), y: a.y + side * sign(b.y - a.y) }
}

/** How much of a step `d` from `from` stays in the span from `start` of `size`. */
const reach = (from: number, d: number, start: number, size: number) =>
  d > 0 ? (start + size - from) / d : d < 0 ? (start - from) / d : 1

/** `b` pulled back towards `a` along the line between them until it is in `r` (`a` is). */
export function fitTo(a: Point, b: Point, r: Rect): Point {
  const t = clamp(
    Math.min(reach(a.x, b.x - a.x, r.x, r.width), reach(a.y, b.y - a.y, r.y, r.height)),
    0,
    1,
  )
  return clampTo({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }, r)
}

/**
 * Where a drag from `a` to `b` ends, inside `r`. With Shift, lines and arrows snap to steps
 * of `SNAP_ANGLE`, and rects, ellipses and blurs become squares and circles.
 */
export function constrain(tool: Tool, a: Point, b: Point, r: Rect, shift: boolean): Point {
  const line = tool === 'line' || tool === 'arrow'
  const box = tool === 'rect' || tool === 'ellipse' || tool === 'blur'
  if (!shift || !(line || box)) return clampTo(b, r)
  return fitTo(a, line ? snapAngle(a, b) : squareFrom(a, b), r)
}

/** Where a shape's Konva node sits: a box's corner or a path's first point. */
export function origin(shape: Shape): Point {
  return isPath(shape) ? { x: shape.points[0], y: shape.points[1] } : shape
}

/**
 * A node's move or resize, as Konva reports it: its new position (the shape's origin) and
 * scale. Negative scale means it was flipped.
 */
export type Transform = { x: number; y: number; scaleX: number; scaleY: number }

/** The shape after its node moved to (x, y) and was scaled by (scaleX, scaleY). */
export function transform(shape: Shape, t: Transform): Shape {
  if (isPath(shape)) {
    const [x0, y0] = shape.points
    const points = shape.points.map((v, i) =>
      i % 2 ? t.y + (v - y0) * t.scaleY : t.x + (v - x0) * t.scaleX,
    )
    return { ...shape, points } as PathShape
  }
  if (shape.kind === 'text') {
    // Text scales as a whole: the font grows or shrinks, it never stretches.
    const k = clamp(Math.abs(t.scaleY), MIN_FONT / shape.fontSize, MAX_FONT / shape.fontSize)
    const far = {
      x: t.x + shape.width * k * Math.sign(t.scaleX || 1),
      y: t.y + shape.height * k * Math.sign(t.scaleY || 1),
    }
    return { ...shape, ...span(t, far), fontSize: shape.fontSize * k }
  }
  const far = { x: t.x + shape.width * t.scaleX, y: t.y + shape.height * t.scaleY }
  return { ...shape, ...span(t, far) }
}

export function translate(shape: Shape, dx: number, dy: number): Shape {
  if (!isPath(shape)) return { ...shape, x: shape.x + dx, y: shape.y + dy }
  const points = shape.points.map((v, i) => v + (i % 2 ? dy : dx))
  return { ...shape, points } as PathShape
}

/** The box around flat points (x1, y1, x2, y2, …). */
export function pointBounds(points: readonly number[]): Rect {
  let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity]
  for (let i = 0; i + 1 < points.length; i += 2) {
    x0 = Math.min(x0, points[i])
    x1 = Math.max(x1, points[i])
    y0 = Math.min(y0, points[i + 1])
    y1 = Math.max(y1, points[i + 1])
  }
  return span({ x: x0, y: y0 }, { x: x1, y: y1 })
}

/** The shape's geometry as a box, strokes left out. */
export function bounds(shape: Shape): Rect {
  if (isPath(shape)) return pointBounds(shape.points)
  return { x: shape.x, y: shape.y, width: shape.width, height: shape.height }
}

/** How far a shape paints past its geometry: its stroke, or its arrow head. */
function overhang(shape: Shape): number {
  const { size } = shape.style
  if (shape.kind === 'arrow') return size * 4
  if (shape.kind === 'highlighter') return (size * HIGHLIGHTER_WIDTH) / 2
  // Text paints inside its box; 1 px more keeps antialiased edges.
  if (shape.kind === 'text') return 1
  return size
}

/** The box a shape paints, strokes and arrow heads included. */
export function paintBounds(shape: Shape): Rect {
  const b = bounds(shape)
  const pad = overhang(shape)
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
  // Never more than Rust takes. The tool stops at the limit; past it, the last rect covers
  // all the rest, so nothing is ever left unblurred.
  if (out.length <= MAX_BLURS) return out
  return [...out.slice(0, MAX_BLURS - 1), out.slice(MAX_BLURS - 1).reduce(union)]
}

/** A logical rect in whole physical px relative to the crop, clipped to it, or `null`. */
export function physicalIn(r: Rect, crop: Rect, scale: Scale): Rect | null {
  const x0 = clamp(Math.floor(r.x * scale.x) - crop.x, 0, crop.width)
  const y0 = clamp(Math.floor(r.y * scale.y) - crop.y, 0, crop.height)
  const x1 = clamp(Math.ceil((r.x + r.width) * scale.x) - crop.x, 0, crop.width)
  const y1 = clamp(Math.ceil((r.y + r.height) * scale.y) - crop.y, 0, crop.height)
  return x1 > x0 && y1 > y0 ? { x: x0, y: y0, width: x1 - x0, height: y1 - y0 } : null
}
