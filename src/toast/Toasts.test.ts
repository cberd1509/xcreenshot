import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { FADE, SAVED_SHOWN, SHOWN, type ToastEvent } from './stack'
import Toasts from './Toasts.svelte'

let missed: ToastEvent[]
let idle: number[]
let failing: boolean
/** `toast_clickable` and `toast_action` calls, in order. */
let calls: unknown[]

const copied = (seq: number): ToastEvent => ({ seq, kind: 'copied', width: 1260, height: 1064 })
const toasts = () => screen.queryAllByRole('status')
// jsdom is not a Mac: the reveal button says Show in folder (`reveal` in `stack.ts`).
/** Each toast's words, a space between each piece of text (the buttons have none between). */
const texts = () =>
  toasts().map((t) => {
    const walker = document.createTreeWalker(t, NodeFilter.SHOW_TEXT)
    const words: string[] = []
    while (walker.nextNode()) words.push(walker.currentNode.textContent!.trim())
    return words.filter(Boolean).join(' ')
  })
/** Lets the mocked IPC and events settle, with the clock faked. */
const settle = async () => {
  await vi.advanceTimersByTimeAsync(0)
  await vi.advanceTimersByTimeAsync(0)
  await vi.advanceTimersByTimeAsync(0)
}

async function mount() {
  render(Toasts)
  await settle()
}

