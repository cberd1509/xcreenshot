import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { tick } from '../../test/tick'
import ShortcutRecorder from './ShortcutRecorder.svelte'

/** The platform the recorder sees. */
const platform = vi.hoisted(() => ({ windows: false }))
vi.mock('$lib/keys', async (importOriginal) => {
  const keys = await importOriginal<typeof import('$lib/keys')>()
  return {
    ...keys,
    comboProblem: (modifiers: string[], key: string) =>
      keys.comboProblem(modifiers, key, platform.windows),
  }
})

const PAUSE = 'pause_capture_shortcut'
const RESUME = 'resume_capture_shortcut'

/** IPC commands and saves, in order. */
let calls: string[]
let failing: Set<string>
/** While set, pausing waits for it. */
let pausing: Promise<void> | null
let onchange: ReturnType<typeof vi.fn<(a: string) => Promise<void>>>
let onreset: ReturnType<typeof vi.fn<() => Promise<void>>>

const button = (name: string) => screen.getByRole('button', { name })
/** The key-cap button, named for the current shortcut while not recording. */
const changeButton = () => screen.getByRole('button', { name: /^Change shortcut/ })
const down = (code: string, mods: KeyboardEventInit = {}) =>
  fireEvent.keyDown(window, { code, ...mods })
/** Releases a key and lets the save go through. */
const up = async (code: string, mods: KeyboardEventInit = {}) => {
  await fireEvent.keyUp(window, { code, ...mods })
  await tick()
}

function setup(accelerator = 'Control+Shift+Super+S') {
  return render(ShortcutRecorder, {
    title: 'Take a screenshot',
    description: 'Works from any app.',
    accelerator,
    onchange,
    onreset,
  })
}

/** Clicks Change and waits for the shortcut to be let go. */
async function change() {
  await userEvent.click(changeButton())
  await tick()
}

async function record() {
  setup()
  await change()
}

