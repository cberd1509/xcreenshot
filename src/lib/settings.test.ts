import { afterEach, describe, expect, it, vi } from 'vitest'

async function loadOn(userAgent: string) {
  vi.resetModules()
  vi.stubGlobal('navigator', { userAgent })
  return import('./settings.svelte')
}

afterEach(() => vi.unstubAllGlobals())

describe('settings on macOS', () => {
  it('defaults to ⌃⇧⌘S and shows Mac key symbols', async () => {
    const s = await loadOn('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')
    expect(s.isMac).toBe(true)
    expect(s.defaultSettings.shortcut).toBe('Control+Shift+Super+S')
    expect(s.shortcutKeys(s.defaultSettings.shortcut)).toEqual(['⌃', '⇧', '⌘', 'S'])
    expect(s.shortcutKeys('Alt+CmdOrCtrl+K')).toEqual(['⌥', '⌘', 'K'])
  })
})

describe('settings on Windows', () => {
  it('defaults to PrintScreen and shows Windows key names', async () => {
    const s = await loadOn('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')
    expect(s.isMac).toBe(false)
    expect(s.defaultSettings.shortcut).toBe('PrintScreen')
    expect(s.shortcutKeys('Control+Super+S')).toEqual(['Control', 'Win', 'S'])
  })
})

describe('settings state', () => {
  it('starts from the defaults without sharing the object', async () => {
    const s = await loadOn('Macintosh')
    expect(s.settings).toEqual(s.defaultSettings)
    s.settings.magnifier = true
    expect(s.defaultSettings.magnifier).toBe(false)
  })
})
