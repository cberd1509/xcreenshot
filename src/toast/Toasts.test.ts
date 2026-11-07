import { cleanup, render, screen } from '@testing-library/svelte'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { FADE, SHOWN, type ToastEvent } from './stack'
import Toasts from './Toasts.svelte'

let missed: ToastEvent[]
let idle: number[]
let failing: boolean

const copied = (seq: number): ToastEvent => ({ seq, kind: 'copied', width: 1260, height: 1064 })
const toasts = () => screen.queryAllByRole('status')
const texts = () => toasts().map((t) => t.textContent!.replace(/\s+/g, ' ').trim())
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
    mockWindows('toast')
    mockIPC(
      (cmd, args) => {
        if (failing) throw new Error('nope')
        if (cmd === 'toast_ready') return missed
        if (cmd === 'toast_idle') idle.push((args as { seq: number }).seq)
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
    expect(texts()).toEqual(['Copied 1260 × 1064', 'Saved xcreenshot-2026-11-06-23.12.05.png'])
    // The older one is smaller and dimmer.
    expect(toasts()[0].style.transform).toBe('scale(0.97)')
    expect(toasts()[0].style.opacity).toBe('0.8')
    expect(toasts()[1].style.opacity).toBe('1')
    await vi.advanceTimersByTimeAsync(SHOWN)
    expect(toasts().map((t) => t.style.opacity)).toEqual(['0', '0'])
    expect(idle).toEqual([])
    await vi.advanceTimersByTimeAsync(FADE)
    expect(toasts()).toEqual([])
    expect(idle).toEqual([2])
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
    expect(texts()).toEqual(['Saved a.png', 'Shared AirDrop', 'Copied 1260 × 1064 ×3'])
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
    expect(texts()).toEqual(['Copied 1260 × 1064', 'Saved a.png'])
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
