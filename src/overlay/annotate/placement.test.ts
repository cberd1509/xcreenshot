import { describe, expect, it } from 'vitest'
import { toolbarPosition } from './placement'

const bounds = { width: 1000, height: 700 }
const size = { width: 300, height: 40 }

describe('toolbarPosition', () => {
  it('goes below the selection, right-aligned', () => {
    expect(toolbarPosition({ x: 100, y: 100, width: 400, height: 200 }, size, bounds)).toEqual({
      x: 200,
      y: 306,
    })
  })

  it('stays on screen at the left and right edges', () => {
    expect(toolbarPosition({ x: 10, y: 100, width: 50, height: 50 }, size, bounds).x).toBe(0)
    expect(toolbarPosition({ x: 900, y: 100, width: 200, height: 50 }, size, bounds).x).toBe(700)
  })

  it('goes above when there is no room below', () => {
    expect(toolbarPosition({ x: 400, y: 300, width: 400, height: 380 }, size, bounds)).toEqual({
      x: 500,
      y: 254,
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
