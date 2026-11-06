import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { shortcutKeys } from '$lib/settings.svelte'
import { tick } from '../test/tick'
import Permission from './Permission.svelte'

let calls: string[]
let granted: boolean
let shortcut: string | null
let failing: Set<string>

const count = (cmd: string) => calls.filter((c) => c === cmd).length
const button = (name: string) => screen.getByRole('button', { name })
const isPrimary = (el: HTMLElement) => el.className.includes('bg-primary ')
/** Lets the mocked IPC answer, then moves the clock on. */
const after = async (ms: number) => {
  await vi.advanceTimersByTimeAsync(ms)
  await tick()
}

async function mount() {
  render(Permission)
  await after(0)
}

describe('Permission', () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
    calls = []
    granted = false
    shortcut = 'Control+Shift+Super+S'
    failing = new Set()
    mockWindows('permission')
    mockIPC((cmd) => {
      calls.push(cmd)
      if (failing.has(cmd)) throw new Error('nope')
      if (cmd === 'permission_granted') return granted
      if (cmd === 'capture_shortcut') return shortcut
    })
  })

  afterEach(async () => {
    cleanup()
    await tick()
    clearMocks()
    vi.useRealTimers()
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
    expect(screen.getByRole('heading', { name: 'All set.' })).toBeInTheDocument()
    for (const key of shortcutKeys('Control+Shift+Super+S')) {
      expect(screen.getByText(key)).toBeInTheDocument()
    }
    await after(3000)
    expect(count('permission_granted')).toBe(3)
  })

  it('asks for a shortcut when none is registered', async () => {
    shortcut = null
    granted = true
    await mount()
    expect(screen.getByText('Set a shortcut in Settings.')).toBeInTheDocument()
  })

  it('opens System Settings and restarts from the buttons', async () => {
    await mount()
    await fireEvent.click(button('Open System Settings'))
    await fireEvent.click(button('Restart xcreenshot'))
    await tick()
    expect(calls).toContain('open_permission_settings')
    expect(calls).toContain('restart_app')
  })

  it('offers the restart from the done screen', async () => {
    granted = true
    await mount()
    expect(screen.getByText(/Screenshots look empty\?/)).toBeInTheDocument()
    await fireEvent.click(button('Restart xcreenshot'))
    await tick()
    expect(calls).toContain('restart_app')
  })

  it('asks for a restart when back from System Settings without the permission', async () => {
    await mount()
    await fireEvent.focus(window)
    await tick()
    expect(isPrimary(button('Restart xcreenshot'))).toBe(false)

    await fireEvent.click(button('Open System Settings'))
    await fireEvent.focus(window)
    await tick()
    expect(screen.getByText(/macOS needs xcreenshot to restart/)).toBeInTheDocument()
    expect(isPrimary(button('Restart xcreenshot'))).toBe(true)
    expect(isPrimary(button('Open System Settings'))).toBe(false)
  })

  it('shows done when back from System Settings with the permission', async () => {
    await mount()
    await fireEvent.click(button('Open System Settings'))
    granted = true
    await fireEvent.focus(window)
    await tick()
    expect(screen.getByRole('heading', { name: 'All set.' })).toBeInTheDocument()
    await fireEvent.focus(window)
    await tick()
    expect(count('permission_granted')).toBe(2)
  })

  it('stops checking when the window closes', async () => {
    await mount()
    cleanup()
    await after(3000)
    expect(count('permission_granted')).toBe(1)
  })

  it('logs every failed command', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    failing = new Set([
      'permission_granted',
      'capture_shortcut',
      'open_permission_settings',
      'restart_app',
    ])
    await mount()
    await fireEvent.click(button('Open System Settings'))
    await fireEvent.focus(window)
    await fireEvent.click(button('Restart xcreenshot'))
    await tick()
    const logged = error.mock.calls.map((c) => c[0])
    expect(logged).toEqual(
      expect.arrayContaining([
        'permission_granted failed',
        'capture_shortcut failed',
        'open_permission_settings failed',
        'restart_app failed',
      ]),
    )
    expect(logged.filter((m) => m === 'permission_granted failed')).toHaveLength(2)
  })

  it('treats a failed shortcut lookup as no shortcut', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    failing = new Set(['capture_shortcut'])
    granted = true
    await mount()
    expect(screen.getByText('Set a shortcut in Settings.')).toBeInTheDocument()
  })
})
