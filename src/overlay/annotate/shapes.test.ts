import { describe, expect, it } from 'vitest'
import {
  DEFAULT_STYLE,
  HIGHLIGHTER_WIDTH,
  STEP,
  constrain,
  extend,
  fitTo,
  freehand,
  hueColor,
  pointBounds,
  smooth,
  snapAngle,
  squareFrom,
  MIN_VISIBLE,
  READY,
  TOOLS,
  blurRects,
  bounds,
  clampTo,
  draw,
  keepInside,
  origin,
  paintBounds,
  paintedBounds,
  physicalIn,
  toolForKey,
  transform,
  translate,
  type Shape,
} from './shapes'

const style = DEFAULT_STYLE
const rect: Shape = { id: 'r', kind: 'rect', style, x: 10, y: 20, width: 30, height: 40 }
const arrow: Shape = { id: 'a', kind: 'arrow', style, points: [10, 20, 40, 60] }
/** Points from flat coordinates. */
const pts = (...xy: number[]) => xy.flatMap((v, i) => (i % 2 ? [] : [{ x: v, y: xy[i + 1] }]))
const deg = (p: { x: number; y: number }) => (Math.atan2(p.y, p.x) * 180) / Math.PI
const blur = (x: number, y: number, width: number, height: number): Shape => ({
  id: 'b',
  kind: 'blur',
  style,
  x,
  y,
  width,
  height,
})

describe('tools', () => {
  it('lists every tool once, in toolbar order, with a single-letter key', () => {
    expect(TOOLS.map((t) => t.tool)).toEqual([
      'pen',
      'line',
      'arrow',
      'rect',
      'ellipse',
      'highlighter',
      'text',
      'blur',
    ])
    expect(new Set(TOOLS.map((t) => t.key)).size).toBe(TOOLS.length)
    for (const t of TOOLS) expect(t.key).toMatch(/^[A-Z]$/)
  })

  it('maps letters to tools, ignoring case', () => {
    expect(toolForKey('r')).toBe('rect')
    expect(toolForKey('A')).toBe('arrow')
    expect(toolForKey('b')).toBe('blur')
    expect(toolForKey('x')).toBeNull()
    expect(toolForKey('Enter')).toBeNull()
  })

  it('draws with every tool', () => {
    expect([...READY].toSorted()).toEqual([
      'arrow',
      'blur',
      'ellipse',
      'highlighter',
      'line',
      'pen',
      'rect',
      'text',
    ])
  })
})

describe('hueColor', () => {
  it('gives the fully saturated color at a hue', () => {
    expect([0, 60, 120, 180, 240, 300, 30].map(hueColor)).toEqual([
      '#FF0000',
      '#FFFF00',
      '#00FF00',
      '#00FFFF',
      '#0000FF',
      '#FF00FF',
      '#FF8000',
    ])
  })
})

describe('draw', () => {
  it('makes a rect from a drag in any direction', () => {
    expect(draw('rect', 'x', { x: 50, y: 60 }, { x: 10, y: 20 }, style)).toEqual({
      id: 'x',
      kind: 'rect',
      style,
      x: 10,
      y: 20,
      width: 40,
      height: 40,
    })
  })

  it('makes a blur rect', () => {
    expect(draw('blur', 'x', { x: 0, y: 0 }, { x: 5, y: 6 }, style)).toMatchObject({
      kind: 'blur',
      width: 5,
      height: 6,
    })
  })

  it('makes an arrow from start to end', () => {
    expect(draw('arrow', 'x', { x: 50, y: 60 }, { x: 10, y: 20 }, style)).toEqual({
      id: 'x',
      kind: 'arrow',
      style,
      points: [50, 60, 10, 20],
    })
  })

  it('drops a click or a tiny drag', () => {
    expect(draw('rect', 'x', { x: 5, y: 5 }, { x: 7, y: 7 }, style)).toBeNull()
    // Long enough on one axis is a drag.
    expect(draw('arrow', 'x', { x: 5, y: 5 }, { x: 5, y: 9 }, style)).not.toBeNull()
  })

  it('makes a line and an ellipse', () => {
    expect(draw('line', 'x', { x: 5, y: 6 }, { x: 50, y: 60 }, style)).toEqual({
      id: 'x',
      kind: 'line',
      style,
      points: [5, 6, 50, 60],
    })
    expect(draw('ellipse', 'x', { x: 50, y: 60 }, { x: 10, y: 20 }, style)).toMatchObject({
      kind: 'ellipse',
      x: 10,
      y: 20,
      width: 40,
      height: 40,
    })
  })

  it('draws nothing by dragging with a freehand tool or text', () => {
    expect(draw('pen', 'x', { x: 0, y: 0 }, { x: 50, y: 50 }, style)).toBeNull()
    expect(draw('text', 'x', { x: 0, y: 0 }, { x: 50, y: 50 }, style)).toBeNull()
  })
})

