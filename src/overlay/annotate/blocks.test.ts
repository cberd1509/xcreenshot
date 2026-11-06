import { describe, expect, it } from 'vitest'
import { BLOCK, average, blockSize, blocks, spans, type Shrunk } from './blocks'

// The same values as `block_size_grows_with_the_rect` and `spans_merge_a_short_remainder` in
// src-tauri/src/output/compose.rs: the preview must show the blocks Rust paints.
describe('blocks, as Rust paints them', () => {
  it('sizes blocks like Rust', () => {
    expect(BLOCK).toBe(16)
    const sizes: [number, number, number][] = [
      [1, 1, 16],
      [10, 300, 16],
      [200, 120, 16],
      [256, 256, 32],
      [800, 400, 50],
      [3024, 1964, 245],
    ]
    for (const [w, h, block] of sizes) expect(blockSize(w, h)).toBe(block)
  })

  it('cuts spans like Rust, a short remainder joining the last block', () => {
    expect(spans(0, 33, 16)).toEqual([
      [0, 16],
      [16, 33],
    ])
    expect(spans(5, 1, 16)).toEqual([[5, 6]])
    expect(spans(0, 32, 16)).toEqual([
      [0, 16],
      [16, 32],
    ])
  })
})

/** A shrunk frame of `width × height` px, 16 frame px each; px (x, y) is (x, y, 0). */
function frame(width: number, height: number): Shrunk {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) data.set([x, y, 0, 255], (y * width + x) * 4)
  }
  return { data, width, height, factor: { x: 16, y: 16 } }
}

describe('average', () => {
  it('averages the shrunk px under a rect', () => {
    const f = frame(8, 8)
    expect(average(f, { x: 0, y: 0, width: 32, height: 16 })).toBe('rgb(1, 0, 0)')
    expect(average(f, { x: 32, y: 48, width: 16, height: 16 })).toBe('rgb(2, 3, 0)')
  })

  it('uses at least one px, inside the frame', () => {
    const f = frame(8, 8)
    expect(average(f, { x: 20, y: 20, width: 2, height: 2 })).toBe('rgb(1, 1, 0)')
    expect(average(f, { x: 500, y: 500, width: 16, height: 16 })).toBe('rgb(7, 7, 0)')
  })
})

describe('blocks', () => {
  it('cuts the rect from its corner into colored blocks', () => {
    const b = blocks({ x: 16, y: 0, width: 40, height: 20 }, frame(8, 8))
    expect(b).toEqual([
      { x: 16, y: 0, width: 16, height: 20, color: 'rgb(1, 1, 0)' },
      { x: 32, y: 0, width: 24, height: 20, color: 'rgb(3, 1, 0)' },
    ])
  })
})
