import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { shortcutKeys } from '$lib/keys'
import { tick } from '../test/tick'
import Permission from './Permission.svelte'

const SHORTCUT = 'Control+Shift+Super+S'

let calls: string[]
let granted: boolean
let registered: boolean
let accelerator: string
let failing: Set<string>

const count = (cmd: string) => calls.filter((c) => c === cmd).length
const button = (name: string) => screen.getByRole('button', { name })
const isPrimary = (el: HTMLElement) => el.className.includes('bg-primary ')
const done = () => screen.queryByRole('heading', { name: 'All set.' })
/** Lets the mocked IPC answer, then moves the (fake) clock on. */
const after = (ms = 0) => vi.advanceTimersByTimeAsync(ms)

async function mount() {
  render(Permission)
  await after()
}

describe('Permission', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    calls = []
    granted = false
    registered = true
    accelerator = SHORTCUT
    failing = new Set()
    mockWindows('permission')
    mockIPC((cmd) => {
      calls.push(cmd)
      if (failing.has(cmd)) throw new Error('nope')
      if (cmd === 'permission_granted') return granted
      if (cmd === 'capture_shortcut') return { accelerator, registered }
    })
  })

  afterEach(async () => {
    cleanup()
    vi.useRealTimers()
    await tick()
    clearMocks()
    vi.restoreAllMocks()
  })

  it('shows the three steps and checks right away', async () => {
    await mount()
    expect(screen.getByRole('heading', { name: 'Allow Screen Recording' })).toBeInTheDocument()
    expect(screen.getAllByRole('listitem')).toHaveLength(3)
    expect(screen.getByText('Turn on xcreenshot')).toBeInTheDocument()
    expect(screen.getByText(/Privacy & Security › Screen Recording/)).toBeInTheDocument()
    expect(count('permission_granted')).toBe(1)
    expect(isPrimary(button('Open System Settings'))).toBe(true)
  })

  it('re-checks every second and shows the shortcut once granted', async () => {
    await mount()
    await after(1000)
    expect(count('permission_granted')).toBe(2)
    granted = true
    await after(1000)
    expect(done()).toBeInTheDocument()
    for (const key of shortcutKeys(SHORTCUT)) {
      expect(screen.getByText(key)).toBeInTheDocument()
    }
  })

  it('keeps checking slowly once granted and goes back if access is removed', async () => {
    granted = true
    await mount()
    expect(done()).toBeInTheDocument()
    await after(4000)
    expect(count('permission_granted')).toBe(1)
    granted = false
    await after(1000)
    expect(count('permission_granted')).toBe(2)
    expect(done()).toBeNull()
    expect(screen.getByRole('heading', { name: 'Allow Screen Recording' })).toBeInTheDocument()
  })

  it('says another app has the shortcut when it is not registered', async () => {
    registered = false
    granted = true
    await mount()
    expect(screen.getByText(/Another app is using/)).toBeInTheDocument()
    for (const key of shortcutKeys(SHORTCUT)) {
      expect(screen.getByText(key)).toBeInTheDocument()
    }
    expect(screen.getByText(/Close it and restart xcreenshot/)).toBeInTheDocument()
    expect(screen.queryByText(/Screenshots look empty/)).toBeNull()
    await fireEvent.click(button('Restart xcreenshot'))
    await after()
    expect(calls).toContain('restart_app')
  })

  it('opens System Settings and restarts from the buttons', async () => {
    await mount()
    await fireEvent.click(button('Open System Settings'))
    await fireEvent.click(button('Restart xcreenshot'))
    await after()
    expect(calls).toContain('open_permission_settings')
    expect(calls).toContain('restart_app')
  })

  it('offers the restart from the done screen', async () => {
    granted = true
    await mount()
    expect(screen.getByText(/Screenshots look empty\?/)).toBeInTheDocument()
    await fireEvent.click(button('Restart xcreenshot'))
    await after()
    expect(calls).toContain('restart_app')
  })

  it('asks for a restart when back from System Settings without the permission', async () => {
    await mount()
    await fireEvent.focus(window)
    await after()
    expect(isPrimary(button('Restart xcreenshot'))).toBe(false)

    await fireEvent.click(button('Open System Settings'))
    await after()
    await fireEvent.focus(window)
    await after()
    expect(screen.getByText(/macOS needs xcreenshot to restart/)).toBeInTheDocument()
    expect(isPrimary(button('Restart xcreenshot'))).toBe(true)
    expect(isPrimary(button('Open System Settings'))).toBe(false)
  })

  it('does not ask for a restart if System Settings did not open', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    failing = new Set(['open_permission_settings'])
    await mount()
    await fireEvent.click(button('Open System Settings'))
    await after()
    await fireEvent.focus(window)
    await after()
    expect(isPrimary(button('Restart xcreenshot'))).toBe(false)
  })

  it('shows done when back from System Settings with the permission', async () => {
    await mount()
    await fireEvent.click(button('Open System Settings'))
    await after()
    granted = true
    await fireEvent.focus(window)
    await after()
    expect(done()).toBeInTheDocument()
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
    expect(isPrimary(button('Restart xcreenshot'))).toBe(false)
  })

  it('stops checking when the window closes', async () => {
    await mount()
    cleanup()
    await after(10_000)
    expect(count('permission_granted')).toBe(1)
  })

  it('stops even if a check was answering when the window closed', async () => {
    render(Permission)
    cleanup()
    await after(10_000)
    expect(count('permission_granted')).toBe(1)
  })

  it('logs every failed command', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    failing = new Set(['capture_shortcut', 'open_permission_settings', 'restart_app'])
    await mount()
    await fireEvent.click(button('Open System Settings'))
    await fireEvent.click(button('Restart xcreenshot'))
    await after()
    expect(error.mock.calls.map((c) => c[0])).toEqual(
      expect.arrayContaining([
        'capture_shortcut failed',
        'open_permission_settings failed',
        'restart_app failed',
      ]),
    )
  })

  it('shows a shortcut changed in Settings when the window is back in focus', async () => {
    granted = true
    await mount()
    accelerator = 'Alt+Shift+F5'
    await fireEvent.focus(window)
    await after()
    expect(screen.getByText('F5')).toBeInTheDocument()
  })

  it('leaves the shortcut out when it cannot be read', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    failing = new Set(['capture_shortcut'])
    granted = true
    await mount()
    expect(done()).toBeInTheDocument()
    expect(screen.queryByText(/Another app is using|to take a screenshot/)).toBeNull()
  })
})