describe('freehand', () => {
  it('keeps a point only once the pointer has moved far enough', () => {
    const points = pts(10, 10)
    expect(extend(points, { x: 11, y: 11 })).toBe(false)
    expect(extend(points, { x: 10, y: 10 + STEP })).toBe(true)
    expect(points).toEqual(pts(10, 10, 10, 12))
    const empty: { x: number; y: number }[] = []
    expect(extend(empty, { x: 1, y: 1 })).toBe(true)
  })

  it('smooths corners and keeps both ends', () => {
    expect(smooth([0, 0, 8, 0, 8, 8], 1)).toEqual([0, 0, 2, 0, 6, 0, 8, 2, 8, 6, 8, 8])
    // Two passes by default: 3 points, then 6, then 12.
    expect(smooth([0, 0, 8, 0, 8, 8])).toHaveLength(24)
    // Too short to smooth, and never the same array.
    const two = [0, 0, 5, 5]
    expect(smooth(two)).toEqual(two)
    expect(smooth(two)).not.toBe(two)
  })

  it('never leaves the box of the points drawn through', () => {
    const raw = [0, 0, 40, 30, 5, 60, 90, 10, 50, 50]
    const s = smooth(raw)
    const [b, r] = [pointBounds(s), pointBounds(raw)]
    expect(b.x).toBeGreaterThanOrEqual(r.x)
    expect(b.y).toBeGreaterThanOrEqual(r.y)
    expect(b.x + b.width).toBeLessThanOrEqual(r.x + r.width)
    expect(b.y + b.height).toBeLessThanOrEqual(r.y + r.height)
  })

  it('makes a pen or highlighter stroke, smoothed', () => {
    const p = freehand('pen', 'x', pts(0, 0, 10, 0, 10, 10), style)
    expect(p).toMatchObject({ id: 'x', kind: 'pen', style })
    expect(p?.points).toEqual(smooth([0, 0, 10, 0, 10, 10]))
    expect(freehand('highlighter', 'x', pts(0, 0, 10, 0), style)?.kind).toBe('highlighter')
  })

  it('drops a dot, a single point, and other tools', () => {
    expect(freehand('pen', 'x', pts(0, 0, 2, 2), style)).toBeNull()
    expect(freehand('pen', 'x', pts(0, 0), style)).toBeNull()
    expect(freehand('pen', 'x', [], style)).toBeNull()
    expect(freehand('rect', 'x', pts(0, 0, 50, 50), style)).toBeNull()
  })
})

