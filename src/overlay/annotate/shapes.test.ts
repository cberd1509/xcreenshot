import { describe, expect, it } from 'vitest'
import {
  DEFAULT_STYLE,
  READY,
  TOOLS,
  blurRects,
  clampTo,
  draw,
  origin,
  toolForKey,
  transform,
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
  it('clamps a point to the selection', () => {
    expect(clampTo({ x: -5, y: 500 }, { width: 100, height: 50 })).toEqual({ x: 0, y: 50 })
    expect(clampTo({ x: 5, y: 6 }, { width: 100, height: 50 })).toEqual({ x: 5, y: 6 })
  })

  it('puts a rect node at its corner and an arrow node at its start', () => {
    expect(origin(rect)).toMatchObject({ x: 10, y: 20 })
    expect(origin(arrow)).toEqual({ x: 10, y: 20 })
  })

  it('moves a rect and an arrow', () => {
    const t = { x: 15, y: 25, scaleX: 1, scaleY: 1 }
    expect(transform(rect, t)).toMatchObject({ x: 15, y: 25, width: 30, height: 40 })
    expect(transform(arrow, t)).toMatchObject({ points: [15, 25, 45, 65] })
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

  it('round-trips through JSON', () => {
    const shapes = [rect, arrow]
    expect(JSON.parse(JSON.stringify(shapes))).toEqual(shapes)
  })
})

describe('blurRects', () => {
  it('scales blur rects to physical px, rounding outwards', () => {
    expect(blurRects([blur(1.2, 2.6, 10, 10)], { width: 100, height: 100 }, 2)).toEqual([
      { x: 2, y: 5, width: 21, height: 21 },
    ])
  })

  it('clips to the image and drops rects outside it', () => {
    const size = { width: 50, height: 40 }
    expect(blurRects([blur(20, 10, 100, 100), blur(60, 0, 5, 5)], size, 1)).toEqual([
      { x: 20, y: 10, width: 30, height: 30 },
    ])
  })

  it('skips other shapes', () => {
    expect(blurRects([rect, arrow], { width: 100, height: 100 }, 1)).toEqual([])
  })
})
