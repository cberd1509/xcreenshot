import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { HEADER_LEN, readFrame } from './frame'
import { FakeImageData, packFrame } from '../test/overlay'

describe('readFrame', () => {
  beforeEach(() => {
    vi.stubGlobal('ImageData', FakeImageData)
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reads the size header and wraps the pixels without copying', () => {
    const buffer = packFrame(3, 2)
    const frame = readFrame(buffer)!
    expect(frame.width).toBe(3)
    expect(frame.height).toBe(2)
    expect(frame.data.buffer).toBe(buffer)
    expect(frame.data.byteOffset).toBe(HEADER_LEN)
    expect(frame.data.length).toBe(3 * 2 * 4)
    expect(Array.from(frame.data.slice(0, 4))).toEqual([0, 1, 2, 3])
  })

  it('returns null without a frame', () => {
    expect(readFrame(new ArrayBuffer(0))).toBeNull()
    expect(readFrame(new ArrayBuffer(4))).toBeNull()
    expect(readFrame(packFrame(0, 2))).toBeNull()
    expect(readFrame(packFrame(2, 0))).toBeNull()
  })

  it('returns null when the size does not match the pixels', () => {
    const full = packFrame(2, 2)
    expect(readFrame(full.slice(0, full.byteLength - 1))).toBeNull()
  })
})
