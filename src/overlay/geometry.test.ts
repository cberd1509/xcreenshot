import { describe, expect, it } from 'vitest'
import {
  CURSORS,
  HANDLES,
  badgePosition,
  contains,
  fromPoints,
  hex,
  intersect,
  isEmpty,
  lockedSize,
  loupePosition,
  move,
  nudge,
  resize,
  resizeTo,
  span,
  toPhysical,
  union,
  type Rect,
} from './geometry'

const display = { width: 100, height: 80 }
const rect = (x: number, y: number, width: number, height: number): Rect => ({
  x,
  y,
  width,
  height,
})
const retina = { x: 2, y: 2 }

describe('span, union and intersect', () => {
  it('spans two points in any order, negative ones included', () => {
    expect(span({ x: 10, y: -20 }, { x: -30, y: 40 })).toEqual(rect(-30, -20, 40, 60))
  })

  it('holds both rects', () => {
    expect(union(rect(0, 0, 10, 10), rect(-5, 20, 10, 10))).toEqual(rect(-5, 0, 15, 30))
  })

  it('overlaps two rects, or says they do not', () => {
    expect(intersect(rect(0, 0, 10, 10), rect(5, -5, 10, 10))).toEqual(rect(5, 0, 5, 5))
    expect(intersect(rect(0, 0, 10, 10), rect(10, 0, 10, 10))).toBeNull()
  })
})

describe('fromPoints', () => {
  it('normalizes a drag in any direction', () => {
    const r = rect(10, 20, 30, 40)
    expect(fromPoints({ x: 10, y: 20 }, { x: 40, y: 60 }, display)).toEqual(r)
    expect(fromPoints({ x: 40, y: 60 }, { x: 10, y: 20 }, display)).toEqual(r)
    expect(fromPoints({ x: 40, y: 20 }, { x: 10, y: 60 }, display)).toEqual(r)
  })

  it('clamps to the display', () => {
    expect(fromPoints({ x: -5, y: -5 }, { x: 500, y: 500 }, display)).toEqual(rect(0, 0, 100, 80))
    expect(fromPoints({ x: 120, y: 10 }, { x: 130, y: 20 }, display)).toEqual(rect(100, 10, 0, 10))
  })
})

describe('isEmpty and contains', () => {
  it('knows an empty rect', () => {
    expect(isEmpty(rect(1, 1, 0, 5))).toBe(true)
    expect(isEmpty(rect(1, 1, 5, 0))).toBe(true)
    expect(isEmpty(rect(1, 1, 1, 1))).toBe(false)
  })

  it('includes the edges', () => {
    const r = rect(10, 10, 10, 10)
    expect(contains(r, { x: 10, y: 20 })).toBe(true)
    expect(contains(r, { x: 15, y: 15 })).toBe(true)
    expect(contains(r, { x: 9.9, y: 15 })).toBe(false)
    expect(contains(r, { x: 15, y: 20.1 })).toBe(false)
  })
})

describe('move and nudge', () => {
  it('moves inside the display without changing the size', () => {
    const r = rect(10, 10, 20, 20)
    expect(move(r, 5, -3, display)).toEqual(rect(15, 7, 20, 20))
    expect(move(r, -50, -50, display)).toEqual(rect(0, 0, 20, 20))
    expect(move(r, 500, 500, display)).toEqual(rect(80, 60, 20, 20))
  })

  it('shrinks a rect bigger than the display', () => {
    expect(move(rect(0, 0, 200, 200), 0, 0, display)).toEqual(rect(0, 0, 100, 80))
  })

  it('nudges by physical pixels with the arrow keys only', () => {
    const r = rect(10, 10, 20, 20)
    expect(nudge(r, 'ArrowLeft', 1, retina, display)).toEqual(rect(9.5, 10, 20, 20))
    expect(nudge(r, 'ArrowRight', 10, retina, display)).toEqual(rect(15, 10, 20, 20))
    expect(nudge(r, 'ArrowUp', 1, { x: 1, y: 1 }, display)).toEqual(rect(10, 9, 20, 20))
    expect(nudge(r, 'ArrowDown', 10, retina, display)).toEqual(rect(10, 15, 20, 20))
    expect(nudge(rect(0, 0, 5, 5), 'ArrowLeft', 10, retina, display)).toEqual(rect(0, 0, 5, 5))
    expect(nudge(r, 'a', 1, retina, display)).toBeNull()
  })
})

describe('resize', () => {
  const r = rect(10, 10, 20, 20)

  it('moves only the edges of the handle', () => {
    const cases: [(typeof HANDLES)[number], Rect][] = [
      ['nw', rect(15, 13, 15, 17)],
      ['n', rect(10, 13, 20, 17)],
      ['ne', rect(10, 13, 25, 17)],
      ['e', rect(10, 10, 25, 20)],
      ['se', rect(10, 10, 25, 23)],
      ['s', rect(10, 10, 20, 23)],
      ['sw', rect(15, 10, 15, 23)],
      ['w', rect(15, 10, 15, 20)],
    ]
    expect(cases.map(([h]) => h)).toEqual(HANDLES)
    for (const [handle, expected] of cases) {
      expect({ handle, rect: resize(r, handle, 5, 3, display) }).toEqual({ handle, rect: expected })
    }
  })

  it('flips past the opposite edge', () => {
    expect(resize(r, 'e', -30, 0, display)).toEqual(rect(0, 10, 10, 20))
    expect(resize(r, 'n', 0, 25, display)).toEqual(rect(10, 30, 20, 5))
  })

  it('clamps to the display', () => {
    expect(resize(r, 'se', 500, 500, display)).toEqual(rect(10, 10, 90, 70))
    expect(resize(r, 'nw', -500, -500, display)).toEqual(rect(0, 0, 30, 30))
  })

  it('has a cursor for every handle and for moving', () => {
    for (const h of HANDLES) expect(CURSORS[h]).toMatch(/-resize$/)
    expect(CURSORS.move).toBe('move')
  })
})