describe('constrain', () => {
  const r = { x: 0, y: 0, width: 200, height: 100 }
  const a = { x: 50, y: 50 }

  it('snaps to 15° steps, keeping the length', () => {
    expect(snapAngle(a, { x: 90, y: 52 })).toEqual({ x: expect.closeTo(90.05, 2), y: 50 })
    expect(snapAngle(a, { x: 51, y: 10 })).toEqual({ x: 50, y: expect.closeTo(9.99, 1) })
    const d = snapAngle({ x: 0, y: 0 }, { x: 10, y: 9 })
    expect(d.x).toBeCloseTo(d.y)
    expect(deg(snapAngle({ x: 0, y: 0 }, { x: 100, y: 30 }))).toBeCloseTo(15)
  })

  it('makes a square on the longer side, in the drag’s direction', () => {
    expect(squareFrom(a, { x: 60, y: 90 })).toEqual({ x: 90, y: 90 })
    expect(squareFrom(a, { x: 20, y: 45 })).toEqual({ x: 20, y: 20 })
    // A flat drag still goes somewhere.
    expect(squareFrom(a, { x: 50, y: 10 })).toEqual({ x: 90, y: 10 })
  })

  it('pulls a point back along the drag into the rect', () => {
    expect(fitTo(a, { x: 150, y: 150 }, r)).toEqual({ x: 100, y: 100 })
    expect(fitTo(a, { x: -50, y: 50 }, r)).toEqual({ x: 0, y: 50 })
    expect(fitTo(a, { x: 60, y: 60 }, r)).toEqual({ x: 60, y: 60 })
    expect(fitTo(a, a, r)).toEqual(a)
  })

  it('clamps without Shift, and snaps or squares with it', () => {
    expect(constrain('line', a, { x: 300, y: 52 }, r, false)).toEqual({ x: 200, y: 52 })
    expect(constrain('line', a, { x: 90, y: 52 }, r, true).y).toBe(50)
    expect(constrain('arrow', a, { x: 300, y: 52 }, r, true)).toEqual({ x: 200, y: 50 })
    // A square that would leave the rect shrinks, still square.
    expect(constrain('rect', a, { x: 160, y: 70 }, r, true)).toEqual({ x: 100, y: 100 })
    expect(constrain('ellipse', a, { x: 70, y: 60 }, r, true)).toEqual({ x: 70, y: 70 })
    expect(constrain('blur', a, { x: 70, y: 60 }, r, true)).toEqual({ x: 70, y: 70 })
    expect(constrain('pen', a, { x: 300, y: 60 }, r, true)).toEqual({ x: 200, y: 60 })
  })
})

