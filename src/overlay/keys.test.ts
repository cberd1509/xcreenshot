import { describe, expect, it, vi } from 'vitest'
import { control, keyLabels, shortcut, typing } from './keys'

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

describe('keyLabels', () => {
  it('writes ⌘ and ⇧ on macOS, Ctrl+ and Shift+ elsewhere', () => {
    expect(keyLabels(true)).toEqual({ mod: '⌘', shift: '⇧' })
    expect(keyLabels(false)).toEqual({ mod: 'Ctrl+', shift: 'Shift+' })
    platform.mac = true
    expect(keyLabels().mod).toBe('⌘')
  })
})

/** The element `html` makes. */
function el(html: string) {
  const box = document.createElement('div')
  box.innerHTML = html
  return box.firstElementChild!
}

describe('typing and control', () => {
  it('tells fields from controls', () => {
    expect(typing(el('<input>'))).toBe(true)
    expect(typing(el('<textarea></textarea>'))).toBe(true)
    expect(typing(el('<button></button>'))).toBe(false)
    expect(typing(null)).toBe(false)
    expect(control(el('<button></button>'))).toBe(true)
    expect(control(el('<div role="menuitemcheckbox"></div>'))).toBe(true)
    expect(control(el('<div></div>'))).toBe(false)
    expect(control(null)).toBe(false)
  })
})