describe('ShortcutRecorder', () => {
  beforeEach(() => {
    platform.windows = false
    calls = []
    failing = new Set()
    pausing = null
    mockIPC(async (cmd) => {
      calls.push(cmd)
      if (cmd === PAUSE && pausing) await pausing
      if (failing.has(cmd)) throw new Error('nope')
    })
    onchange = vi.fn<(a: string) => Promise<void>>(() => {
      calls.push('save')
      return Promise.resolve()
    })
    onreset = vi.fn<() => Promise<void>>(() => Promise.resolve())
  })

  afterEach(() => {
    cleanup()
    clearMocks()
    vi.restoreAllMocks()
  })

  it('shows the shortcut as keys in a button named for it, and Reset to default', () => {
    setup('Alt+F5')
    expect(screen.getByText('Take a screenshot')).toBeInTheDocument()
    expect(screen.getByText('Works from any app.')).toBeInTheDocument()
    expect(changeButton()).toHaveAccessibleName('Change shortcut, currently Alt F5')
    expect(changeButton()).toHaveTextContent('F5')
    expect(button('Reset to default')).toBeInTheDocument()
  })

  it('ignores keys until Change is pressed', async () => {
    setup()
    await down('KeyD', { altKey: true })
    await up('KeyD', { altKey: true })
    expect(onchange).not.toHaveBeenCalled()
    expect(calls).toEqual([])
  })

  it('shows the keys as they are pressed and saves on key-up', async () => {
    await record()
    expect(screen.getByText('Type a shortcut')).toBeInTheDocument()
    await down('AltLeft', { altKey: true })
    expect(screen.queryByText('Type a shortcut')).toBeNull()
    await down('ShiftLeft', { altKey: true, shiftKey: true })
    await down('KeyD', { altKey: true, shiftKey: true })
    expect(screen.getByText('D')).toBeInTheDocument()
    expect(onchange).not.toHaveBeenCalled()
    await up('KeyD', { altKey: true, shiftKey: true })
    expect(onchange).toHaveBeenCalledExactlyOnceWith('Alt+Shift+D')
    expect(changeButton()).toBeInTheDocument()
  })

  it('lets go of the capture shortcut while recording and takes it back before saving', async () => {
    await record()
    expect(calls).toEqual([PAUSE])
    await down('KeyS', { ctrlKey: true, shiftKey: true, metaKey: true })
    await up('KeyS', { ctrlKey: true, shiftKey: true, metaKey: true })
    expect(calls).toEqual([PAUSE, RESUME, 'save'])
    expect(onchange).toHaveBeenCalledWith('Control+Shift+Super+S')
  })

  it('saves on Enter too', async () => {
    await record()
    await down('KeyD', { ctrlKey: true })
    await down('Enter')
    await tick()
    expect(onchange).toHaveBeenCalledExactlyOnceWith('Control+D')
  })

  it('cancels on Esc, Cancel, the key button or leaving the window, taking the shortcut back each time', async () => {
    await record()
    await down('KeyD', { ctrlKey: true })
    await down('Escape')
    expect(changeButton()).toBeInTheDocument()
    await change()
    expect(screen.queryByRole('button', { name: 'Reset to default' })).toBeNull()
    await userEvent.click(button('Cancel'))
    expect(changeButton()).toBeInTheDocument()
    await change()
    await userEvent.click(button('Recording a shortcut, press to cancel'))
    expect(changeButton()).toBeInTheDocument()
    await change()
    await fireEvent.blur(window)
    expect(changeButton()).toBeInTheDocument()
    await fireEvent.blur(window) // Not recording: nothing to take back.
    expect(onchange).not.toHaveBeenCalled()
    expect(calls).toEqual([PAUSE, RESUME, PAUSE, RESUME, PAUSE, RESUME, PAUSE, RESUME])
  })

  it('takes the shortcut back if the page goes away mid-recording', async () => {
    await record()
    cleanup()
    await tick()
    expect(calls).toEqual([PAUSE, RESUME])
  })

  it('stops and says so when the shortcut cannot be let go', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    failing.add(PAUSE)
    await record()
    await tick()
    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't change the shortcut.")
    expect(changeButton()).toBeInTheDocument()
    await down('KeyD', { ctrlKey: true })
    await up('KeyD', { ctrlKey: true })
    expect(onchange).not.toHaveBeenCalled()
    expect(calls).toEqual([PAUSE])
    expect(error).toHaveBeenCalledWith(`${PAUSE} failed`, expect.any(Error))
  })

  it('logs when the shortcut cannot be taken back', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    failing.add(RESUME)
    await record()
    await down('Escape')
    await tick()
    expect(error.mock.calls.map((c) => c[0])).toEqual([`${RESUME} failed`])
  })

  it('asks for a modifier and keeps recording', async () => {
    await record()
    await down('KeyD')
    expect(screen.getByRole('alert')).toHaveTextContent('Add ⌘, ⌃ or ⌥.')
    await up('KeyD')
    await down('Enter')
    expect(onchange).not.toHaveBeenCalled()
    expect(button('Cancel')).toBeInTheDocument()
    // Modifiers released on their own go back to waiting.
    await down('ControlLeft', { ctrlKey: true })
    await up('ControlLeft')
    expect(screen.getByText('Type a shortcut')).toBeInTheDocument()
    await down('KeyD', { ctrlKey: true })
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('takes bare PrintScreen on Windows, which only sends key-up', async () => {
    platform.windows = true
    await record()
    await up('PrintScreen')
    expect(onchange).toHaveBeenCalledExactlyOnceWith('PrintScreen')
  })

  it('shows why a shortcut was refused', async () => {
    onchange.mockRejectedValueOnce('That shortcut is in use by another app.')
    await record()
    await down('KeyD', { ctrlKey: true })
    await up('KeyD', { ctrlKey: true })
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'That shortcut is in use by another app.',
    )
    onchange.mockRejectedValueOnce(new Error('boom'))
    await change()
    expect(screen.queryByRole('alert')).toBeNull()
    await down('KeyD', { ctrlKey: true })
    await up('KeyD', { ctrlKey: true })
    expect(await screen.findByRole('alert')).toHaveTextContent("Couldn't change the shortcut.")
  })

  it('resets to the default', async () => {
    setup()
    await userEvent.click(button('Reset to default'))
    expect(onreset).toHaveBeenCalledOnce()
    expect(calls).toEqual([])
  })

  it('records only once the shortcut is let go', async () => {
    let release = Function.prototype as () => void
    pausing = new Promise((r) => (release = r))
    setup()
    await userEvent.click(changeButton())
    expect(changeButton()).toBeDisabled()
    expect(button('Reset to default')).toBeDisabled()
    // Pressed before it was let go: the capture shortcut takes it, not the recorder.
    await down('KeyD', { ctrlKey: true })
    await up('KeyD', { ctrlKey: true })
    expect(onchange).not.toHaveBeenCalled()
    release()
    await tick()
    expect(screen.getByText('Type a shortcut')).toBeInTheDocument()
  })

  it('takes the shortcut back if the window is left while letting go', async () => {
    let release = Function.prototype as () => void
    pausing = new Promise((r) => (release = r))
    setup()
    await userEvent.click(changeButton())
    await fireEvent.blur(window)
    release()
    await tick()
    expect(calls).toEqual([PAUSE, RESUME])
    expect(changeButton()).not.toBeDisabled()
  })

  it('keeps the buttons waiting until a save answers', async () => {
    let release = Function.prototype as () => void
    onreset.mockImplementationOnce(() => new Promise((r) => (release = r)))
    setup()
    await userEvent.click(button('Reset to default'))
    expect(changeButton()).toBeDisabled()
    expect(button('Reset to default')).toBeDisabled()
    release()
    await tick()
    expect(changeButton()).not.toBeDisabled()
  })
})