describe('Toasts', () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] })
    missed = []
    idle = []
    failing = false
    calls = []
    mockWindows('toast')
    mockIPC(
      (cmd, args) => {
        if (failing) throw new Error('nope')
        if (cmd === 'toast_ready') return missed
        if (cmd === 'toast_idle') idle.push((args as { seq: number }).seq)
        if (cmd === 'toast_clickable' || cmd === 'toast_action') calls.push({ [cmd]: args })
      },
      { shouldMockEvents: true },
    )
  })

  afterEach(async () => {
    cleanup()
    await settle()
    vi.useRealTimers()
    clearMocks()
    vi.restoreAllMocks()
  })

  it('shows the toasts sent while it loaded, then fades them and says so', async () => {
    missed = [copied(1), { seq: 2, kind: 'saved', file: 'xcreenshot-2026-11-06-23.12.05.png' }]
    await mount()
    expect(texts()).toEqual([
      'Copied 1260 × 1064',
      'Saved xcreenshot-2026-11-06-23.12.05.png Open Show in folder',
    ])
    // The older one is smaller and dimmer.
    expect(toasts()[0].style.transform).toBe('scale(0.97)')
    expect(toasts()[0].style.opacity).toBe('0.8')
    expect(toasts()[1].style.opacity).toBe('1')
    // Saved stays longer than the others.
    await vi.advanceTimersByTimeAsync(SHOWN)
    expect(toasts().map((t) => t.style.opacity)).toEqual(['0', '1'])
    await vi.advanceTimersByTimeAsync(FADE)
    expect(texts()).toEqual(['Saved xcreenshot-2026-11-06-23.12.05.png Open Show in folder'])
    await vi.advanceTimersByTimeAsync(SAVED_SHOWN - SHOWN - FADE)
    expect(toasts()[0].style.opacity).toBe('0')
    expect(idle).toEqual([])
    await vi.advanceTimersByTimeAsync(FADE)
    expect(toasts()).toEqual([])
    expect(idle).toEqual([2])
    // The window took clicks only while the Saved toast was up.
    expect(calls).toEqual([{ toast_clickable: { on: true } }, { toast_clickable: { on: false } }])
  })

  it('opens the saved file or shows it in its folder, and the toast goes', async () => {
    await mount()
    await emit('toast:show', { seq: 1, kind: 'saved', file: 'a.png' })
    await emit('toast:show', copied(2))
    await settle()
    const open = screen.getByRole('button', { name: 'Open' })
    const reveal = screen.getByRole('button', { name: 'Show in folder' })
    // Never focused, by click or by keyboard.
    expect(open.tabIndex).toBe(-1)
    expect(reveal.tabIndex).toBe(-1)
    expect(await fireEvent.mouseDown(reveal)).toBe(false)
    await fireEvent.click(reveal)
    await settle()
    expect(calls).toEqual([
      { toast_clickable: { on: true } },
      { toast_action: { seq: 1, action: 'reveal' } },
      { toast_clickable: { on: false } },
    ])
    expect(toasts()[0].style.opacity).toBe('0')
    await vi.advanceTimersByTimeAsync(FADE)
    expect(texts()).toEqual(['Copied 1260 × 1064'])

    await emit('toast:show', { seq: 3, kind: 'saved', file: 'b.png' })
    await settle()
    calls = []
    await fireEvent.click(screen.getByRole('button', { name: 'Open' }))
    await settle()
    expect(calls).toEqual([
      { toast_action: { seq: 3, action: 'open' } },
      { toast_clickable: { on: false } },
    ])
  })

  it('keeps a Saved toast up while the mouse is on it', async () => {
    missed = [{ seq: 1, kind: 'saved', file: 'a.png' }]
    await mount()
    await fireEvent.mouseEnter(toasts()[0])
    await vi.advanceTimersByTimeAsync(SAVED_SHOWN * 3)
    expect(toasts()[0].style.opacity).toBe('1')
    expect(idle).toEqual([])
    // Leaving restarts its time.
    await fireEvent.mouseLeave(toasts()[0])
    await vi.advanceTimersByTimeAsync(SAVED_SHOWN - 1)
    expect(toasts()[0].style.opacity).toBe('1')
    await vi.advanceTimersByTimeAsync(1 + FADE)
    expect(toasts()).toEqual([])
    expect(idle).toEqual([1])
  })

  it('lets the mouse through when no Saved toast is up', async () => {
    await mount()
    await emit('toast:show', copied(1))
    await settle()
    await fireEvent.mouseEnter(toasts()[0])
    await vi.advanceTimersByTimeAsync(SHOWN + FADE)
    expect(toasts()).toEqual([])
    expect(calls).toEqual([])
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('stacks new toasts and counts a repeat', async () => {
    await mount()
    expect(toasts()).toEqual([])
    await emit('toast:show', { seq: 1, kind: 'saved', file: 'a.png' })
    await emit('toast:show', { seq: 2, kind: 'shared', service: 'AirDrop' })
    await emit('toast:show', copied(3))
    await emit('toast:show', copied(4))
    await emit('toast:show', copied(5))
    await settle()
    expect(texts()).toEqual([
      'Saved a.png Open Show in folder',
      'Shared AirDrop',
      'Copied 1260 × 1064 ×3',
    ])
    expect(toasts()[0].style.transform).toBe('scale(0.94)')
    expect(toasts()[0].style.opacity).toBe('0.55')
    // A toast it already has is not shown twice.
    await emit('toast:show', copied(5))
    await settle()
    expect(texts()[2]).toBe('Copied 1260 × 1064 ×3')
  })

  it('keeps the toasts it missed when a newer one arrives first', async () => {
    let answer: ((missed: ToastEvent[]) => void) | undefined
    mockIPC(
      (cmd) => {
        if (cmd === 'toast_ready') return new Promise((resolve) => (answer = resolve))
      },
      { shouldMockEvents: true },
    )
    render(Toasts)
    await settle()
    await emit('toast:show', { seq: 2, kind: 'saved', file: 'a.png' })
    await settle()
    answer!([copied(1), { seq: 2, kind: 'saved', file: 'a.png' }])
    await settle()
    // In the order of the actions: the missed copy came first.
    expect(texts()).toEqual(['Copied 1260 × 1064', 'Saved a.png Open Show in folder'])
  })

  it('shows Shared alone when the service is not known', async () => {
    await mount()
    await emit('toast:show', { seq: 1, kind: 'shared', service: null })
    await settle()
    expect(texts()).toEqual(['Shared'])
  })

  it('drops the toasts when a capture starts', async () => {
    await mount()
    await emit('toast:show', copied(1))
    await settle()
    await emit('toast:clear')
    await settle()
    expect(toasts()).toEqual([])
    await vi.advanceTimersByTimeAsync(SHOWN + FADE)
    // Rust hid the window already.
    expect(idle).toEqual([])
  })

  it('logs when Rust cannot be reached', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    failing = true
    await mount()
    expect(error).toHaveBeenCalledWith('could not get the toasts', expect.any(Error))
    failing = false
    await emit('toast:show', copied(1))
    await settle()
    failing = true
    await vi.advanceTimersByTimeAsync(SHOWN + FADE)
    expect(error).toHaveBeenCalledTimes(2)
  })
})
