import { describe, expect, it, vi } from 'vitest'
import { control, fieldShortcut, keyLabels, modifier, shortcut, typing } from './keys'

const platform = vi.hoisted(() => ({ mac: true }))
vi.mock('$lib/settings.svelte', () => ({
  get isMac() {
    return platform.mac
  },
}))

const click = (init: MouseEventInit) => new MouseEvent('dblclick', init)
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

describe('modifier', () => {
  it('is ⌘ on macOS and Ctrl elsewhere', () => {
    platform.mac = true
    expect(modifier(click({ metaKey: true }))).toBe(true)
    expect(modifier(click({ ctrlKey: true }))).toBe(false)
    platform.mac = false
    expect(modifier(click({ ctrlKey: true }))).toBe(true)
    expect(modifier(click({ metaKey: true }))).toBe(false)
    platform.mac = true
  })
})

describe('keyLabels', () => {
  it('writes ⌘ and ⇧ on macOS, Ctrl+ and Shift+ elsewhere', () => {
    expect(keyLabels(true)).toEqual({ mod: '⌘', shift: '⇧', key: '⌘' })
    expect(keyLabels(false)).toEqual({ mod: 'Ctrl+', shift: 'Shift+', key: 'Ctrl' })
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

describe('fieldShortcut', () => {
  it('leaves Copy and Save to the capture outside fields', () => {
    expect(fieldShortcut(el('<button></button>'), true)).toBe(false)
    expect(fieldShortcut(null, false)).toBe(false)
  })

  it('gives them to a multi-line field', () => {
    expect(fieldShortcut(el('<textarea></textarea>'), true)).toBe(true)
    expect(fieldShortcut(el('<textarea></textarea>'), false)).toBe(true)
  })

  it('gives them to a number field without reading its selection, which can throw', () => {
    const width = el('<input type="number" value="200">') as HTMLInputElement
    Object.defineProperty(width, 'selectionStart', {
      get: () => {
        throw new DOMException('no selection', 'InvalidStateError')
      },
    })
    expect(fieldShortcut(width, true)).toBe(true)
    expect(fieldShortcut(width, false)).toBe(true)
  })

  it('gives a one-line field only a Copy of the text selected in it', () => {
    const hex = el('<input value="#EF4444">') as HTMLInputElement
    hex.setSelectionRange(7, 7)
    expect(fieldShortcut(hex, true)).toBe(false)
    hex.setSelectionRange(0, 7)
    expect(fieldShortcut(hex, true)).toBe(true)
    expect(fieldShortcut(hex, false)).toBe(false)
  })
})
