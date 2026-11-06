import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { emit, listen } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { box, fakeContext } from '../test/canvas'
import Overlay from './Overlay.svelte'

type Info = {
  session: number
  width: number
  height: number
  logicalWidth: number
  logicalHeight: number
  escapeShortcut: boolean
}
type At = { x: number; y: number }

let calls: { cmd: string; args: unknown }[]
let ready: Info | null
let frame: (args: { session: number }) => unknown
let confirmed: () => unknown
let context: ReturnType<typeof fakeContext>
let selected: { label: string; active: boolean }[]

const rgba = (width: number, height: number) => new Uint8Array(width * height * 4).fill(7).buffer
const called = (cmd: string) => calls.filter((c) => c.cmd === cmd)
const tick = () => new Promise((r) => setTimeout(r, 0))
const small = (session: number): Info => ({
  session,
  width: 4,
  height: 2,
  logicalWidth: 2,
  logicalHeight: 1,
  escapeShortcut: true,
})
// A 2x display of 1000 x 700 points.
const info: Info = {
  session: 9,
  width: 2000,
  height: 1400,
  logicalWidth: 1000,
  logicalHeight: 700,
  escapeShortcut: true,
}
/** Esc from Rust, forwarded to this page. */
const esc = (token = 1) => emit('overlay:escape', { session: 9, token })
const answers = () => called('overlay_escaped').map((c) => c.args)

const overlay = () => screen.getByTestId('overlay')
const selection = () => screen.queryByTestId('selection')
const rect = () => box(selection()!)
const point = ({ x, y }: At) => ({ clientX: x, clientY: y, button: 0, pointerId: 1 })

function drag(from: At, to: At, target: Element = overlay()) {
  fireEvent.pointerDown(target, point(from))
  fireEvent.pointerMove(overlay(), point(to))
  fireEvent.pointerUp(overlay(), point(to))
}

/** Renders and waits until the listeners are registered and the load check ran. */
async function mount(props: { magnifier?: boolean } = {}) {
  const view = render(Overlay, { props })
  await waitFor(() => expect(called('overlay_ready')).toHaveLength(1))
  return view
}

/** Mounts and draws the 2x frame. */
async function shown(props: { magnifier?: boolean } = {}) {
  frame = () => rgba(info.width, info.height)
  const view = await mount(props)
  await emit('overlay:show', info)
  await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
  return view
}

