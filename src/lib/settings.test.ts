import { describe, expect, it } from 'vitest'
import { defaultSettings, setSetting, settings, shortcutKeys } from './settings.svelte'

describe('settings on macOS', () => {
  it('defaults to ⌃⇧⌘S and shows Mac key symbols', () => {
    const { shortcut } = defaultSettings(true)
    expect(shortcut).toBe('Control+Shift+Super+S')
    expect(shortcutKeys(shortcut, true)).toEqual(['⌃', '⇧', '⌘', 'S'])
    expect(shortcutKeys('Alt+CmdOrCtrl+K', true)).toEqual(['⌥', '⌘', 'K'])
  })
})

describe('settings on Windows', () => {
  it('defaults to PrintScreen and shows Windows key names', () => {
    expect(defaultSettings(false).shortcut).toBe('PrintScreen')
    expect(shortcutKeys('Control+Super+S', false)).toEqual(['Control', 'Win', 'S'])
  })
})

describe('settings state', () => {
  it('starts from the defaults for this platform', () => {
    expect(settings).toEqual(defaultSettings())
  })

  it('returns a fresh object each time', () => {
    expect(defaultSettings()).not.toBe(defaultSettings())
  })

  it('turns an on/off setting on and off', () => {
    const before = settings.magnifier
    setSetting('magnifier', !before)
    expect(settings.magnifier).toBe(!before)
    setSetting('magnifier', before)
    expect(settings.magnifier).toBe(before)
  })
})
