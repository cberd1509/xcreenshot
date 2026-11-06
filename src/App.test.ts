import { fireEvent, render, screen, within } from '@testing-library/svelte'
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

/** The platform the page sees. */
const platform = vi.hoisted(() => ({ windows: false, mac: true }))
vi.mock('$lib/keys', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/keys')>()),
  get isWindows() {
    return platform.windows
  },
  get isMac() {
    return platform.mac
  },
}))

let saved: Settings
let version: number
let refuse: string | null
/** Commands that changed a setting. */
let changes: string[]
/** Every command called. */
let commands: string[]
let refuseRepo: boolean
/** Holds set_pref answers until released, when set. */
let hold: Promise<void> | null

/** A promise and the function that resolves it. */
function deferred() {
  let resolve: () => void = Function.prototype as () => void
  const promise = new Promise<void>((r) => (resolve = r))
  return { promise, resolve }
}

/** Lays the sections out 200 px apart in a 300 px tall page, as jsdom has no layout. */
function layOut() {
  const page = screen.getByRole('main')
  const ids = ['shortcuts', 'capture', 'general', 'about']
  ids.forEach((id, i) => {
    const el = document.getElementById(id)!
    Object.defineProperty(el, 'offsetTop', { value: 28 + i * 200 })
    Object.defineProperty(el, 'offsetHeight', { value: 170 })
  })
  Object.defineProperty(page, 'clientHeight', { value: 300 })
  Object.defineProperty(page, 'scrollHeight', { value: 28 + 4 * 200 })
  return async (top: number) => {
    page.scrollTop = top
    await fireEvent.scroll(page)
  }
}

const current = () =>
  within(screen.getByRole('navigation')).getByRole('link', { current: 'page' }).textContent?.trim()