describe('Overlay', () => {
  beforeEach(async () => {
    calls = []
    ready = null
    selected = []
    frame = () => rgba(4, 2)
    confirmed = () => null
    mockWindows('overlay-1')
    mockIPC(
      (cmd, args) => {
        calls.push({ cmd, args })
        if (cmd === 'overlay_ready') return ready
        if (cmd === 'overlay_frame') return frame(args as { session: number })
        if (cmd === 'overlay_confirm') return confirmed()
      },
      { shouldMockEvents: true },
    )
    context = fakeContext()
    await listen<{ label: string; active: boolean }>('overlay:selected', (e) =>
      selected.push(e.payload),
    )
  })

  afterEach(async () => {
    // Unmount and let it unlisten through the mocked IPC before the mocks go away.
    cleanup()
    await tick()
    clearMocks()
    vi.restoreAllMocks()
  })

  it('draws the frame on show, then reports the paint', async () => {
    const { container } = await mount()
    expect(called('overlay_frame')).toHaveLength(0)

    await emit('overlay:show', small(3))
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    expect(called('overlay_frame')[0].args).toEqual({ session: 3 })
    expect(called('overlay_painted')[0].args).toEqual({ session: 3 })
    const canvas = container.querySelector('canvas')!
    expect([canvas.width, canvas.height]).toEqual([4, 2])
    const [image, x, y] = context.putImageData.mock.calls[0]
    expect([image.width, image.height, image.data.length, x, y]).toEqual([4, 2, 32, 0, 0])
  })

  it('draws a frame sent while it was loading', async () => {
    ready = small(5)
    await mount()
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    // The show event for the same session does not fetch again.
    await emit('overlay:show', ready)
    expect(called('overlay_frame')).toHaveLength(1)
  })

  it('accepts the frame as a byte view', async () => {
    frame = () => new Uint8Array(rgba(4, 2))
    await mount()
    await emit('overlay:show', small(1))
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
  })

  it('does nothing when the session is already gone', async () => {
    frame = () => new ArrayBuffer(0)
    await mount()
    await emit('overlay:show', small(1))
    await waitFor(() => expect(called('overlay_frame')).toHaveLength(1))
    expect(context.putImageData).not.toHaveBeenCalled()
    expect(called('overlay_painted')).toHaveLength(0)
    expect(called('close_overlay')).toHaveLength(0)
    // Nothing drawn: no selection either.
    drag({ x: 0, y: 0 }, { x: 1, y: 1 })
    expect(selection()).toBeNull()
  })

  it('drops an older frame when a newer session arrives first', async () => {
    let respond!: (buffer: ArrayBuffer) => void
    frame = ({ session }) =>
      session === 1 ? new Promise<ArrayBuffer>((r) => (respond = r)) : rgba(4, 2)
    await mount()
    await emit('overlay:show', small(1))
    await emit('overlay:show', small(2))
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    respond(rgba(4, 2))
    await tick()
    expect(called('overlay_painted').map((c) => c.args)).toEqual([{ session: 2 }])
  })

  it('closes the overlay when the frame cannot be drawn', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    await mount()
    // Wrong size for the pixels: ImageData throws.
    await emit('overlay:show', { ...small(1), width: 5, height: 5 })
    await waitFor(() => expect(called('close_overlay')).toHaveLength(1))
    expect(called('close_overlay')[0].args).toEqual({ session: 1 })
    expect(error).toHaveBeenCalled()
    expect(called('overlay_painted')).toHaveLength(0)
  })

  it('frees the frame when the window is hidden', async () => {
    const { container } = await mount()
    await emit('overlay:show', small(1))
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

  it('ignores a show for an older session', async () => {
    await mount()
    await emit('overlay:show', small(3))
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    await emit('overlay:show', small(2))
    await tick()
    expect(called('overlay_frame').map((c) => c.args)).toEqual([{ session: 3 }])
  })

  it('leaves its own Esc key to Rust when Rust owns Esc', async () => {
    await mount()
    await emit('overlay:show', small(1))
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    await userEvent.keyboard('{Escape}')
    expect(called('overlay_escape')).toHaveLength(0)
    expect(called('close_overlay')).toHaveLength(0)
  })

  it('forwards its own Esc key when Rust has no Esc shortcut', async () => {
    await mount()
    await emit('overlay:show', { ...small(1), escapeShortcut: false })
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    await userEvent.keyboard('a{Escape}')
    expect(called('overlay_escape')).toHaveLength(1)
    expect(called('close_overlay')).toHaveLength(0)
  })

  it('stops listening when unmounted', async () => {
    const { unmount } = await mount()
    unmount()
    await tick()
    await emit('overlay:show', small(1))
    expect(called('overlay_frame')).toHaveLength(0)
  })

  describe('selection', () => {
    it('draws in any direction and shows its size in real pixels', async () => {
      await shown()
      expect(selection()).toBeNull()
      drag({ x: 110, y: 70 }, { x: 10, y: 20 })
      expect(rect()).toEqual([10, 20, 100, 50])
      expect(screen.getByTestId('size-badge')).toHaveTextContent('200 × 100')
      expect(selected).toEqual([{ label: 'overlay-1', active: true }])
    })

    it('focuses its window on press, so keys reach it', async () => {
      await shown()
      fireEvent.pointerDown(overlay(), point({ x: 5, y: 5 }))
      expect(called('plugin:window|set_focus')).toHaveLength(1)
    })

    it('logs a failed focus', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      await shown()
      mockIPC((cmd) => {
        if (cmd === 'plugin:window|set_focus') throw new Error('no window')
      })
      fireEvent.pointerDown(overlay(), point({ x: 5, y: 5 }))
      await waitFor(() => expect(error).toHaveBeenCalled())
    })

    it('ignores a click without a drag and other buttons', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 10, y: 10 })
      expect(selection()).toBeNull()
      fireEvent.pointerDown(overlay(), { ...point({ x: 1, y: 1 }), button: 2 })
      fireEvent.pointerMove(overlay(), point({ x: 50, y: 50 }))
      expect(selection()).toBeNull()
      expect(selected).toEqual([])
    })

    it('moves when dragged inside and starts over when dragged outside', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      drag({ x: 50, y: 30 }, { x: 60, y: 25 }, selection()!)
      expect(rect()).toEqual([20, 5, 100, 50])
      drag({ x: 500, y: 500 }, { x: 520, y: 530 })
      expect(rect()).toEqual([500, 500, 20, 30])
      expect(selected.map((s) => s.active)).toEqual([true, false, true])
    })

    it('resizes with the handles', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      const handle = (h: string) => selection()!.querySelector(`[data-handle="${h}"]`)!
      drag({ x: 110, y: 60 }, { x: 130, y: 90 }, handle('se'))
      expect(rect()).toEqual([10, 10, 120, 80])
      drag({ x: 10, y: 10 }, { x: 0, y: 0 }, handle('nw'))
      expect(rect()).toEqual([0, 0, 130, 90])
      // Collapsing it to nothing clears it.
      drag({ x: 130, y: 0 }, { x: 0, y: 0 }, handle('e'))
      expect(selection()).toBeNull()
    })

    it('stays on this display: clamps a drag past every edge', async () => {
      await shown()
      // The display is 1000 x 700 points; other displays are beyond each edge.
      drag({ x: 500, y: 350 }, { x: -50, y: -30 })
      expect(rect()).toEqual([0, 0, 500, 350])
      drag({ x: 500, y: 350 }, { x: 1200, y: 900 })
      expect(rect()).toEqual([500, 350, 500, 350])
      // Moving keeps the whole selection on the display.
      drag({ x: 600, y: 400 }, { x: 2000, y: 2000 }, selection()!)
      expect(rect()).toEqual([500, 350, 500, 350])
      drag({ x: 600, y: 400 }, { x: -2000, y: -2000 }, selection()!)
      expect(rect()).toEqual([0, 0, 500, 350])
      // Resizing too.
      const handle = (h: string) => selection()!.querySelector(`[data-handle="${h}"]`)!
      drag({ x: 500, y: 350 }, { x: 5000, y: 5000 }, handle('se'))
      expect(rect()).toEqual([0, 0, 1000, 700])
    })

    it('keeps the drag after the pointer leaves the window', async () => {
      await shown()
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 100, y: 100 }))
      fireEvent.pointerLeave(overlay(), point({ x: 1001, y: 100 }))
      // Captured moves keep coming from beyond the edge, then from inside again.
      fireEvent.pointerMove(overlay(), point({ x: 1500, y: 120 }))
      expect(rect()).toEqual([10, 10, 990, 110])
      fireEvent.pointerMove(overlay(), point({ x: 200, y: 120 }))
      expect(rect()).toEqual([10, 10, 190, 110])
      fireEvent.pointerUp(overlay(), point({ x: 200, y: 120 }))
      expect(rect()).toEqual([10, 10, 190, 110])
    })

    it('captures the pointer on press', async () => {
      await shown()
      const capture = vi.fn<(id: number) => void>()
      overlay().setPointerCapture = capture
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      expect(capture).toHaveBeenCalledWith(1)
    })

    it('does not select from moves it did not start', async () => {
      // A drag that started on another display moves over this one: nothing happens here.
      await shown()
      fireEvent.pointerMove(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 200, y: 200 }))
      fireEvent.pointerUp(overlay(), point({ x: 200, y: 200 }))
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

    it('nudges by physical pixels with the arrow keys', async () => {
      await shown()
      await userEvent.keyboard('{ArrowRight}')
      expect(selection()).toBeNull()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('{ArrowRight}{ArrowDown}')
      expect(rect()).toEqual([10.5, 10.5, 100, 50])
      await userEvent.keyboard('{Shift>}{ArrowLeft}{ArrowUp}{/Shift}')
      expect(rect()).toEqual([5.5, 5.5, 100, 50])
      await userEvent.keyboard('a')
      expect(rect()).toEqual([5.5, 5.5, 100, 50])
    })

    it('clears the selection on Esc, then says it had nothing to undo', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await esc(1)
      await waitFor(() => expect(selection()).toBeNull())
      expect(selected.at(-1)).toEqual({ label: 'overlay-1', active: false })
      await esc(2)
      await waitFor(() => expect(answers()).toHaveLength(2))
      expect(answers()).toEqual([
        { session: 9, token: 1, handled: true },
        { session: 9, token: 2, handled: false },
      ])
      // Rust decides whether to close; the page never does on Esc.
      expect(called('close_overlay')).toHaveLength(0)
    })

    it('Esc during a drag stops it for good', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      fireEvent.pointerDown(selection()!, point({ x: 50, y: 30 }))
      await esc(1)
      await waitFor(() => expect(selection()).toBeNull())
      fireEvent.pointerMove(overlay(), point({ x: 70, y: 40 }))
      expect(selection()).toBeNull()
      // A drag that has not drawn anything yet is undone too.
      fireEvent.pointerDown(overlay(), point({ x: 5, y: 5 }))
      await esc(2)
      await waitFor(() => expect(answers()).toHaveLength(2))
      expect(answers().map((a) => (a as { handled: boolean }).handled)).toEqual([true, true])
    })

    it('Esc cancels a size edit before the selection', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.click(screen.getByRole('button', { name: /200 × 100/ }))
      await esc()
      await waitFor(() => expect(screen.queryByLabelText('Width')).toBeNull())
      expect(selection()).not.toBeNull()
      expect(answers()).toEqual([{ session: 9, token: 1, handled: true }])
    })

    it('keeps one selection across displays', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      // Another display losing a selection changes nothing here.
      await emit('overlay:selected', { label: 'overlay-2', active: false })
      await tick()
      expect(selection()).not.toBeNull()
      await emit('overlay:selected', { label: 'overlay-2', active: true })
      await waitFor(() => expect(selection()).toBeNull())
      // Display 2 has the selection: this page has nothing to undo and says so.
      await esc()
      await waitFor(() => expect(answers()).toEqual([{ session: 9, token: 1, handled: false }]))
    })

    it('confirms the pixel rect on Enter', async () => {
      await shown()
      await userEvent.keyboard('{Enter}')
      expect(called('overlay_confirm')).toHaveLength(0)
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('{Enter}')
      expect(called('overlay_confirm')[0].args).toEqual({
        session: 9,
        rect: { x: 20, y: 20, width: 200, height: 100 },
      })
    })

    it('treats a selection under one pixel as none', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 10.2, y: 60 })
      expect(selection()).toBeNull()
      expect(screen.queryByTestId('size-badge')).toBeNull()
      expect(selected).toEqual([])
      await userEvent.keyboard('{Enter}')
      expect(called('overlay_confirm')).toHaveLength(0)
    })

    it('logs a confirm that fails and closes', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      confirmed = () => {
        throw new Error('empty')
      }
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('{Enter}')
      await waitFor(() => expect(called('close_overlay')).toHaveLength(1))
      // Only the session it confirmed, never a newer one.
      expect(called('close_overlay')[0].args).toEqual({ session: 9 })
      expect(error).toHaveBeenCalled()
    })

    it('sets an exact size from the badge', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.click(screen.getByRole('button', { name: /200 × 100/ }))
      const width = screen.getByLabelText('Width')
      await userEvent.clear(width)
      // Typing does not nudge or confirm the selection.
      await userEvent.type(width, '300{ArrowLeft}{Enter}')
      expect(rect()).toEqual([10, 10, 150, 50])
      expect(called('overlay_confirm')).toHaveLength(0)
      // Clicks on the badge do not start a new selection.
      fireEvent.pointerDown(screen.getByTestId('size-badge'), point({ x: 20, y: 5 }))
      expect(rect()).toEqual([10, 10, 150, 50])
    })

    it('Enter on the lock toggle stays in the badge', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.click(screen.getByRole('button', { name: /200 × 100/ }))
      screen.getByRole('button', { name: 'Lock ratio' }).focus()
      await userEvent.keyboard('{Enter}')
      expect(called('overlay_confirm')).toHaveLength(0)
    })

    it('shows the loupe while drawing only when enabled', async () => {
      context = fakeContext([255, 128, 0, 255])
      await shown({ magnifier: true })
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 20, y: 30 }))
      expect(screen.getByTestId('magnifier')).toHaveTextContent('40, 60')
      expect(screen.getByTestId('magnifier')).toHaveTextContent('#FF8000')
      // Hidden while a captured drag is past any edge of this display, back after.
      for (const off of [
        { x: -1, y: 30 },
        { x: 20, y: -1 },
        { x: 1000, y: 30 },
        { x: 20, y: 700 },
      ]) {
        fireEvent.pointerMove(overlay(), point(off))
        expect(screen.queryByTestId('magnifier')).toBeNull()
      }
      fireEvent.pointerMove(overlay(), point({ x: 999, y: 699 }))
      expect(screen.getByTestId('magnifier')).toHaveTextContent('1998, 1398')
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
