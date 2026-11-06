import { describe, expect, it } from 'vitest'
import { BLOCK, average, blockSize, blocks, paint, spans, type Shrunk } from './blocks'

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
    expect(average(f, { x: 0, y: 0, width: 32, height: 16 })).toEqual([1, 0, 0])
    expect(average(f, { x: 32, y: 48, width: 16, height: 16 })).toEqual([2, 3, 0])
  })

  it('uses at least one px, inside the frame', () => {
    const f = frame(8, 8)
    expect(average(f, { x: 20, y: 20, width: 2, height: 2 })).toEqual([1, 1, 0])
    expect(average(f, { x: 500, y: 500, width: 16, height: 16 })).toEqual([7, 7, 0])
  })
})

describe('paint', () => {
  it('paints the shrunk px whose centers are in the rect', () => {
    const f = frame(8, 8)
    paint(f, { x: 0, y: 0, width: 32, height: 16 }, [9, 9, 9])
    expect(Array.from(f.data.subarray(0, 12))).toEqual([9, 9, 9, 255, 9, 9, 9, 255, 2, 0, 0, 255])
    // Nothing past the frame.
    paint(f, { x: 500, y: 0, width: 16, height: 16 }, [9, 9, 9])
    expect(f.data[7 * 4]).toBe(7)
  })
})

describe('blocks', () => {
  it('cuts the rect from its corner, relative to the crop as Rust gets it', () => {
    // A crop at (5, 3), not on a 16 px grid: blocks start at the blur, not at the frame's 0.
    const b = blocks({ x: 11, y: 0, width: 40, height: 20 }, { x: 5, y: 3 }, frame(8, 8))
    expect(b).toEqual([
      { x: 16, y: 3, width: 16, height: 20, color: 'rgb(1, 1, 0)' },
      { x: 32, y: 3, width: 24, height: 20, color: 'rgb(3, 1, 0)' },
    ])
    const starts = spans(11, 40, 16).map(([x0]) => 5 + x0)
    expect(b.map((k) => k.x)).toEqual(starts)
  })

  it('paints its blocks, so a later blur over it averages them, as in Rust', () => {
    const f = frame(8, 8)
    const later = { x: 0, y: 0, width: 16, height: 16 }
    expect(blocks(later, { x: 0, y: 0 }, frame(8, 8))[0].color).toBe('rgb(0, 0, 0)')
    // A first blur, one block over px 0 and 1, takes both to their average.
    blocks({ x: 0, y: 0, width: 31, height: 16 }, { x: 0, y: 0 }, f)
    expect(blocks(later, { x: 0, y: 0 }, f)[0].color).toBe('rgb(1, 0, 0)')
  })
})
