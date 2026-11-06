import { fireEvent, render, screen } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { getVersion } from '@tauri-apps/api/app'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import {
  fallbackSettings,
  resetSettings,
  settings,
  shortcutKeys,
  type Settings,
} from '$lib/settings.svelte'
import { tick } from './test/tick'

const SHORTCUT = 'Control+Shift+Super+S'

let saved: Settings
let version: number
let refuse: string | null
/** Commands that changed a setting. */
let changes: string[]
/** Holds set_pref answers until released, when set. */
let hold: Promise<void> | null

/** A promise and the function that resolves it. */
function deferred() {
  let resolve: () => void = Function.prototype as () => void
  const promise = new Promise<void>((r) => (resolve = r))
  return { promise, resolve }
}

describe('Settings window', () => {
  beforeEach(() => {
    saved = { ...fallbackSettings(), shortcut: SHORTCUT }
    version = 0
    refuse = null
    changes = []
    hold = null
    resetSettings()
    Object.assign(settings, saved)
    mockIPC(async (cmd, args) => {
      if (cmd !== 'set_pref' && cmd !== 'set_capture_option') return
      changes.push(cmd)
      if (hold) await hold
      if (refuse) throw refuse
      const { key, value, on } = args as { key: keyof Settings; value?: never; on?: boolean }
      saved = { ...saved, [key]: on ?? value ?? SHORTCUT }
      return { ...saved, version: ++version }
    })
  })

  afterEach(() => {
    clearMocks()
    vi.restoreAllMocks()
  })

  it('shows every section and the app version', async () => {
    render(App)
    for (const name of ['Shortcuts', 'Capture', 'General']) {
      expect(screen.getByRole('heading', { name })).toBeInTheDocument()
    }
    expect(await screen.findByText('Version 0.1.0')).toBeInTheDocument()
  })

  it('shows the capture shortcut as keys', () => {
    render(App)
    expect(screen.getByText('Take a screenshot')).toBeInTheDocument()
    for (const key of shortcutKeys(SHORTCUT)) {
      expect(screen.getByText(key)).toBeInTheDocument()
    }
  })

  it('records a new shortcut and saves it', async () => {
    render(App)
    await userEvent.click(screen.getByRole('button', { name: 'Change' }))
    await fireEvent.keyDown(window, { code: 'KeyD', altKey: true })
    await fireEvent.keyUp(window, { code: 'KeyD', altKey: true })
    await tick()
    expect(settings.shortcut).toBe('Alt+D')
    await userEvent.click(screen.getByRole('button', { name: 'Reset to default' }))
    expect(settings.shortcut).toBe(SHORTCUT)
  })

  it('saves a setting from its switch', async () => {
    render(App)
    const magnifier = screen.getByRole('switch', { name: 'Magnifier' })
    expect(magnifier).not.toBeChecked()
    await userEvent.click(magnifier)
    expect(saved.magnifier).toBe(true)
    expect(settings.magnifier).toBe(true)
    expect(magnifier).toBeChecked()
  })

  it('keeps a switch as it was and says why when saving fails', async () => {
    refuse = "Couldn't change the login item."
    render(App)
    const login = screen.getByRole('switch', { name: 'Open at login' })
    await userEvent.click(login)
    await tick()
    expect(settings.openAtLogin).toBe(true)
    expect(login).toBeChecked()
    expect(screen.getByRole('alert')).toHaveTextContent(refuse)
    refuse = null
    await userEvent.click(login)
    await tick()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(settings.openAtLogin).toBe(false)
  })

  it('disables a switch while it saves, so a double click sends one change', async () => {
    const { promise, resolve: release } = deferred()
    hold = promise
    render(App)
    const login = screen.getByRole('switch', { name: 'Open at login' })
    await fireEvent.click(login)
    await fireEvent.click(login)
    expect(login).toBeDisabled()
    release()
    await tick()
    expect(changes).toEqual(['set_pref'])
    expect(login).not.toBeDisabled()
    expect(settings.openAtLogin).toBe(false)
  })

  it('has a fallback message for unexpected errors', async () => {
    mockIPC(() => {
      throw new Error('boom')
    })
    render(App)
    await userEvent.click(screen.getByRole('switch', { name: 'Magnifier' }))
    await tick()
    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't save the settings.")
  })

  it('marks the clicked section as current', async () => {
    render(App)
    const general = screen.getByRole('link', { name: 'General' })
    expect(general).not.toHaveAttribute('aria-current')
    await userEvent.click(general)
    expect(general).toHaveAttribute('aria-current', 'true')
  })

  it('hides the version when it cannot be read', async () => {
    vi.mocked(getVersion).mockRejectedValueOnce(new Error('no runtime'))
    render(App)
    await Promise.resolve()
    expect(screen.queryByText(/^Version/)).not.toBeInTheDocument()
  })
})
