import { afterEach, describe, expect, it, vi } from 'vitest'
import { KEY, recall, remember } from './memory'
import { DEFAULT_STYLE } from './shapes'

describe('memory', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('starts with no tool and the default style', () => {
    expect(recall()).toEqual({ tool: null, style: DEFAULT_STYLE })
  })

  it('remembers the tool, color and size', () => {
    remember({ tool: 'pen', style: { color: '#12AB9F', size: 8 } })
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual({
      tool: 'pen',
      color: '#12AB9F',
      size: 8,
    })
    expect(recall()).toEqual({ tool: 'pen', style: { color: '#12AB9F', size: 8 } })
  })

  it('falls back to the defaults for anything wrong or missing', () => {
    localStorage.setItem(KEY, JSON.stringify({ tool: 'laser', color: 'red', size: 5 }))
    expect(recall()).toEqual({ tool: null, style: DEFAULT_STYLE })
    localStorage.setItem(KEY, JSON.stringify({ tool: 'blur' }))
    expect(recall()).toEqual({ tool: 'blur', style: DEFAULT_STYLE })
    localStorage.setItem(KEY, JSON.stringify({ tool: 'text' }))
    expect(recall().tool).toBe('text')
    for (const bad of ['{', 'null', '5']) {
      localStorage.setItem(KEY, bad)
      expect(recall()).toEqual({ tool: null, style: DEFAULT_STYLE })
    }
  })

  it('does without storage when it is full or blocked', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('full')
    })
    expect(() => remember({ tool: 'pen', style: DEFAULT_STYLE })).not.toThrow()
    // Stubbed first, so `unstubAllGlobals` puts the real one back afterwards.
    vi.stubGlobal('localStorage', undefined)
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      get() {
        throw new Error('blocked')
      },
    })
    expect(recall()).toEqual({ tool: null, style: DEFAULT_STYLE })
    expect(() => remember({ tool: 'pen', style: DEFAULT_STYLE })).not.toThrow()
  })
})