describe('geometry', () => {
  const sel = { x: 100, y: 50, width: 200, height: 100 }

  it('clamps a point to the selection', () => {
    expect(clampTo({ x: 5, y: 500 }, sel)).toEqual({ x: 100, y: 150 })
    expect(clampTo({ x: 150, y: 60 }, sel)).toEqual({ x: 150, y: 60 })
  })

  it('puts a rect node at its corner and an arrow node at its start', () => {
    expect(origin(rect)).toMatchObject({ x: 10, y: 20 })
    expect(origin(arrow)).toEqual({ x: 10, y: 20 })
  })

  it('moves a rect and an arrow', () => {
    const t = { x: 15, y: 25, scaleX: 1, scaleY: 1 }
    expect(transform(rect, t)).toMatchObject({ x: 15, y: 25, width: 30, height: 40 })
    expect(transform(arrow, t)).toMatchObject({ points: [15, 25, 45, 65] })
    expect(translate(rect, 5, 5)).toMatchObject({ x: 15, y: 25, width: 30 })
    expect(translate(arrow, 5, 5)).toMatchObject({ points: [15, 25, 45, 65] })
  })

  it('resizes a rect and an arrow', () => {
    const t = { x: 10, y: 20, scaleX: 2, scaleY: 0.5 }
    expect(transform(rect, t)).toMatchObject({ x: 10, y: 20, width: 60, height: 20 })
    expect(transform(arrow, t)).toMatchObject({ points: [10, 20, 70, 40] })
  })

  it('normalizes a flipped rect', () => {
    const t = { x: 40, y: 60, scaleX: -1, scaleY: -1 }
    expect(transform(rect, t)).toMatchObject({ x: 10, y: 20, width: 30, height: 40 })
  })

  it('keeps the id and style', () => {
    const t = { x: 0, y: 0, scaleX: 1, scaleY: 1 }
    expect(transform(rect, t)).toMatchObject({ id: 'r', style })
  })

  it('moves and resizes every point of a path', () => {
    const pen: Shape = { id: 'p', kind: 'pen', style, points: [10, 20, 20, 30, 40, 10] }
    expect(origin(pen)).toEqual({ x: 10, y: 20 })
    expect(translate(pen, 5, -5)).toMatchObject({ points: [15, 15, 25, 25, 45, 5] })
    expect(transform(pen, { x: 0, y: 0, scaleX: 2, scaleY: -1 })).toMatchObject({
      points: [0, 0, 20, -10, 60, 10],
    })
    expect(bounds(pen)).toEqual({ x: 10, y: 10, width: 30, height: 20 })
  })

  it('boxes what an ellipse, a line and a highlighter paint', () => {
    expect(paintBounds({ ...rect, kind: 'ellipse' })).toEqual(paintBounds(rect))
    const line: Shape = { id: 'l', kind: 'line', style, points: [10, 20, 40, 20] }
    expect(paintBounds(line)).toEqual({ x: 6, y: 16, width: 38, height: 8 })
    // A highlighter is wider: half its width on each side.
    const hl: Shape = { ...line, kind: 'highlighter', points: [10, 20, 40, 20] }
    const half = (style.size * HIGHLIGHTER_WIDTH) / 2
    expect(paintBounds(hl)).toEqual({
      x: 10 - half,
      y: 20 - half,
      width: 30 + 2 * half,
      height: 2 * half,
    })
  })

  it('boxes a shape, with and without its stroke', () => {
    const backwards: Shape = { ...arrow, points: [40, 60, 10, 20] }
    expect(bounds(backwards)).toEqual({ x: 10, y: 20, width: 30, height: 40 })
    expect(bounds(rect)).toEqual({ x: 10, y: 20, width: 30, height: 40 })
    expect(paintBounds(rect)).toEqual({ x: 6, y: 16, width: 38, height: 48 })
    // Arrow heads stick out further.
    expect(paintBounds(arrow)).toEqual({ x: -6, y: 4, width: 62, height: 72 })
  })

  it('round-trips through JSON', () => {
    const shapes = [rect, arrow]
    expect(JSON.parse(JSON.stringify(shapes))).toEqual(shapes)
  })
})

describe('keepInside', () => {
  const sel = { x: 100, y: 100, width: 200, height: 100 }

  it('leaves a shape that is still in the selection', () => {
    const s: Shape = { ...rect, x: 290, y: 90 }
    expect(keepInside(s, sel)).toBe(s)
  })

  it('pulls back a shape dragged out, until a strip of it shows', () => {
    expect(keepInside({ ...rect, x: 500, y: 500 }, sel)).toMatchObject({
      x: 300 - MIN_VISIBLE,
      y: 200 - MIN_VISIBLE,
    })
    expect(keepInside({ ...rect, x: -500, y: -500 }, sel)).toMatchObject({
      x: 100 + MIN_VISIBLE - 30,
      y: 100 + MIN_VISIBLE - 40,
    })
  })

  it('pulls back a shape dragged past the left or top edge of the display', () => {
    const edge = { x: 0, y: 0, width: 200, height: 100 }
    const out: Shape = { ...rect, x: -100, y: -80 }
    // Its size stays whole: nothing is cut at 0.
    expect(bounds(out)).toEqual({ x: -100, y: -80, width: 30, height: 40 })
    expect(keepInside(out, edge)).toMatchObject({ x: MIN_VISIBLE - 30, y: MIN_VISIBLE - 40 })
    const arrowOut: Shape = { ...arrow, points: [-50, -60, -20, -30] }
    expect(keepInside(arrowOut, edge)).toMatchObject({
      points: [MIN_VISIBLE - 30, MIN_VISIBLE - 30, MIN_VISIBLE, MIN_VISIBLE],
    })
  })

  it('moves and resizes into negative coordinates without cutting the shape', () => {
    const t = { x: -10, y: -20, scaleX: -1, scaleY: 1 }
    expect(transform(rect, t)).toMatchObject({ x: -40, y: -20, width: 30, height: 40 })
  })

  it('boxes everything the shapes paint', () => {
    expect(paintedBounds([])).toBeNull()
    expect(paintedBounds([rect, { ...rect, x: 100 }])).toEqual({
      x: 6,
      y: 16,
      width: 128,
      height: 48,
    })
  })

  it('keeps a flat arrow on the selection', () => {
    const flat: Shape = { ...arrow, points: [0, 150, 50, 150] }
    expect(keepInside(flat, sel)).toMatchObject({
      points: [50 + MIN_VISIBLE, 150, 100 + MIN_VISIBLE, 150],
    })
  })
})

