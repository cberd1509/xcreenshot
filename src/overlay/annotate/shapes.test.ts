import { describe, expect, it } from 'vitest'
import {
  DEFAULT_STYLE,
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

  it('draws with rect and arrow for now', () => {
    expect([...READY].toSorted()).toEqual(['arrow', 'rect'])
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

  it('draws nothing with a tool that is not ready', () => {
    expect(draw('pen', 'x', { x: 0, y: 0 }, { x: 50, y: 50 }, style)).toBeNull()
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
