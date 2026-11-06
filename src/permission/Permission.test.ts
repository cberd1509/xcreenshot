import { cleanup, render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { settings, shortcutKeys } from '$lib/settings.svelte'
import Permission from './Permission.svelte'

let calls: string[]
let grantedNow: boolean
let fail: string | null

const tick = () => new Promise((r) => setTimeout(r, 0))

/** Renders and waits until the page has asked for the current state. */
async function mount() {
  const view = render(Permission)
  await waitFor(() => expect(calls).toContain('permission_granted'))
  return view
}

describe('Permission', () => {
  beforeEach(() => {
    calls = []
    grantedNow = false
    fail = null
    mockWindows('permission')
    mockIPC(
      (cmd) => {
        calls.push(cmd)
        if (cmd === fail) throw new Error('nope')
        if (cmd === 'permission_granted') return grantedNow
      },
      { shouldMockEvents: true },
    )
  })

  afterEach(async () => {
    cleanup()
    await tick()
    clearMocks()
    vi.restoreAllMocks()
  })

  it('shows the three steps', async () => {
    await mount()
    expect(screen.getByRole('heading', { name: 'Allow Screen Recording' })).toBeInTheDocument()
    const steps = screen.getAllByRole('listitem')
    expect(steps.map((s) => s.querySelector('.font-medium')?.textContent)).toEqual([
      'Open System Settings',
      'Turn on xcreenshot',
      'Quit and reopen',
    ])
    expect(screen.getByText(/Privacy & Security › Screen Recording/)).toBeInTheDocument()
  })

  it('opens System Settings and restarts from the buttons', async () => {
    await mount()
    await userEvent.click(screen.getByRole('button', { name: 'Open System Settings' }))
    await userEvent.click(screen.getByRole('button', { name: 'Restart xcreenshot' }))
    expect(calls).toEqual(['permission_granted', 'open_permission_settings', 'restart_app'])
  })

  it('logs a command that fails', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    fail = 'open_permission_settings'
    await mount()
    await userEvent.click(screen.getByRole('button', { name: 'Open System Settings' }))
    await waitFor(() => expect(error).toHaveBeenCalledOnce())
    expect(error.mock.calls[0][0]).toBe('open_permission_settings failed')
  })

  it('shows the shortcut once the permission is granted', async () => {
    await mount()
    await emit('permission:granted')
    expect(await screen.findByRole('heading', { name: 'All set.' })).toBeInTheDocument()
    expect(screen.getByText(/to take a screenshot/)).toBeInTheDocument()
    for (const key of shortcutKeys(settings.shortcut)) {
      expect(screen.getByText(key)).toBeInTheDocument()
    }
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('shows the shortcut right away if already granted', async () => {
    grantedNow = true
    await mount()
    expect(await screen.findByRole('heading', { name: 'All set.' })).toBeInTheDocument()
  })

  it('stays done if the check answers after the grant', async () => {
    let answer: ((granted: boolean) => void) | undefined
    const pending = new Promise<boolean>((resolve) => (answer = resolve))
    mockIPC(
      (cmd) => {
        calls.push(cmd)
        if (cmd === 'permission_granted') return pending
      },
      { shouldMockEvents: true },
    )
    await mount()
    await emit('permission:granted')
    answer?.(false)
    await tick()
    expect(screen.getByRole('heading', { name: 'All set.' })).toBeInTheDocument()
  })
})