const frameAt = (s: number) => ({ width: Math.round(100 * s), height: Math.round(80 * s) })
const at = (s: number) => ({ x: s, y: s })

// Same cases as `maps_points_to_pixels_at_each_scale` and `clamps_to_the_display` in
// src-tauri/src/capture/geometry.rs: a 100 x 80 point display, so both round the same way.
describe('logical to physical (shared with Rust)', () => {
  it('maps points to pixels at each scale', () => {
    const cases: [number, Rect, Rect][] = [
      [1, rect(10, 20, 30, 40), rect(10, 20, 30, 40)],
      [2, rect(10, 20, 30, 40), rect(20, 40, 60, 80)],
      [2, rect(0.5, 0.5, 1, 1), rect(1, 1, 2, 2)], // half points are whole pixels
      [1.5, rect(10, 20, 30, 40), rect(15, 30, 45, 60)],
      [1.5, rect(1, 1, 2, 2), rect(2, 2, 3, 3)], // 1.5 -> 2, 4.5 -> 5
    ]
    for (const [s, r, expected] of cases) {
      expect({ s, r, p: toPhysical(r, at(s), frameAt(s)) }).toEqual({ s, r, p: expected })
    }
  })

  it('clamps to the display', () => {
    const cases: [Rect, Rect][] = [
      [rect(-10, -10, 30, 30), rect(0, 0, 40, 40)],
      [rect(90, 70, 50, 50), rect(180, 140, 20, 20)],
      [rect(-1e9, -1e9, 2e9, 2e9), rect(0, 0, 200, 160)],
    ]
    for (const [r, expected] of cases) {
      expect({ r, p: toPhysical(r, retina, frameAt(2)) }).toEqual({ r, p: expected })
    }
  })
})

describe('exact size', () => {
  const frame = { width: 200, height: 160 }

  it('keeps the ratio for a locked edit', () => {
    expect(lockedSize('width', 300, 1.5)).toEqual({ width: 300, height: 200 })
    expect(lockedSize('height', 200, 1.5)).toEqual({ width: 300, height: 200 })
    expect(lockedSize('width', 1, 4)).toEqual({ width: 1, height: 1 })
    for (const bad of [0, Number.NaN, Infinity, -2]) {
      expect(lockedSize('width', 30, bad)).toEqual({ width: 30, height: 30 })
    }
  })

  it('resizes from the top-left in physical px', () => {
    const r = rect(10, 10, 20, 20)
    expect(resizeTo(r, { width: 101, height: 50 }, retina, frame)).toEqual(rect(10, 10, 50.5, 25))
  })

  it('clamps to the frame', () => {
    const r = rect(50, 40, 10, 10)
    expect(resizeTo(r, { width: 999, height: 0 }, retina, frame)).toEqual(rect(50, 40, 50, 0.5))
  })

  it('keeps the ratio when clamping a locked size', () => {
    const r = rect(50, 40, 10, 10)
    // Room for 100 x 80 px; 400 x 200 scales down to 100 x 50.
    expect(resizeTo(r, { width: 400, height: 200 }, retina, frame, true)).toEqual(
      rect(50, 40, 50, 25),
    )
    expect(resizeTo(r, { width: 40, height: 20 }, retina, frame, true)).toEqual(
      rect(50, 40, 20, 10),
    )
  })
})

describe('hex', () => {
  it('formats the first pixel', () => {
    expect(hex([255, 0, 0, 255])).toBe('#FF0000')
    expect(hex(new Uint8ClampedArray([1, 2, 171, 0, 9, 9, 9, 9]))).toBe('#0102AB')
  })
})

describe('placement', () => {
  const size = { width: 40, height: 10 }

  it('puts the badge above, then below, then inside', () => {
    expect(badgePosition(rect(10, 30, 20, 20), size, display)).toEqual({ x: 10, y: 14 })
    expect(badgePosition(rect(10, 5, 20, 20), size, display)).toEqual({ x: 10, y: 31 })
    expect(badgePosition(rect(10, 0, 20, 80), size, display)).toEqual({ x: 10, y: 6 })
  })

  it('keeps the badge on the display', () => {
    expect(badgePosition(rect(90, 30, 10, 10), size, display).x).toBe(60)
    expect(badgePosition(rect(0, 30, 10, 10), { width: 200, height: 10 }, display).x).toBe(0)
  })

  it('puts the loupe down-right of the cursor, flipped near the edges', () => {
    const loupe = { width: 30, height: 30 }
    expect(loupePosition({ x: 10, y: 10 }, loupe, display)).toEqual({ x: 26, y: 26 })
    expect(loupePosition({ x: 90, y: 70 }, loupe, display)).toEqual({ x: 44, y: 24 })
    expect(loupePosition({ x: 10, y: 10 }, { width: 200, height: 200 }, display)).toEqual({
      x: 0,
      y: 0,
    })
  })
})
