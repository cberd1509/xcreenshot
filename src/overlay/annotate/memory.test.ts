import { afterEach, describe, expect, it, vi } from 'vitest'
import { KEYS, recall, remember } from './memory'
import { DEFAULT_STYLE } from './shapes'

describe('memory', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('starts with the default style', () => {
    expect(recall()).toEqual(DEFAULT_STYLE)
  })

  it('remembers the color and size', () => {
    remember('color', '#12AB9F')
    remember('size', 8)
    expect(localStorage.getItem(KEYS.color)).toBe('#12AB9F')
    expect(recall()).toEqual({ color: '#12AB9F', size: 8 })
  })

  it('keeps each on its own, so one window never overwrites what another chose', () => {
    remember('color', '#007AFF')
    // Another window changes only the size; the color it never touched stays.
    remember('size', 2)
    expect(recall()).toEqual({ color: '#007AFF', size: 2 })
  })

  it('falls back to the defaults for anything wrong or missing', () => {
    localStorage.setItem(KEYS.color, 'red')
    localStorage.setItem(KEYS.size, '5')
    expect(recall()).toEqual(DEFAULT_STYLE)
  })

  it('does without storage when it is full or blocked', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('full')
    })
    expect(() => remember('size', 8)).not.toThrow()
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(recall()).toEqual(DEFAULT_STYLE)
    // Stubbed first, so `unstubAllGlobals` puts the real one back afterwards.
    vi.stubGlobal('localStorage', undefined)
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      get() {
        throw new Error('blocked')
      },
    })
    expect(recall()).toEqual(DEFAULT_STYLE)
    expect(() => remember('size', 8)).not.toThrow()
  })
})
