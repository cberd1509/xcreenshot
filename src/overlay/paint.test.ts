import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fakeFrames } from '../test/frames'
import { PAINT_TIMEOUT_MS, afterPaint } from './paint'

describe('afterPaint', () => {
  let frames: ReturnType<typeof fakeFrames>

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    frames = fakeFrames()
  })

  afterEach(() => {
    vi.useRealTimers()
    frames.restore()
  })

  it('resolves once a whole frame has been rendered, not at the next one', async () => {
    let done: boolean | undefined
    void afterPaint().then((rendered) => (done = rendered))
    frames.next()
    await Promise.resolve()
    expect(done, 'the first frame may not have rendered yet').toBeUndefined()
    frames.next()
    await Promise.resolve()
    expect(done).toBe(true)
    // The timeout was cleared, so it never resolves twice.
    expect(vi.getTimerCount()).toBe(0)
  })

  it('gives up when no frame comes, so a webview that renders nothing still opens', async () => {
    let done: boolean | undefined
    void afterPaint().then((rendered) => (done = rendered))
    frames.next()
    await vi.advanceTimersByTimeAsync(PAINT_TIMEOUT_MS - 1)
    expect(done).toBeUndefined()
    await vi.advanceTimersByTimeAsync(1)
    expect(done).toBe(false)
  })
})
