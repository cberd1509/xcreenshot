import { describe, expect, it } from 'vitest'
import { release } from './buffer'

describe('release', () => {
  it('empties the buffer and every view of it', () => {
    const buffer = new ArrayBuffer(16)
    const view = new Uint8ClampedArray(buffer)
    release(buffer)
    expect(buffer.byteLength).toBe(0)
    expect(view.length).toBe(0)
  })

  it('does nothing without transfer', () => {
    const buffer = new ArrayBuffer(8)
    // An older WebView.
    Object.defineProperty(buffer, 'transfer', { value: undefined })
    release(buffer)
    expect(buffer.byteLength).toBe(8)
  })

  it('never throws', () => {
    const buffer = new ArrayBuffer(8)
    release(buffer)
    // Already detached: transfer throws.
    expect(() => release(buffer)).not.toThrow()
  })
})
