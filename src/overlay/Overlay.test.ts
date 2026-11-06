import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Overlay from './Overlay.svelte'

type Info = { session: number; width: number; height: number }

const putImageData = vi.fn<(image: ImageData, x: number, y: number) => void>()
const drawImage = vi.fn<(...args: unknown[]) => void>()
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

// jsdom's window is 1024 x 768; a 2048 x 1536 frame makes it a 2x display.
const info = { session: 9, width: 2048, height: 1536 }
const overlay = () => screen.getByTestId('overlay')
const selection = () => screen.queryByTestId('selection')
const box = () => {
  const s = selection()!.style
  return [s.left, s.top, s.width, s.height].map((v) => parseFloat(v))
}
const selections = () => called('overlay_selection').map((c) => c.args)

type At = { x: number; y: number }
const point = ({ x, y }: At) => ({ clientX: x, clientY: y, button: 0, pointerId: 1 })

function drag(from: At, to: At, target: Element = overlay()) {
  fireEvent.pointerDown(target, point(from))
  fireEvent.pointerMove(overlay(), point(to))
  fireEvent.pointerUp(overlay(), point(to))
}

async function shown(props: { magnifier?: boolean } = {}) {
  frame = () => rgba(info.width, info.height)
  const view = render(Overlay, { props })
  await waitFor(() => expect(called('overlay_ready')).toHaveLength(1))
  await emit('overlay:show', info)
  await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
  return view
}

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
      drawImage,
      fillRect: vi.fn<(...args: unknown[]) => void>(),
      strokeRect: vi.fn<(...args: unknown[]) => void>(),
    } as unknown as CanvasRenderingContext2D)
  })

  afterEach(async () => {
    // Unmount and let it unlisten through the mocked IPC before the mocks go away.
    cleanup()
    await tick()
    clearMocks()
    vi.restoreAllMocks()
    putImageData.mockReset()
    drawImage.mockReset()
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

  describe('selection', () => {
    it('draws a selection in any direction and shows its size in real pixels', async () => {
      await shown()
      expect(selection()).toBeNull()
      drag({ x: 110, y: 70 }, { x: 10, y: 20 })
      expect(box()).toEqual([10, 20, 100, 50])
      expect(screen.getByTestId('size-badge')).toHaveTextContent('200 × 100')
      expect(selections()).toEqual([{ session: 9, active: true }])
    })

    it('ignores a click without a drag and other buttons', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 10, y: 10 })
      expect(selection()).toBeNull()
      fireEvent.pointerDown(overlay(), { ...point({ x: 1, y: 1 }), button: 2 })
      fireEvent.pointerMove(overlay(), point({ x: 50, y: 50 }))
      expect(selection()).toBeNull()
      expect(selections()).toEqual([])
    })

    it('moves when dragged inside and starts over when dragged outside', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      drag({ x: 50, y: 30 }, { x: 60, y: 25 }, selection()!)
      expect(box()).toEqual([20, 5, 100, 50])
      drag({ x: 500, y: 500 }, { x: 520, y: 530 })
      expect(box()).toEqual([500, 500, 20, 30])
      expect(selections()).toEqual([
        { session: 9, active: true },
        { session: 9, active: false },
        { session: 9, active: true },
      ])
    })

    it('resizes with the handles', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      const handle = (h: string) => selection()!.querySelector(`[data-handle="${h}"]`)!
      drag({ x: 110, y: 60 }, { x: 130, y: 90 }, handle('se'))
      expect(box()).toEqual([10, 10, 120, 80])
      drag({ x: 10, y: 10 }, { x: 0, y: 0 }, handle('nw'))
      expect(box()).toEqual([0, 0, 130, 90])
      // Collapsing it to nothing clears it.
      drag({ x: 130, y: 0 }, { x: 0, y: 0 }, handle('e'))
      expect(selection()).toBeNull()
    })

    it('uses the drag cursor everywhere while dragging', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      const handle = selection()!.querySelector('[data-handle="n"]')!
      fireEvent.pointerDown(handle, point({ x: 60, y: 10 }))
      expect(overlay().style.cursor).toBe('ns-resize')
      fireEvent.pointerCancel(overlay())
      expect(overlay().style.cursor).toBe('')
      fireEvent.pointerDown(selection()!, point({ x: 60, y: 30 }))
      expect(overlay().style.cursor).toBe('move')
    })

    it('nudges with the arrow keys', async () => {
      await shown()
      await userEvent.keyboard('{ArrowRight}')
      expect(selection()).toBeNull()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('{ArrowRight}{ArrowDown}')
      expect(box()).toEqual([11, 11, 100, 50])
      await userEvent.keyboard('{Shift>}{ArrowLeft}{ArrowUp}{/Shift}')
      expect(box()).toEqual([1, 1, 100, 50])
      await userEvent.keyboard('a')
      expect(box()).toEqual([1, 1, 100, 50])
    })

    it('clears the selection on Esc, then closes', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('{Escape}')
      expect(selection()).toBeNull()
      expect(selections().at(-1)).toEqual({ session: 9, active: false })
      expect(called('close_overlay')).toHaveLength(0)
      await userEvent.keyboard('{Escape}')
      expect(called('close_overlay')).toHaveLength(1)
    })

    it('clears the selection when Rust says so', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await emit('overlay:clear')
      await waitFor(() => expect(selection()).toBeNull())
    })

    it('confirms the selection on Enter', async () => {
      await shown()
      await userEvent.keyboard('{Enter}')
      expect(called('overlay_confirm')).toHaveLength(0)
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('{Enter}')
      expect(called('overlay_confirm')[0].args).toEqual({
        session: 9,
        rect: { x: 10, y: 10, width: 100, height: 50 },
      })
    })

    it('logs a confirm that fails', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      await shown()
      mockIPC((cmd) => {
        if (cmd === 'overlay_confirm') throw new Error('empty')
      })
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('{Enter}')
      await waitFor(() => expect(error).toHaveBeenCalled())
    })

    it('sets an exact size from the badge', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.click(screen.getByRole('button', { name: /200 × 100/ }))
      const width = screen.getByLabelText('Width')
      await userEvent.clear(width)
      // Typing does not nudge or confirm the selection.
      await userEvent.type(width, '300{ArrowLeft}{Enter}')
      expect(box()).toEqual([10, 10, 150, 50])
      expect(called('overlay_confirm')).toHaveLength(0)
      // Clicks on the badge do not start a new selection.
      fireEvent.pointerDown(screen.getByTestId('size-badge'), point({ x: 20, y: 5 }))
      expect(box()).toEqual([10, 10, 150, 50])
    })

    it('shows the loupe while drawing only when enabled', async () => {
      await shown({ magnifier: true })
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 20, y: 30 }))
      expect(screen.getByTestId('magnifier')).toHaveTextContent('40, 60')
      expect(screen.getByTestId('magnifier')).toHaveTextContent('#070707')
      fireEvent.pointerUp(overlay())
      expect(screen.queryByTestId('magnifier')).toBeNull()
      // Not while moving.
      fireEvent.pointerDown(selection()!, point({ x: 15, y: 15 }))
      expect(screen.queryByTestId('magnifier')).toBeNull()
    })

    it('follows the magnifier setting by default (off)', async () => {
      await shown()
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 20, y: 30 }))
      expect(screen.queryByTestId('magnifier')).toBeNull()
    })

    it('drops the selection when hidden and on the next capture', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true)
      document.dispatchEvent(new Event('visibilitychange'))
      await waitFor(() => expect(selection()).toBeNull())
      hidden.mockReturnValue(false)
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await emit('overlay:show', { ...info, session: 10 })
      await waitFor(() => expect(selection()).toBeNull())
    })
  })
})
