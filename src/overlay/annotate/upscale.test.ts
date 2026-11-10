import { describe, expect, it } from 'vitest'
import { MAX_SIDE, sizeOptions } from './upscale'

const enabled = (width: number, height: number) =>
  sizeOptions({ width, height }).map((o) => o.enabled)

describe('sizeOptions', () => {
  it('lists 2×, 3× and 4× with the size each gives', () => {
    expect(sizeOptions({ width: 1260, height: 1064 })).toEqual([
      { factor: 2, width: 2520, height: 2128, enabled: true },
      { factor: 3, width: 3780, height: 3192, enabled: true },
      { factor: 4, width: 5040, height: 4256, enabled: true },
    ])
  })

  it('disables a size past the cap on either side, as Rust does', () => {
    expect(MAX_SIDE).toBe(8192)
    // 2048 × 4 is exactly the cap.
    expect(enabled(2048, 10)).toEqual([true, true, true])
    expect(enabled(2049, 10)).toEqual([true, true, false])
    expect(enabled(10, 2731)).toEqual([true, false, false])
    // A full Retina screen allows only 2×.
    expect(enabled(3600, 2338)).toEqual([true, false, false])
    expect(enabled(4097, 1)).toEqual([false, false, false])
  })
})
