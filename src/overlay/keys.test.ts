import { describe, expect, it, vi } from 'vitest'
import { shortcut } from './keys'

const platform = vi.hoisted(() => ({ mac: true }))
vi.mock('$lib/settings.svelte', () => ({
  get isMac() {
    return platform.mac
  },
}))

const key = (k: string, init: KeyboardEventInit = {}) =>
  new KeyboardEvent('keydown', { key: k, ...init })

describe('shortcut', () => {
  it('takes ⌘ on macOS and Ctrl elsewhere', () => {
    platform.mac = true
    expect(shortcut(key('z', { metaKey: true }), 'z')).toBe(true)
    expect(shortcut(key('z', { ctrlKey: true }), 'z')).toBe(false)
    platform.mac = false
    expect(shortcut(key('z', { ctrlKey: true }), 'z')).toBe(true)
    expect(shortcut(key('z', { metaKey: true }), 'z')).toBe(false)
  })

  it('needs Shift only when asked, and no Alt', () => {
    platform.mac = true
    expect(shortcut(key('Z', { metaKey: true, shiftKey: true }), 'z', true)).toBe(true)
    expect(shortcut(key('z', { metaKey: true }), 'z', true)).toBe(false)
    expect(shortcut(key('Z', { metaKey: true, shiftKey: true }), 'z')).toBe(false)
    expect(shortcut(key('z', { metaKey: true, altKey: true }), 'z')).toBe(false)
  })

  it('follows the physical key on a non-Latin layout', () => {
    platform.mac = true
    expect(shortcut(key('я', { metaKey: true, code: 'KeyZ' }), 'z')).toBe(true)
    expect(shortcut(key('я', { metaKey: true, code: 'KeyX' }), 'z')).toBe(false)
  })
})