describe('Settings window', () => {
  beforeEach(() => {
    saved = { ...fallbackSettings(), shortcut: SHORTCUT }
    version = 0
    refuse = null
    changes = []
    hold = null
    commands = []
    refuseRepo = false
    platform.windows = false
    platform.mac = true
    resetSettings()
    Object.assign(settings, saved)
    mockIPC(async (cmd, args) => {
      commands.push(cmd)
      if (cmd === 'open_repo' && refuseRepo) throw 'no browser'
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

  it('lists the sections in the sidebar, with the app version at the bottom', async () => {
    render(App)
    const nav = screen.getByRole('navigation', { name: 'Settings sections' })
    expect(
      within(nav)
        .getAllByRole('link')
        .map((a) => a.textContent?.trim()),
    ).toEqual(['General', 'Shortcuts', 'Capture', 'About'])
    expect(await within(nav).findByText('Version 0.1.0')).toBe(nav.lastElementChild)
    for (const name of ['Shortcuts', 'Capture', 'General', 'About']) {
      expect(screen.getByRole('heading', { name })).toBeInTheDocument()
    }
  })

  it('describes each setting', () => {
    render(App)
    for (const text of [
      'Works from any app.',
      'Start with your last selection.',
      'Zoom in to select exact pixels.',
      'Start xcreenshot when you turn on your Mac.',
      'Get new versions automatically.',
    ]) {
      expect(screen.getByText(text)).toBeInTheDocument()
    }
  })

  it('says "log in" for Open at login on Linux', () => {
    platform.mac = false
    render(App)
    expect(screen.getByText('Start xcreenshot when you log in.')).toBeInTheDocument()
  })

  it('sizes the switch thumbs from the state attribute the switch sets', async () => {
    render(App)
    const magnifier = screen.getByRole('switch', { name: 'Magnifier' })
    const thumb = magnifier.querySelector('[data-slot="switch-thumb"]')
    expect(magnifier).toHaveAttribute('data-state', 'unchecked')
    expect(magnifier.className).toContain('[&[data-state=unchecked]_[data-slot=switch-thumb]]')
    await userEvent.click(magnifier)
    expect(thumb).toHaveAttribute('data-state', 'checked')
    expect(magnifier.className).toContain('[&_[data-slot=switch-thumb][data-state=checked]]')
  })

  it('says "sign in" for Open at login on Windows', () => {
    platform.windows = true
    platform.mac = false
    render(App)
    expect(screen.getByText('Start xcreenshot when you sign in.')).toBeInTheDocument()
  })

  it('says what the app is in About and opens its repo in the browser', async () => {
    render(App)
    const about = screen.getByRole('heading', { name: 'About' }).parentElement!
    expect(within(about).getByText('xcreenshot')).toBeInTheDocument()
    expect(await within(about).findByText('Version 0.1.0')).toBeInTheDocument()
    expect(within(about).getByText('MIT License')).toBeInTheDocument()
    const repo = within(about).getByRole('button', { name: 'github.com/cberd1509/xcreenshot' })
    await userEvent.click(repo)
    expect(commands).toContain('open_repo')
  })

  it('logs when the repo cannot be opened', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    refuseRepo = true
    render(App)
    await userEvent.click(screen.getByRole('button', { name: 'github.com/cberd1509/xcreenshot' }))
    await tick()
    expect(error).toHaveBeenCalledWith('open_repo failed', 'no browser')
  })

  it('shows the capture shortcut as keys in its button', () => {
    render(App)
    expect(screen.getByText('Take a screenshot')).toBeInTheDocument()
    const change = screen.getByRole('button', { name: /^Change shortcut, currently/ })
    for (const key of shortcutKeys(SHORTCUT)) {
      expect(within(change).getByText(key)).toBeInTheDocument()
    }
  })

  it('records a new shortcut and saves it', async () => {
    render(App)
    await userEvent.click(screen.getByRole('button', { name: /^Change shortcut/ }))
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

  it('marks the section in view, starting with the first on the page', async () => {
    render(App)
    const scrollTo = layOut()
    expect(current()).toBe('Shortcuts')
    await scrollTo(200)
    expect(current()).toBe('Capture')
    await scrollTo(400)
    expect(current()).toBe('General')
    await scrollTo(528) // The end: About never reaches the top.
    expect(current()).toBe('About')
    await scrollTo(0)
    expect(current()).toBe('Shortcuts')
  })

  it('marks a clicked section while it is in view, then follows the scroll again', async () => {
    render(App)
    const scrollTo = layOut()
    await userEvent.click(screen.getByRole('link', { name: 'General' }))
    expect(current()).toBe('General')
    // Scrolled to the end, General is still visible, so it stays marked.
    await scrollTo(528)
    expect(current()).toBe('General')
    await scrollTo(0)
    expect(current()).toBe('Shortcuts')
    await scrollTo(528)
    expect(current()).toBe('About')
  })

  it('hides the version when it cannot be read', async () => {
    vi.mocked(getVersion).mockRejectedValueOnce(new Error('no runtime'))
    render(App)
    await Promise.resolve()
    expect(screen.queryByText(/Version/)).not.toBeInTheDocument()
  })
})

const line = () => screen.queryByText('Allow xcreenshot in System Settings › Login Items.')

describe('Login item approval', () => {
  let needs: boolean | Error
  let opened: number
  /** Calls to login_item_needs_approval. */
  let checks: number

  beforeEach(() => {
    needs = true
    opened = 0
    checks = 0
    resetSettings()
    Object.assign(settings, { ...fallbackSettings(), shortcut: SHORTCUT })
    mockIPC((cmd, args) => {
      if (cmd === 'login_item_needs_approval') {
        checks++
        if (needs instanceof Error) throw needs
        return needs
      }
      if (cmd === 'open_login_items') opened++
      if (cmd === 'set_pref') {
        const { value } = args as { value: boolean }
        return { ...settings, openAtLogin: value, version: 1 }
      }
    })
  })

  afterEach(() => {
    clearMocks()
    vi.restoreAllMocks()
  })

  it('asks to allow the login item and opens Login Items', async () => {
    render(App)
    await tick()
    expect(line()).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'Open System Settings' }))
    expect(opened).toBe(1)
  })

  it('checks again on focus and after the switch changes', async () => {
    render(App)
    await tick()
    expect(line()).toBeInTheDocument()
    needs = false
    await fireEvent.focus(window)
    await tick()
    expect(line()).toBeNull()
    needs = true
    await userEvent.click(screen.getByRole('switch', { name: 'Open at login' }))
    await tick()
    expect(settings.openAtLogin).toBe(false)
    expect(line()).toBeNull()
    const before = checks
    await fireEvent.focus(window)
    expect(checks).toBe(before)
    await userEvent.click(screen.getByRole('switch', { name: 'Open at login' }))
    await tick()
    expect(line()).toBeInTheDocument()
  })

  it('shows nothing when the check fails', async () => {
    needs = new Error('no runtime')
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    render(App)
    await tick()
    expect(line()).toBeNull()
    expect(error).toHaveBeenCalledWith('login_item_needs_approval failed', needs)
  })

  it('logs a failure to open Login Items', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    render(App)
    await tick()
    mockIPC((cmd) => {
      if (cmd === 'open_login_items') throw new Error('nope')
      return true
    })
    await userEvent.click(screen.getByRole('button', { name: 'Open System Settings' }))
    await tick()
    expect(error).toHaveBeenCalledWith('open_login_items failed', expect.any(Error))
  })

  it('ignores an older check that answers last', async () => {
    const answers: ((needs: boolean) => void)[] = []
    mockIPC((cmd) => {
      if (cmd === 'login_item_needs_approval')
        return new Promise<boolean>((resolve) => answers.push(resolve))
    })
    render(App)
    await tick()
    await fireEvent.focus(window)
    expect(answers).toHaveLength(2)
    answers[1](false)
    await tick()
    answers[0](true)
    await tick()
    expect(line()).toBeNull()
  })

  it('drops a check that answers after Open at login was turned off', async () => {
    const answers: ((needs: boolean) => void)[] = []
    mockIPC((cmd, args) => {
      if (cmd === 'login_item_needs_approval')
        return new Promise<boolean>((resolve) => answers.push(resolve))
      if (cmd === 'set_pref') {
        const { value } = args as { value: boolean }
        return { ...settings, openAtLogin: value, version: 1 }
      }
    })
    render(App)
    await tick()
    expect(answers).toHaveLength(1)
    const login = screen.getByRole('switch', { name: 'Open at login' })
    await userEvent.click(login)
    await tick()
    expect(settings.openAtLogin).toBe(false)
    answers[0](true)
    await tick()
    await userEvent.click(login)
    await tick()
    expect(settings.openAtLogin).toBe(true)
    expect(answers).toHaveLength(2)
    expect(line()).toBeNull()
    answers[1](true)
    await tick()
    expect(line()).toBeInTheDocument()
  })
})
