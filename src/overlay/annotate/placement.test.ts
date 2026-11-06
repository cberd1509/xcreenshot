import { describe, expect, it } from 'vitest'
import { railPosition, toolbarPosition } from './placement'

const bounds = { width: 1000, height: 700 }
const size = { width: 300, height: 40 }

describe('toolbarPosition', () => {
  it('goes below the selection, right-aligned', () => {
    expect(toolbarPosition({ x: 100, y: 100, width: 400, height: 200 }, size, bounds)).toEqual({
      x: 200,
      y: 312,
    })
  })

  it('stays on screen at the left and right edges', () => {
    expect(toolbarPosition({ x: 10, y: 100, width: 50, height: 50 }, size, bounds).x).toBe(0)
    expect(toolbarPosition({ x: 900, y: 100, width: 200, height: 50 }, size, bounds).x).toBe(700)
  })

  it('goes above when there is no room below', () => {
    expect(toolbarPosition({ x: 400, y: 300, width: 400, height: 380 }, size, bounds)).toEqual({
      x: 500,
      y: 248,
    })
  })

  it('goes inside along the bottom when there is room neither below nor above', () => {
    expect(toolbarPosition({ x: 0, y: 0, ...bounds }, size, bounds)).toEqual({ x: 700, y: 660 })
  })

  it('keeps to the top-left on a display smaller than the toolbar', () => {
    const tiny = { width: 100, height: 20 }
    expect(toolbarPosition({ x: 0, y: 0, ...tiny }, size, tiny)).toEqual({ x: 0, y: 0 })
  })
})

describe('railPosition', () => {
  const rail = { width: 48, height: 500 }

  it('goes right of the selection, top-aligned', () => {
    expect(railPosition({ x: 100, y: 100, width: 400, height: 200 }, rail, bounds)).toEqual({
      x: 512,
      y: 100,
    })
  })

  it('flips to the left when there is no room on the right', () => {
    expect(railPosition({ x: 500, y: 0, width: 480, height: 200 }, rail, bounds).x).toBe(440)
  })

  it('goes inside along the right edge when there is room on neither side', () => {
    expect(railPosition({ x: 0, y: 0, ...bounds }, rail, bounds).x).toBe(940)
  })

  it('stays on screen at the bottom', () => {
    expect(railPosition({ x: 100, y: 600, width: 50, height: 50 }, rail, bounds).y).toBe(200)
  })

  it('keeps to the top-left on a display smaller than the rail', () => {
    const tiny = { width: 20, height: 20 }
    expect(railPosition({ x: 0, y: 0, ...tiny }, rail, tiny)).toEqual({ x: 0, y: 0 })
  })
})
