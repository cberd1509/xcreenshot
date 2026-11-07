import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { shortcutKeys, spokenKeys } from '$lib/keys'
import {
  CHANGED,
  fallbackSettings,
  resetSettings,
  settings,
  syncSettings,
} from '$lib/settings.svelte'
import { tick } from '../test/tick'
import Onboarding from './Onboarding.svelte'

/** The platform the page sees. */
const platform = vi.hoisted(() => ({ mac: true }))
vi.mock('$lib/keys', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/keys')>()),
  get isMac() {
    return platform.mac
  },
}))

const SHORTCUT = 'Control+Shift+Super+S'
const FALLBACK = 'Control+Shift+PrintScreen'

let calls: string[]
/** The `claim` argument of each PrtSc check. */
let claims: boolean[]
let granted: boolean
let registered: boolean
let taken: boolean
let failing: Set<string>
let pane: string
/** The saved shortcut, as `get_prefs` answers. */
let savedShortcut: string
/** Settings version Rust is at. */
let version: number
/** Rust's answer to `onboarding_tried`: a capture ended before the page listened. */
let triedEarly: boolean
/** Answers held back per command, answered in the order given to `answer`. */
let held: Map<string, ((v: unknown) => void)[]>

const count = (cmd: string) => calls.filter((c) => c === cmd).length
const button = (name: string) => screen.getByRole('button', { name })
const noButton = (name: string) => screen.queryByRole('button', { name })
const isPrimary = (el: HTMLElement) => el.className.includes('bg-primary ')
const permissionStatus = () => screen.getAllByRole('status')[0]
const tryIt = () => screen.queryByText('Try it now.')
const works = () => screen.queryByText('It works.')
const changeButton = () => screen.getByRole('button', { name: /^Change shortcut/ })
/** Lets the mocked IPC answer, then moves the (fake) clock on. */
const after = (ms = 0) => vi.advanceTimersByTimeAsync(ms)

async function mount() {
  render(Onboarding)
  await after()
}

/** Rust sends the new settings to every page after a change. */
async function changed(shortcut: string) {
  await emit(CHANGED, { ...fallbackSettings(), shortcut, version: ++version })
  await after()
}