describe('blurRects', () => {
  const crop = { x: 0, y: 0, width: 100, height: 100 }
  const one = { x: 1, y: 1 }

  it('uses each axis’s own scale', () => {
    expect(blurRects([blur(10, 10, 10, 10)], crop, { x: 2, y: 1.5 })).toEqual([
      { x: 20, y: 15, width: 20, height: 15 },
    ])
  })

  it('scales blur rects to physical px of the crop, rounding outwards', () => {
    expect(blurRects([blur(1.2, 2.6, 10, 10)], crop, { x: 2, y: 2 })).toEqual([
      { x: 2, y: 5, width: 21, height: 21 },
    ])
    // Relative to the crop's corner.
    expect(blurRects([blur(10, 10, 10, 10)], { ...crop, x: 5, y: 8 }, one)).toEqual([
      { x: 5, y: 2, width: 10, height: 10 },
    ])
  })

  it('clips to the crop and drops rects outside it', () => {
    const small = { x: 0, y: 0, width: 50, height: 40 }
    expect(blurRects([blur(20, 10, 100, 100), blur(60, 0, 5, 5)], small, one)).toEqual([
      { x: 20, y: 10, width: 30, height: 30 },
    ])
    expect(physicalIn({ x: 60, y: 0, width: 5, height: 5 }, small, one)).toBeNull()
  })

  it('skips other shapes', () => {
    expect(blurRects([rect, arrow], crop, one)).toEqual([])
  })
})

describe('text', () => {
  const text: Shape = {
    id: 't',
    kind: 'text',
    style: DEFAULT_STYLE,
    x: 100,
    y: 50,
    width: 80,
    height: 40,
    text: 'Hi',
    fontSize: 20,
    background: false,
  }

  it('moves, boxes and sits at its corner', () => {
    expect(origin(text)).toMatchObject({ x: 100, y: 50 })
    expect(translate(text, 5, -5)).toMatchObject({ x: 105, y: 45, text: 'Hi' })
    expect(bounds(text)).toEqual({ x: 100, y: 50, width: 80, height: 40 })
    // Text paints inside its box, plus a pixel for antialiasing.
    expect(paintBounds(text)).toEqual({ x: 99, y: 49, width: 82, height: 42 })
  })

  it('scales its font when resized, never stretching', () => {
    expect(transform(text, { x: 90, y: 40, scaleX: 3, scaleY: 2 })).toMatchObject({
      x: 90,
      y: 40,
      width: 160,
      height: 80,
      fontSize: 40,
    })
  })

  it('stays the right way round when flipped', () => {
    expect(transform(text, { x: 180, y: 90, scaleX: -1, scaleY: -1 })).toMatchObject({
      x: 100,
      y: 50,
      width: 80,
      height: 40,
      fontSize: 20,
    })
    // A zero scale counts as unflipped.
    expect(transform(text, { x: 0, y: 0, scaleX: 0, scaleY: 1 })).toMatchObject({ x: 0, y: 0 })
    expect(transform(text, { x: 0, y: 0, scaleX: 1, scaleY: 0 })).toMatchObject({ x: 0, y: 0 })
  })

  it('keeps the font between its smallest and largest size', () => {
    const small = transform(text, { x: 0, y: 0, scaleX: 0.01, scaleY: 0.01 })
    expect(small).toMatchObject({ fontSize: 8, height: 16 })
    expect(transform(text, { x: 0, y: 0, scaleX: 100, scaleY: 100 })).toMatchObject({
      fontSize: 400,
    })
  })
})
