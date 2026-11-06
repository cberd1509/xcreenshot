import { cleanup, render, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Overlay from './Overlay.svelte'

type Info = { session: number; width: number; height: number }

const putImageData = vi.fn<(image: ImageData, x: number, y: number) => void>()
let calls: { cmd: string; args: unknown }[]
let ready: Info | null
let frame: (args: { session: number }) => unknown

const rgba = (width: number, height: number) => new Uint8Array(width * height * 4).fill(7).buffer
const called = (cmd: string) => calls.filter((c) => c.cmd === cmd)

/** Renders and waits until the show listener is registered and the load check ran. */
async function mount() {
  const view = render(Overlay)
  await waitFor(() => expect(called('overlay_ready')).toHaveLength(1))
  return view
}

const tick = () => new Promise((r) => setTimeout(r, 0))

describe('Overlay', () => {
  beforeEach(() => {
    calls = []
    ready = null
    frame = () => rgba(4, 2)
    mockWindows('overlay-1')
    mockIPC(
      (cmd, args) => {
        calls.push({ cmd, args })
        if (cmd === 'overlay_ready') return ready
        if (cmd === 'overlay_frame') return frame(args as { session: number })
      },
      { shouldMockEvents: true },
    )
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
      putImageData,
    } as unknown as CanvasRenderingContext2D)
  })

  afterEach(async () => {
    // Unmount and let it unlisten through the mocked IPC before the mocks go away.
    cleanup()
    await tick()
    clearMocks()
    vi.restoreAllMocks()
    putImageData.mockReset()
  })

  it('draws the frame on show, then reports the paint', async () => {
    const { container } = await mount()
    expect(called('overlay_frame')).toHaveLength(0)

    await emit('overlay:show', { session: 3, width: 4, height: 2 })
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    expect(called('overlay_frame')[0].args).toEqual({ session: 3 })
    expect(called('overlay_painted')[0].args).toEqual({ session: 3 })
    const canvas = container.querySelector('canvas')!
    expect([canvas.width, canvas.height]).toEqual([4, 2])
    const [image, x, y] = putImageData.mock.calls[0]
    expect([image.width, image.height, image.data.length, x, y]).toEqual([4, 2, 32, 0, 0])
  })

  it('draws a frame sent while it was loading', async () => {
    ready = { session: 5, width: 4, height: 2 }
    await mount()
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    // The show event for the same session does not fetch again.
    await emit('overlay:show', ready)
    expect(called('overlay_frame')).toHaveLength(1)
  })

  it('accepts the frame as a byte view', async () => {
    frame = () => new Uint8Array(rgba(4, 2))
    await mount()
    await emit('overlay:show', { session: 1, width: 4, height: 2 })
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
  })

  it('does nothing when the session is already gone', async () => {
    frame = () => new ArrayBuffer(0)
    await mount()
    await emit('overlay:show', { session: 1, width: 4, height: 2 })
    await waitFor(() => expect(called('overlay_frame')).toHaveLength(1))
    expect(putImageData).not.toHaveBeenCalled()
    expect(called('overlay_painted')).toHaveLength(0)
    expect(called('close_overlay')).toHaveLength(0)
  })

  it('drops an older frame when a newer session arrives first', async () => {
    let respond!: (buffer: ArrayBuffer) => void
    frame = ({ session }) =>
      session === 1 ? new Promise<ArrayBuffer>((r) => (respond = r)) : rgba(4, 2)
    await mount()
    await emit('overlay:show', { session: 1, width: 4, height: 2 })
    await emit('overlay:show', { session: 2, width: 4, height: 2 })
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    respond(rgba(4, 2))
    await tick()
    expect(called('overlay_painted').map((c) => c.args)).toEqual([{ session: 2 }])
  })

  it('closes the overlay when the frame cannot be drawn', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    await mount()
    // Wrong size for the pixels: ImageData throws.
    await emit('overlay:show', { session: 1, width: 5, height: 5 })
    await waitFor(() => expect(called('close_overlay')).toHaveLength(1))
    expect(error).toHaveBeenCalled()
    expect(called('overlay_painted')).toHaveLength(0)
  })

  it('frees the frame when the window is hidden', async () => {
    const { container } = await mount()
    await emit('overlay:show', { session: 1, width: 4, height: 2 })
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    const canvas = container.querySelector('canvas')!
    const hidden = vi.spyOn(document, 'hidden', 'get')
    hidden.mockReturnValue(false)
    document.dispatchEvent(new Event('visibilitychange'))
    expect(canvas.width).toBe(4)
    hidden.mockReturnValue(true)
    document.dispatchEvent(new Event('visibilitychange'))
    expect([canvas.width, canvas.height]).toEqual([0, 0])
  })

  it('closes on Esc only', async () => {
    await mount()
    await userEvent.keyboard('a')
    expect(called('close_overlay')).toHaveLength(0)
    await userEvent.keyboard('{Escape}')
    expect(called('close_overlay')).toHaveLength(1)
  })

  it('stops listening when unmounted', async () => {
    const { unmount } = await mount()
    unmount()
    await tick()
    await emit('overlay:show', { session: 1, width: 4, height: 2 })
    expect(called('overlay_frame')).toHaveLength(0)
  })
})
