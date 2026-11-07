import { describe, expect, it } from 'vitest'
import { comboFromEvent, comboProblem, shortcutKeys, spokenKeys, toAccelerator } from './keys'

const key = (code: string, mods: Partial<KeyboardEvent> = {}) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
})

describe('spokenKeys', () => {
  it('names the keys in words', () => {
    expect(spokenKeys('Control+Shift+Super+S', true)).toBe('Control Shift Command S')
    expect(spokenKeys('Alt+CmdOrCtrl+K', true)).toBe('Option Command K')
    expect(spokenKeys('Alt+Super+F5', false)).toBe('Alt Win F5')
    expect(spokenKeys('CmdOrCtrl+K', false)).toBe('Control K')
  })
})

describe('shortcutKeys', () => {
  it('shows Mac key symbols on macOS', () => {
    expect(shortcutKeys('Control+Shift+Super+S', true)).toEqual(['⌃', '⇧', '⌘', 'S'])
    expect(shortcutKeys('Alt+CmdOrCtrl+K', true)).toEqual(['⌥', '⌘', 'K'])
  })

  it('shows Windows key names on Windows', () => {
    expect(shortcutKeys('Control+Super+S', false)).toEqual(['Ctrl', 'Win', 'S'])
    expect(shortcutKeys('CmdOrCtrl+K', false)).toEqual(['Ctrl', 'K'])
    expect(shortcutKeys('Control+Shift+PrintScreen', false)).toEqual(['Ctrl', 'Shift', 'PrtSc'])
  })

  it('shows punctuation and arrows as characters', () => {
    expect(shortcutKeys('Alt+ArrowUp', true)).toEqual(['⌥', '↑'])
    expect(shortcutKeys('Control+Minus', false)).toEqual(['Ctrl', '-'])
  })

  it('has no keys before the shortcut is known', () => {
    expect(shortcutKeys('')).toEqual([])
  })
})

describe('recording a shortcut', () => {
  it('turns a key press into an accelerator, modifiers first', () => {
    const all = { ctrlKey: true, altKey: true, shiftKey: true, metaKey: true }
    expect(toAccelerator(comboFromEvent(key('KeyS', all)))).toBe('Control+Alt+Shift+Super+S')
    expect(toAccelerator(comboFromEvent(key('Digit4', { metaKey: true })))).toBe('Super+4')
    expect(toAccelerator(comboFromEvent(key('F5', { altKey: true })))).toBe('Alt+F5')
    expect(toAccelerator(comboFromEvent(key('ArrowUp', { shiftKey: true })))).toBe('Shift+ArrowUp')
    expect(toAccelerator(comboFromEvent(key('Numpad1', { ctrlKey: true })))).toBe('Control+Numpad1')
  })

  it('has no main key while only modifiers are held', () => {
    for (const code of ['ControlLeft', 'ShiftRight', 'AltLeft', 'MetaRight', 'OSLeft', '']) {
      expect(comboFromEvent(key(code, { ctrlKey: true }))).toEqual({
        modifiers: ['Control'],
        key: null,
      })
    }
    expect(toAccelerator({ modifiers: ['Control', 'Shift'], key: null })).toBe('Control+Shift')
  })

  it('needs ⌘/Win, Control or Alt; Shift alone is not enough', () => {
    expect(comboProblem([], 'S', false)).toBe('Add ⌘, ⌃ or ⌥.')
    expect(comboProblem([], 'S', true)).toBe('Add Ctrl, Alt or Win.')
    expect(comboProblem(['Shift'], 'S', false)).toBe('Add ⌘, ⌃ or ⌥.')
    expect(comboProblem(['Shift'], 'F5', true)).toBe('Add Ctrl, Alt or Win.')
    for (const m of ['Control', 'Alt', 'Super']) {
      expect(comboProblem([m, 'Shift'], 'S', false)).toBeNull()
    }
  })

  it('allows bare PrintScreen on Windows only', () => {
    expect(comboProblem([], 'PrintScreen', true)).toBeNull()
    expect(comboProblem([], 'PrintScreen', false)).not.toBeNull()
  })
})
