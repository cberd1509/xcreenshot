import { describe, expect, it } from 'vitest'
import { toolbarPosition } from './placement'

const bounds = { width: 1000, height: 700 }
const size = { width: 300, height: 40 }
// The badge sits above the selection's left edge.
const badgeAbove = (r: { x: number; y: number }) => ({ x: r.x, y: r.y - 30, width: 80, height: 24 })

describe('toolbarPosition', () => {
  it('goes below the selection, right-aligned', () => {
    const r = { x: 100, y: 100, width: 400, height: 200 }
    expect(toolbarPosition(r, size, bounds, badgeAbove(r))).toEqual({ x: 200, y: 306 })
  })

  it('stays on screen when the selection is narrow and near the left edge', () => {
    const r = { x: 10, y: 100, width: 50, height: 50 }
    expect(toolbarPosition(r, size, bounds, badgeAbove(r)).x).toBe(0)
  })

  it('stays on screen near the right edge', () => {
    const r = { x: 900, y: 100, width: 200, height: 50 }
    expect(toolbarPosition(r, size, bounds, badgeAbove(r)).x).toBe(700)
  })

  it('flips above when there is no room below', () => {
    const r = { x: 400, y: 300, width: 400, height: 380 }
    // The badge is inside the selection here, out of the way.
    const badge = { x: 400, y: 306, width: 80, height: 24 }
    expect(toolbarPosition(r, size, bounds, badge)).toEqual({ x: 500, y: 254 })
  })

  it('goes inside along the bottom for a full-screen selection', () => {
    const r = { x: 0, y: 0, ...bounds }
    const badge = { x: 0, y: 6, width: 80, height: 24 }
    expect(toolbarPosition(r, size, bounds, badge)).toEqual({ x: 700, y: 654 })
  })

  it('moves aside to keep clear of the badge', () => {
    // A narrow selection: the right-aligned spot below would cover the badge.
    const r = { x: 300, y: 0, width: 100, height: 100 }
    const badge = { x: 300, y: 106, width: 80, height: 24 }
    expect(toolbarPosition(r, size, bounds, badge)).toEqual({ x: 386, y: 106 })
  })

  it('moves to the badge’s left when its right side has no room', () => {
    const r = { x: 900, y: 0, width: 100, height: 100 }
    const badge = { x: 900, y: 106, width: 80, height: 24 }
    expect(toolbarPosition(r, size, bounds, badge)).toEqual({ x: 594, y: 106 })
  })

  it('covers the badge when there is no other room', () => {
    const tiny = { width: 300, height: 40 }
    const r = { x: 0, y: 0, width: 300, height: 40 }
    const badge = { x: 0, y: 0, width: 300, height: 40 }
    expect(toolbarPosition(r, size, tiny, badge)).toEqual({ x: 0, y: 0 })
  })
})
