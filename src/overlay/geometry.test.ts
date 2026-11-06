import { describe, expect, it } from 'vitest'
import {
  CURSORS,
  HANDLES,
  badgePosition,
  contains,
  fromPoints,
  hexAt,
  isEmpty,
  lockedSize,
  loupePosition,
  move,
  nudge,
  resize,
  resizeTo,
  toLogical,
  toPhysical,
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

  it('nudges with the arrow keys only', () => {
    const r = rect(10, 10, 20, 20)
    expect(nudge(r, 'ArrowLeft', 1, display)).toEqual(rect(9, 10, 20, 20))
    expect(nudge(r, 'ArrowRight', 10, display)).toEqual(rect(20, 10, 20, 20))
    expect(nudge(r, 'ArrowUp', 1, display)).toEqual(rect(10, 9, 20, 20))
    expect(nudge(r, 'ArrowDown', 10, display)).toEqual(rect(10, 20, 20, 20))
    expect(nudge(rect(0, 0, 5, 5), 'ArrowLeft', 10, display)).toEqual(rect(0, 0, 5, 5))
    expect(nudge(r, 'a', 1, display)).toBeNull()
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

describe('logical and physical', () => {
  const frame = { width: 200, height: 160 }

  it('scales to whole pixels', () => {
    expect(toPhysical(rect(10, 20, 30, 40), retina, frame)).toEqual(rect(20, 40, 60, 80))
    // Half points are whole pixels at 2x.
    expect(toPhysical(rect(0.5, 0.5, 1, 1), retina, frame)).toEqual(rect(1, 1, 2, 2))
    // 1.5x: edges round to the nearest pixel, like Rust's `to_pixels`.
    expect(toPhysical(rect(1, 1, 2, 2), { x: 1.5, y: 1.5 }, frame)).toEqual(rect(2, 2, 3, 3))
  })

  it('clamps to the frame', () => {
    expect(toPhysical(rect(90, 70, 50, 50), retina, frame)).toEqual(rect(180, 140, 20, 20))
  })

  it('round-trips whole pixels', () => {
    const p = rect(21, 41, 61, 81)
    expect(toPhysical(toLogical(p, retina), retina, frame)).toEqual(p)
    expect(toLogical(p, { x: 2, y: 1 })).toEqual(rect(10.5, 41, 30.5, 81))
  })
})

describe('exact size', () => {
  const frame = { width: 200, height: 160 }

  it('keeps the ratio for a locked edit', () => {
    expect(lockedSize('width', 300, 1.5)).toEqual({ width: 300, height: 200 })
    expect(lockedSize('height', 200, 1.5)).toEqual({ width: 300, height: 200 })
    expect(lockedSize('width', 1, 4)).toEqual({ width: 1, height: 1 })
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

describe('hexAt', () => {
  const frame = { width: 2, height: 2 }
  const pixels = new Uint8ClampedArray([
    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 1, 2, 171, 255,
  ])

  it('reads the pixel color', () => {
    expect(hexAt(pixels, frame, 0, 0)).toBe('#FF0000')
    expect(hexAt(pixels, frame, 1, 0)).toBe('#00FF00')
    expect(hexAt(pixels, frame, 0, 1)).toBe('#0000FF')
    expect(hexAt(pixels, frame, 1, 1)).toBe('#0102AB')
  })

  it('is null outside the frame', () => {
    for (const [x, y] of [
      [-1, 0],
      [0, -1],
      [2, 0],
      [0, 2],
    ]) {
      expect(hexAt(pixels, frame, x, y)).toBeNull()
    }
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
