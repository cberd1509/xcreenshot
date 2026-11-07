import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PAINT_TIMEOUT_MS, afterPaint } from './paint'

describe('afterPaint', () => {
  let frames: FrameRequestCallback[]
  const real = globalThis.requestAnimationFrame

  /** Runs the callbacks waiting for the next frame, as the browser does once per frame. */
  function nextFrame() {
    const now = frames
    frames = []
    now.forEach((callback) => callback(0))
  }

  beforeEach(() => {
    frames = []
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    globalThis.requestAnimationFrame = (callback) => frames.push(callback)
  })

  afterEach(() => {
    vi.useRealTimers()
    globalThis.requestAnimationFrame = real
  })

  it('resolves once a whole frame has been rendered, not at the next one', async () => {
    let done: boolean | undefined
    void afterPaint().then((rendered) => (done = rendered))
    nextFrame()
    await Promise.resolve()
    expect(done, 'the first frame may not have rendered yet').toBeUndefined()
    nextFrame()
    await Promise.resolve()
    expect(done).toBe(true)
    // The timeout was cleared, so it never resolves twice.
    expect(vi.getTimerCount()).toBe(0)
  })

  it('gives up when no frame comes, so a webview that renders nothing still opens', async () => {
    let done: boolean | undefined
    void afterPaint().then((rendered) => (done = rendered))
    nextFrame()
    await vi.advanceTimersByTimeAsync(PAINT_TIMEOUT_MS - 1)
    expect(done).toBeUndefined()
    await vi.advanceTimersByTimeAsync(1)
    expect(done).toBe(false)
  })
})