describe('Onboarding', () => {
  beforeEach(async () => {
    vi.useFakeTimers()
    platform.mac = true
    calls = []
    claims = []
    granted = false
    registered = true
    taken = false
    failing = new Set()
    pane = 'Screen & System Audio Recording'
    version = 1
    savedShortcut = SHORTCUT
    triedEarly = false
    held = new Map()
    resetSettings()
    mockWindows('onboarding')
    mockIPC(
      (cmd, args) => {
        calls.push(cmd)
        if (cmd === 'onboarding_print_screen') claims.push((args as { claim: boolean }).claim)
        if (failing.has(cmd)) throw new Error('nope')
        const waiting = held.get(cmd)
        if (waiting) return new Promise((resolve) => waiting.push(resolve))
        if (cmd === 'onboarding_tried') return triedEarly
        if (cmd === 'permission_granted') return granted
        if (cmd === 'capture_shortcut') return { accelerator: settings.shortcut, registered }
        if (cmd === 'permission_pane') return pane
        if (cmd === 'onboarding_print_screen') return taken
        if (cmd === 'get_prefs') return { ...fallbackSettings(), shortcut: savedShortcut, version }
        if (cmd === 'set_pref') {
          const { value } = args as { value: string }
          return { ...fallbackSettings(), shortcut: value, version: ++version }
        }
      },
      { shouldMockEvents: true },
    )
    await syncSettings()
    calls = []
  })

  afterEach(async () => {
    cleanup()
    vi.useRealTimers()
    await tick()
    clearMocks()
    vi.restoreAllMocks()
  })

  describe('on macOS', () => {
    it('shows the three steps, waits for the permission and checks right away', async () => {
      await mount()
      expect(screen.getByRole('heading', { name: 'Allow screen recording' })).toBeInTheDocument()
      expect(
        screen.getByText('xcreenshot needs this to take screenshots. Nothing leaves your Mac.'),
      ).toBeInTheDocument()
      expect(screen.getAllByRole('listitem').map((li) => li.textContent?.trim())).toEqual([
        expect.stringMatching(/Click Open System Settings below\./),
        expect.stringMatching(
          /Go to Privacy & Security › Screen & System Audio Recording\s+and turn on xcreenshot\./,
        ),
        expect.stringMatching(/Come back to this window\. It updates on its own\./),
      ])
      expect(permissionStatus()).toHaveTextContent('Waiting for permission…')
      expect(count('permission_granted')).toBe(1)
      expect(isPrimary(button('Open System Settings'))).toBe(true)
      expect(button('Later')).toBeInTheDocument()
      expect(noButton('Done')).toBeNull()
      expect(noButton('Restart xcreenshot')).toBeNull()
      expect(tryIt()).toBeNull()
      expect(count('onboarding_print_screen')).toBe(0)
    })

    it('shows the shortcut to change, while waiting too', async () => {
      await mount()
      expect(screen.getByText('Take a screenshot')).toBeInTheDocument()
      expect(screen.getByText('Works from any app. Click to change it.')).toBeInTheDocument()
      expect(changeButton()).toHaveAccessibleName(
        `Change shortcut, currently ${spokenKeys(SHORTCUT)}`,
      )
      for (const key of shortcutKeys(SHORTCUT)) {
        expect(changeButton()).toHaveTextContent(key)
      }
      expect(noButton('Reset to default')).toBeNull()
    })

    it('names the Screen Recording pane as macOS 14 does', async () => {
      pane = 'Screen Recording'
      await mount()
      expect(screen.getByText('Privacy & Security › Screen Recording')).toBeInTheDocument()
    })

    it('names no pane until the version is known', () => {
      render(Onboarding)
      expect(screen.getByText('Privacy & Security')).toBeInTheDocument()
    })

    it('says "Screen Recording" if the version cannot be read', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      failing = new Set(['permission_pane'])
      await mount()
      expect(screen.getByText('Privacy & Security › Screen Recording')).toBeInTheDocument()
      expect(error).toHaveBeenCalledWith('permission_pane failed', expect.any(Error))
    })

    it('shows the app icon', async () => {
      await mount()
      expect(screen.getByRole('img', { name: 'xcreenshot' })).toHaveAttribute(
        'src',
        expect.stringContaining('128x128@2x.png'),
      )
    })

    it('closes the window on Later', async () => {
      await mount()
      await fireEvent.click(button('Later'))
      await after()
      expect(calls).toContain('plugin:window|close')
    })

    it('logs when the window cannot close', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      failing = new Set(['plugin:window|close'])
      await mount()
      await fireEvent.click(button('Later'))
      await after()
      expect(error).toHaveBeenCalledWith('close failed', expect.any(Error))
    })

    it('re-checks every second; once allowed, says so and offers Done and Try it', async () => {
      await mount()
      await after(1000)
      expect(count('permission_granted')).toBe(2)
      granted = true
      await after(1000)
      expect(permissionStatus()).toHaveTextContent('Screen recording is on.')
      expect(noButton('Later')).toBeNull()
      expect(noButton('Open System Settings')).toBeNull()
      expect(isPrimary(button('Done'))).toBe(true)
      expect(tryIt()).toBeInTheDocument()
      expect(screen.getByText('Press the keys and drag over anything.')).toBeInTheDocument()
      await fireEvent.click(button('Done'))
      await after()
      expect(calls).toContain('plugin:window|close')
    })

    it('keeps checking slowly once allowed and goes back if access is removed', async () => {
      granted = true
      await mount()
      expect(button('Done')).toBeInTheDocument()
      await after(4000)
      expect(count('permission_granted')).toBe(1)
      granted = false
      await after(1000)
      expect(count('permission_granted')).toBe(2)
      expect(noButton('Done')).toBeNull()
      expect(permissionStatus()).toHaveTextContent('Waiting for permission…')
    })

    it('says it works once a capture opened with the shortcut closes', async () => {
      granted = true
      await mount()
      expect(works()).toBeNull()
      await emit('capture:closed')
      await after()
      expect(tryIt()).toBeNull()
      expect(works()).toBeInTheDocument()
      expect(screen.getByText(/You're all set\./)).toBeInTheDocument()
    })

    it('says another app has the shortcut, and offers no Try it, when it is not registered', async () => {
      registered = false
      granted = true
      await mount()
      expect(screen.getByRole('alert')).toHaveTextContent('That shortcut is in use by another app.')
      expect(tryIt()).toBeNull()
    })

    it('changes the shortcut like Settings does', async () => {
      granted = true
      await mount()
      await fireEvent.click(changeButton())
      await after()
      expect(calls).toContain('pause_capture_shortcut')
      await fireEvent.keyDown(window, { code: 'KeyD', altKey: true })
      await fireEvent.keyUp(window, { code: 'KeyD', altKey: true })
      await after()
      expect(calls).toContain('resume_capture_shortcut')
      expect(calls).toContain('set_pref')
      expect(settings.shortcut).toBe('Alt+D')
      expect(changeButton()).toHaveAccessibleName(
        `Change shortcut, currently ${spokenKeys('Alt+D')}`,
      )
      // Reads whether the new one registered.
      expect(count('capture_shortcut')).toBe(2)
    })

    it('follows a shortcut changed in Settings', async () => {
      granted = true
      await mount()
      await changed('Alt+Shift+F5')
      expect(changeButton()).toHaveTextContent('F5')
      expect(screen.getAllByText('F5')).toHaveLength(2)
    })

    it('opens System Settings from its button', async () => {
      await mount()
      await fireEvent.click(button('Open System Settings'))
      await after()
      expect(calls).toContain('open_permission_settings')
    })

    it('asks for a restart when back from System Settings twice without the permission', async () => {
      await mount()
      await fireEvent.focus(window)
      await after()
      expect(noButton('Restart xcreenshot')).toBeNull()

      await fireEvent.click(button('Open System Settings'))
      await after()
      await fireEvent.focus(window)
      await after()
      // Once may just mean the switch isn't on yet.
      expect(noButton('Restart xcreenshot')).toBeNull()
      expect(permissionStatus()).toHaveTextContent('Waiting for permission…')
      await fireEvent.focus(window)
      await after()
      expect(permissionStatus()).toHaveTextContent(/macOS needs xcreenshot to restart/)
      expect(isPrimary(button('Restart xcreenshot'))).toBe(true)
      expect(isPrimary(button('Open System Settings'))).toBe(false)
      await fireEvent.click(button('Restart xcreenshot'))
      await after()
      expect(calls).toContain('restart_app')
    })

    it('does not ask for a restart if System Settings did not open', async () => {
      vi.spyOn(console, 'error').mockImplementation(() => {})
      failing = new Set(['open_permission_settings'])
      await mount()
      await fireEvent.click(button('Open System Settings'))
      await after()
      await fireEvent.focus(window)
      await after()
      expect(noButton('Restart xcreenshot')).toBeNull()
    })

    it('shows the permission on when back from System Settings with it', async () => {
      await mount()
      await fireEvent.click(button('Open System Settings'))
      await after()
      granted = true
      await fireEvent.focus(window)
      await after()
      expect(permissionStatus()).toHaveTextContent('Screen recording is on.')
      await fireEvent.focus(window)
      await after()
      expect(count('permission_granted')).toBe(2)
    })

    it('backs off after failed checks and says so after a few', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      failing = new Set(['permission_granted'])
      await mount()
      expect(screen.queryByRole('alert')).toBeNull()
      await after(1000) // 2nd check
      await after(2000) // 3rd
      expect(count('permission_granted')).toBe(3)
      expect(screen.getByRole('alert')).toHaveTextContent("Can't check the permission")
      await after(3999)
      expect(count('permission_granted')).toBe(3)
      await after(1) // 4th, after 4 s
      expect(count('permission_granted')).toBe(4)
      expect(error).toHaveBeenCalledWith('permission_granted failed', expect.any(Error))

      failing.clear()
      await after(8000)
      expect(screen.queryByRole('alert')).toBeNull()
    })

    it('a failed check on focus does not ask for a restart', async () => {
      vi.spyOn(console, 'error').mockImplementation(() => {})
      await mount()
      await fireEvent.click(button('Open System Settings'))
      await after()
      failing = new Set(['permission_granted'])
      await fireEvent.focus(window)
      await after()
      expect(noButton('Restart xcreenshot')).toBeNull()
    })

    it('stops checking when the window closes', async () => {
      await mount()
      cleanup()
      await after(10_000)
      expect(count('permission_granted')).toBe(1)
    })

    it('stops even if a check was answering when the window closed', async () => {
      render(Onboarding)
      cleanup()
      await after(10_000)
      expect(count('permission_granted')).toBe(1)
    })

    it('logs every failed command', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      failing = new Set(['capture_shortcut', 'open_permission_settings'])
      await mount()
      await fireEvent.click(button('Open System Settings'))
      await after()
      expect(error.mock.calls.map((c) => c[0])).toEqual(
        expect.arrayContaining(['capture_shortcut failed', 'open_permission_settings failed']),
      )
      // Unknown is not "taken": Try it still shows once allowed.
      granted = true
      await after(1000)
      expect(tryIt()).toBeInTheDocument()
    })
  })

  describe('answers that cross', () => {
    it('keeps the latest permission check when an older one answers last', async () => {
      held.set('permission_granted', [])
      render(Onboarding)
      await after()
      await fireEvent.click(button('Open System Settings'))
      await after()
      await fireEvent.focus(window) // A second check while the first is out.
      await after()
      const [first, second] = held.get('permission_granted')!
      second(true)
      await after()
      expect(permissionStatus()).toHaveTextContent('Screen recording is on.')
      first(false)
      await after()
      expect(permissionStatus()).toHaveTextContent('Screen recording is on.')
    })

    it('keeps the latest shortcut status when an older one answers last', async () => {
      granted = true
      held.set('capture_shortcut', [])
      await mount()
      await fireEvent.focus(window)
      await after()
      const [first, second] = held.get('capture_shortcut')!
      second({ accelerator: SHORTCUT, registered: true })
      await after()
      first({ accelerator: SHORTCUT, registered: false })
      await after()
      expect(screen.queryByRole('alert')).toBeNull()
      expect(tryIt()).toBeInTheDocument()
    })

    it('keeps the latest PrtSc check when an older one answers last', async () => {
      platform.mac = false
      Object.assign(settings, { shortcut: FALLBACK })
      held.set('onboarding_print_screen', [])
      await mount()
      await fireEvent.focus(window)
      await after()
      const [first, second] = held.get('onboarding_print_screen')!
      second(false)
      await after()
      first(true)
      await after()
      expect(screen.queryByText('Want to use PrtSc instead?')).toBeNull()
    })

    it('shows it works for a capture that ended before the page listened', async () => {
      granted = true
      triedEarly = true
      await mount()
      expect(works()).toBeInTheDocument()
    })

    it('logs when it cannot ask', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      failing = new Set(['onboarding_tried'])
      granted = true
      await mount()
      expect(error).toHaveBeenCalledWith('onboarding_tried failed', expect.any(Error))
      expect(tryIt()).toBeInTheDocument()
    })
  })

  describe('on Windows', () => {
    beforeEach(() => {
      platform.mac = false
      Object.assign(settings, { shortcut: 'PrintScreen' })
    })

    it('shows the shortcut and Try it, with no permission steps', async () => {
      await mount()
      expect(screen.getByRole('heading', { name: 'xcreenshot is ready' })).toBeInTheDocument()
      expect(screen.getByText('It lives in the system tray.')).toBeInTheDocument()
      expect(screen.queryByRole('listitem')).toBeNull()
      expect(screen.getByText('Works from any app. Click to change it.')).toBeInTheDocument()
      expect(changeButton()).toHaveTextContent('PrtSc')
      expect(tryIt()).toBeInTheDocument()
      expect(screen.queryByText('Want to use PrtSc instead?')).toBeNull()
      expect(count('permission_granted')).toBe(0)
      expect(count('permission_pane')).toBe(0)
      expect(count('onboarding_print_screen')).toBe(0)
      expect(noButton('Later')).toBeNull()
      expect(isPrimary(button('Done'))).toBe(true)
      await emit('capture:closed')
      await after()
      expect(works()).toBeInTheDocument()
    })

    it('explains Ctrl Shift PrtSc and switches to PrtSc once Snipping Tool lets go', async () => {
      taken = true
      Object.assign(settings, { shortcut: FALLBACK })
      await mount()
      expect(
        screen.getByText('PrtSc opens Snipping Tool, so xcreenshot uses this. Click to change it.'),
      ).toBeInTheDocument()
      expect(changeButton()).toHaveTextContent(/Ctrl\s*Shift\s*PrtSc/)
      expect(screen.getByRole('heading', { name: 'Want to use PrtSc instead?' })).toBeVisible()
      expect(screen.getAllByRole('listitem').map((li) => li.textContent?.trim())).toEqual([
        expect.stringMatching(/Click Open keyboard settings\.$/),
        expect.stringMatching(/Turn off\s+Use the Print screen key to open screen capture\.$/),
        expect.stringMatching(/Come back here\. xcreenshot switches to PrtSc on its own\.$/),
      ])
      await fireEvent.click(button('Open keyboard settings'))
      await after()
      expect(calls).toContain('open_keyboard_settings')

      // Back from Windows Settings: Rust takes PrtSc and sends the new shortcut.
      taken = false
      await fireEvent.focus(window)
      await after()
      await changed('PrintScreen')
      // Only claims PrtSc once the steps were shown.
      expect(claims).toEqual([false, true])
      expect(screen.queryByText('Want to use PrtSc instead?')).toBeNull()
      expect(changeButton()).toHaveAccessibleName('Change shortcut, currently PrintScreen')
      expect(screen.getByText('Works from any app. Click to change it.')).toBeInTheDocument()
    })

    it('drops the PrtSc steps when another shortcut is picked here', async () => {
      taken = true
      Object.assign(settings, { shortcut: FALLBACK })
      await mount()
      expect(screen.getByText('Want to use PrtSc instead?')).toBeInTheDocument()
      await fireEvent.click(changeButton())
      await after()
      await fireEvent.keyDown(window, { code: 'KeyD', altKey: true })
      await fireEvent.keyUp(window, { code: 'KeyD', altKey: true })
      await after()
      expect(settings.shortcut).toBe('Alt+D')
      expect(screen.queryByText('Want to use PrtSc instead?')).toBeNull()
      expect(screen.getByText('Works from any app. Click to change it.')).toBeInTheDocument()
      expect(claims).toEqual([false])
    })

    it('logs failed commands', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      failing = new Set(['onboarding_print_screen', 'open_keyboard_settings'])
      taken = true
      Object.assign(settings, { shortcut: FALLBACK })
      await mount()
      expect(error).toHaveBeenCalledWith('onboarding_print_screen failed', expect.any(Error))
      failing = new Set(['open_keyboard_settings'])
      await fireEvent.focus(window)
      await after()
      await fireEvent.click(button('Open keyboard settings'))
      await after()
      expect(error).toHaveBeenCalledWith('open_keyboard_settings failed', expect.any(Error))
    })
  })
})
