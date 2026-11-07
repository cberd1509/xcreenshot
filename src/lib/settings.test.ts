import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  CHANGED,
  fallbackSettings,
  resetSettings,
  setCaptureOption,
  setSetting,
  settings,
  syncSettings,
  type Settings,
} from './settings.svelte'

const saved: Settings = {
  shortcut: 'Alt+Shift+F5',
  rememberRegion: false,
  magnifier: true,
  quickCopy: true,
  openAtLogin: false,
  checkForUpdates: false,
}

let calls: { cmd: string; args: unknown }[]
let failing: Set<string>
/** The version Rust is at; each change goes up by one. */
let version: number

beforeEach(() => {
  resetSettings()
  calls = []
  failing = new Set()
  version = 1
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args })
      if (failing.has(cmd)) throw 'That shortcut is in use by another app.'
      if (cmd === 'get_prefs') return { ...saved, version }
      if (cmd === 'set_pref') {
        const { key, value } = args as { key: keyof Settings; value: never }
        return { ...saved, [key]: value, version: ++version }
      }
      if (cmd === 'set_capture_option') {
        const { key, on } = args as { key: keyof Settings; on: boolean }
        return { ...saved, [key]: on, version: ++version }
      }
    },
    { shouldMockEvents: true },
  )
})

afterEach(() => {
  clearMocks()
  vi.restoreAllMocks()
})

describe('settings', () => {
  it('starts from fallbacks with no shortcut: Rust owns the default', () => {
    expect(settings).toEqual(fallbackSettings())
    expect(settings.shortcut).toBe('')
    expect(fallbackSettings()).not.toBe(fallbackSettings())
  })

  it('loads the saved settings and follows changes from other windows', async () => {
    await syncSettings()
    expect(settings).toEqual(saved)
    await emit(CHANGED, { ...saved, magnifier: false, version: 2 })
    expect(settings.magnifier).toBe(false)
  })

  it('ignores a snapshot older than the one shown', async () => {
    await syncSettings()
    await emit(CHANGED, { ...saved, magnifier: false, version: 5 })
    await emit(CHANGED, { ...saved, magnifier: true, version: 4 })
    expect(settings.magnifier).toBe(false)
  })

  it('keeps a change that arrives while loading', async () => {
    mockIPC(
      async (cmd) => {
        if (cmd !== 'get_prefs') return
        await emit(CHANGED, { ...saved, magnifier: false, version: 2 })
        return { ...saved, version: 1 }
      },
      { shouldMockEvents: true },
    )
    await syncSettings()
    expect(settings.magnifier).toBe(false)
  })

  it('keeps the fallbacks if the settings cannot be loaded', async () => {
    failing.add('get_prefs')
    await expect(syncSettings()).rejects.toBeDefined()
    expect(settings).toEqual(fallbackSettings())
  })

  it('changes a setting through Rust and takes what it saved', async () => {
    await setSetting('openAtLogin', true)
    expect(calls).toEqual([{ cmd: 'set_pref', args: { key: 'openAtLogin', value: true } }])
    expect(settings).toEqual({ ...saved, openAtLogin: true })
    await setSetting('shortcut', null)
    expect(calls[1].args).toEqual({ key: 'shortcut', value: null })
  })

  it('changes capture options through the narrower command the overlay may use', async () => {
    await setSetting('quickCopy', false)
    await setSetting('magnifier', false)
    await setSetting('rememberRegion', true)
    expect(calls).toEqual([
      { cmd: 'set_capture_option', args: { key: 'quickCopy', on: false } },
      { cmd: 'set_capture_option', args: { key: 'magnifier', on: false } },
      { cmd: 'set_capture_option', args: { key: 'rememberRegion', on: true } },
    ])
    expect(settings.rememberRegion).toBe(true)
  })

  it('saves the overlay options and logs one that fails', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    setCaptureOption('magnifier', false)
    await vi.waitFor(() => expect(settings.magnifier).toBe(false))
    failing.add('set_capture_option')
    setCaptureOption('rememberRegion', true)
    await vi.waitFor(() =>
      expect(error).toHaveBeenCalledWith('saving rememberRegion failed', expect.anything()),
    )
  })

  it('does not let a late response undo a newer change', async () => {
    await syncSettings()
    await emit(CHANGED, { ...saved, checkForUpdates: true, version: 10 })
    await setSetting('openAtLogin', true) // Answers with version 2.
    expect(settings.checkForUpdates).toBe(true)
    expect(settings.openAtLogin).toBe(false)
  })

  it('changes nothing when Rust refuses', async () => {
    failing.add('set_pref')
    await expect(setSetting('shortcut', 'Alt+S')).rejects.toBe(
      'That shortcut is in use by another app.',
    )
    expect(settings).toEqual(fallbackSettings())
  })
})

/** A fresh copy of the module, imported as if in the Tauri webview. */
async function load() {
  vi.resetModules()
  const tauri = globalThis as { isTauri?: boolean }
  tauri.isTauri = true
  try {
    return await import('./settings.svelte')
  } finally {
    delete tauri.isTauri
  }
}

describe('in the app', () => {
  it('syncs on its own when running in Tauri', async () => {
    const fresh = await load()
    await vi.waitFor(() => expect(fresh.settings).toEqual(saved))
  })

  it('logs when the settings cannot be loaded', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    failing.add('get_prefs')
    await load()
    await vi.waitFor(() =>
      expect(error).toHaveBeenCalledWith('loading settings failed', expect.anything()),
    )
  })
})
