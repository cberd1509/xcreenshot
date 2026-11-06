import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { emit, listen } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { resetSettings, settings } from '$lib/settings.svelte'
import { box, fakeContext } from '../test/canvas'
import { stages } from '../test/konva'
import { OUTPUT_HEADER } from './annotate/output'
import type { Rect } from './geometry'

type Invoke = (
  cmd: string,
  body: unknown,
  options?: { headers?: Record<string, string> },
) => Promise<unknown>
import Overlay from './Overlay.svelte'

/** The platform the overlay sees (`isMac`). */
const platform = vi.hoisted(() => ({ mac: false }))
vi.mock('$lib/settings.svelte', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/settings.svelte')>()),
  get isMac() {
    return platform.mac
  },
}))

type Info = {
  session: number
  width: number
  height: number
  logicalWidth: number
  logicalHeight: number
  escapeShortcut: boolean
  region?: Rect | null
}
type At = { x: number; y: number }

let calls: { cmd: string; args: unknown }[]
let ready: Info | null
let frame: (args: { session: number }) => unknown
let finished: (cmd: string) => unknown
let context: ReturnType<typeof fakeContext>
type Selected = { label: string; active: boolean; session: number; stamp: number }
let selected: Selected[]

const rgba = (width: number, height: number) => new Uint8Array(width * height * 4).fill(7).buffer
const called = (cmd: string) => calls.filter((c) => c.cmd === cmd)
/** Copy and save requests: their header (parsed) and raw body. */
let sent: { cmd: string; header: unknown; body: unknown }[]
const output = (cmd: string, i: number) => sent.filter((c) => c.cmd === cmd)[i]?.header
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
/** Another display's selection event, newer than any so far unless `stamp` says otherwise. */
let later = 0
const other = (active: boolean, extra: { session?: number; stamp?: number } = {}) =>
  emit('overlay:selected', {
    label: 'overlay-2',
    active,
    session: 9,
    stamp: Date.now() + 1000 + ++later,
    ...extra,
  })
const esc = (token = 1) => emit('overlay:escape', { session: 9, token })
const answers = () => called('overlay_escaped').map((c) => c.args)
/** The selection `drag({ x: 10, y: 10 }, { x: 110, y: 60 })` makes, in pixels. */
const rect200 = { x: 20, y: 20, width: 200, height: 100 }
const ctrl = (key: string) => `{Control>}${key}{/Control}`

const overlay = () => screen.getByTestId('overlay')
const selection = () => screen.queryByTestId('selection')
const rect = () => box(selection()!)
const field = (name: 'Width' | 'Height') => screen.getByLabelText<HTMLInputElement>(name)
/** The size badge's fields, as shown. */
const size = () => [field('Width').value, field('Height').value].join(' × ')
const handle = (h: string) => selection()!.querySelector(`[data-handle="${h}"]`)!
const hint = () => [...screen.getByTestId('hint').children].map((c) => c.textContent)
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

const layer = () => screen.getByTestId('annotations')
/** The annotation shapes on the Konva stage. */
const shapes = () => stages[0].children[0].children.filter((n) => n.className !== 'Transformer')

/** Selects (10, 10) to (110, 60), then draws a rect in it with R. */
async function annotated() {
  await shown()
  drag({ x: 10, y: 10 }, { x: 110, y: 60 })
  await waitFor(() => expect(stages).toHaveLength(1))
  await userEvent.keyboard('r')
  fireEvent.pointerDown(layer(), point({ x: 20, y: 20 }))
  fireEvent.pointerMove(layer(), point({ x: 60, y: 50 }))
  fireEvent.pointerUp(layer(), point({ x: 60, y: 50 }))
  await waitFor(() => expect(shapes()).toHaveLength(1))
}

/** Opens the color popover, as when picking a color. */
async function colors() {
  await userEvent.click(screen.getByRole('button', { name: 'Color and size' }))
  await screen.findByLabelText('Thick')
}

describe('Overlay', () => {
  beforeEach(async () => {
    platform.mac = false
    calls = []
    ready = null
    selected = []
    frame = () => rgba(4, 2)
    finished = () => null
    mockWindows('overlay-1')
    mockIPC(
      (cmd, args) => {
        calls.push({ cmd, args })
        if (cmd === 'overlay_ready') return ready
        if (cmd === 'overlay_frame') return frame(args as { session: number })
        if (cmd === 'overlay_copy' || cmd === 'overlay_save') return finished(cmd)
        if (cmd === 'set_capture_option') {
          const { key, on } = args as { key: string; on: boolean }
          return { ...settings, [key]: on, version: calls.length }
        }
      },
      { shouldMockEvents: true },
    )
    // The mock drops request headers: record them on the way.
    sent = []
    const internals = Reflect.get(window, '__TAURI_INTERNALS__') as { invoke: Invoke }
    const inner = internals.invoke
    internals.invoke = (cmd, body, options) => {
      const header = options?.headers?.[OUTPUT_HEADER]
      if (header) sent.push({ cmd, header: JSON.parse(header), body })
      return inner(cmd, body, options)
    }
    context = fakeContext()
    await listen<Selected>('overlay:selected', (e) => selected.push(e.payload))
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

  it('does not paint a frame that arrives after the overlay closed', async () => {
    let respond!: (buffer: ArrayBuffer) => void
    frame = () => new Promise<ArrayBuffer>((r) => (respond = r))
    const { container } = await mount()
    await emit('overlay:show', small(1))
    await waitFor(() => expect(called('overlay_frame')).toHaveLength(1))
    await emit('overlay:closed')
    respond(rgba(4, 2))
    await tick()
    expect(called('overlay_painted')).toHaveLength(0)
    expect(container.querySelector('canvas')!.width).toBe(0)
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

  it('frees the frame when the overlay closes, not when it is only hidden', async () => {
    const { container } = await mount()
    const root = screen.getByTestId('overlay')
    // Nothing is rendered until there is a frame.
    expect(root).not.toBeVisible()
    await emit('overlay:show', small(1))
    await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
    expect(root).toBeVisible()
    const canvas = container.querySelector('canvas')!
    // Hidden for the Save dialog: the frame stays for when it shows again.
    vi.spyOn(document, 'hidden', 'get').mockReturnValue(true)
    document.dispatchEvent(new Event('visibilitychange'))
    expect(canvas.width).toBe(4)
    await emit('overlay:closed')
    await waitFor(() => expect([canvas.width, canvas.height]).toEqual([0, 0]))
    // Rendering nothing lets WebKit drop the full-screen layers too.
    expect(root).not.toBeVisible()
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
      expect(size()).toBe('200 × 100')
      expect(selected).toEqual([
        { label: 'overlay-1', active: true, session: 9, stamp: expect.any(Number) },
      ])
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
      fireEvent.pointerDown(handle('n'), point({ x: 60, y: 10 }))
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
      expect(selected.at(-1)).toMatchObject({ label: 'overlay-1', active: false })
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
      await userEvent.click(field('Width'))
      await userEvent.keyboard('5')
      await esc()
      await waitFor(() => expect(field('Width')).not.toHaveFocus())
      expect(size()).toBe('200 × 100')
      expect(selection()).not.toBeNull()
      expect(answers()).toEqual([{ session: 9, token: 1, handled: true }])
    })

    it('keeps one selection across displays', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      // Another display losing a selection changes nothing here.
      await other(false)
      await tick()
      expect(selection()).not.toBeNull()
      await other(true)
      await waitFor(() => expect(selection()).toBeNull())
      // Display 2 has the selection: this page has nothing to undo and says so.
      await esc()
      await waitFor(() => expect(answers()).toEqual([{ session: 9, token: 1, handled: false }]))
    })

    it('ignores a late event from another display, or one from another capture', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      const mine = selected.at(-1)!.stamp
      await other(true, { stamp: mine - 1 })
      await other(true, { session: 8 })
      await tick()
      expect(selection()).not.toBeNull()
      // Once a newer one came, an older one still changes nothing.
      await other(true, { stamp: mine + 5 })
      await waitFor(() => expect(selection()).toBeNull())
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await other(true, { stamp: mine + 2 })
      await tick()
      expect(selection()).not.toBeNull()
    })

    describe('remembered region', () => {
      const region = { x: 10, y: 10, width: 100, height: 50 }

      async function restored() {
        frame = () => rgba(info.width, info.height)
        await mount()
        await emit('overlay:show', { ...info, region })
        await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
      }

      it('opens with the region selected, as after a drag', async () => {
        await restored()
        expect(rect()).toEqual([10, 10, 100, 50])
        expect(size()).toBe('200 × 100')
        expect(selected).toEqual([
          { label: 'overlay-1', active: true, session: 9, stamp: expect.any(Number) },
        ])
      })

      it('copies it right away on Enter', async () => {
        await restored()
        await userEvent.keyboard('{Enter}')
        await waitFor(() =>
          expect(output('overlay_copy', 0)).toEqual({ session: 9, rect: rect200, blurs: [] }),
        )
      })

      it('is replaced by a new drag', async () => {
        await restored()
        drag({ x: 500, y: 500 }, { x: 520, y: 530 })
        expect(rect()).toEqual([500, 500, 20, 30])
      })

      it('leaves Esc to close the overlay until it is changed', async () => {
        await restored()
        await esc()
        await waitFor(() => expect(answers()).toEqual([{ session: 9, token: 1, handled: false }]))
        expect(selection()).not.toBeNull()
        await userEvent.keyboard('{ArrowRight}')
        await esc(2)
        await waitFor(() => expect(answers()).toHaveLength(2))
        expect(answers()[1]).toEqual({ session: 9, token: 2, handled: true })
        expect(selection()).toBeNull()
      })

      it('starts empty without one', async () => {
        frame = () => rgba(info.width, info.height)
        await mount()
        await emit('overlay:show', { ...info, region: null })
        await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
        expect(selection()).toBeNull()
      })
    })

    it('copies the pixel rect on Enter and Ctrl+C', async () => {
      await shown()
      await userEvent.keyboard('{Enter}')
      await userEvent.keyboard(ctrl('c'))
      expect(called('overlay_copy')).toHaveLength(0)
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('{Enter}')
      await waitFor(() =>
        expect(output('overlay_copy', 0)).toEqual({ session: 9, rect: rect200, blurs: [] }),
      )
      await waitFor(async () => {
        await userEvent.keyboard(ctrl('c'))
        expect(called('overlay_copy')).toHaveLength(2)
      })
      await waitFor(() =>
        expect(output('overlay_copy', 1)).toEqual({ session: 9, rect: rect200, blurs: [] }),
      )
    })

    it('saves the pixel rect on Ctrl+S', async () => {
      await shown()
      await userEvent.keyboard(ctrl('s'))
      expect(called('overlay_save')).toHaveLength(0)
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard(ctrl('s'))
      await waitFor(() =>
        expect(output('overlay_save', 0)).toEqual({ session: 9, rect: rect200, blurs: [] }),
      )
      expect(called('overlay_copy')).toHaveLength(0)
    })

    it('uses Cmd instead of Ctrl on macOS', async () => {
      platform.mac = true
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard(ctrl('c'))
      await userEvent.keyboard(ctrl('s'))
      expect(calls.filter((c) => c.cmd === 'overlay_copy' || c.cmd === 'overlay_save')).toEqual([])
      await userEvent.keyboard('{Meta>}s{/Meta}')
      await waitFor(() => expect(called('overlay_save')).toHaveLength(1))
      await userEvent.keyboard('{Meta>}c{/Meta}')
      await waitFor(() => expect(called('overlay_copy')).toHaveLength(1))
    })

    it('follows the physical key on a non-Latin layout', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      // Russian layout: the C key types "с", the S key "ы".
      fireEvent.keyDown(window, { key: 'ы', code: 'KeyS', ctrlKey: true })
      await waitFor(() => expect(called('overlay_save')).toHaveLength(1))
      fireEvent.keyDown(window, { key: 'с', code: 'KeyD', ctrlKey: true })
      expect(called('overlay_copy')).toHaveLength(0)
    })

    it('ignores other modifiers and plain letters', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('cs')
      await userEvent.keyboard('{Control>}{Shift>}s{/Shift}{/Control}')
      await userEvent.keyboard('{Control>}{Alt>}c{/Alt}{/Control}')
      expect(calls.filter((c) => c.cmd === 'overlay_copy' || c.cmd === 'overlay_save')).toEqual([])
    })

    it('runs one copy or save at a time', async () => {
      let done!: () => void
      finished = () => new Promise<void>((r) => (done = r))
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard(ctrl('s'))
      await userEvent.keyboard('{Enter}')
      expect(called('overlay_save')).toHaveLength(1)
      expect(called('overlay_copy')).toHaveLength(0)
      done()
      await waitFor(async () => {
        await userEvent.keyboard('{Enter}')
        expect(called('overlay_copy')).toHaveLength(1)
      })
    })

    it('treats a selection under one pixel as none', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 10.2, y: 60 })
      expect(selection()).toBeNull()
      expect(screen.queryByTestId('size-badge')).toBeNull()
      expect(selected).toEqual([])
      await userEvent.keyboard('{Enter}')
      expect(called('overlay_copy')).toHaveLength(0)
    })

    it('says so when copy or save fails, and stays open', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      finished = (cmd) => {
        throw new Error(`${cmd} broke`)
      }
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.keyboard('{Enter}')
      expect(await screen.findByRole('alert')).toHaveTextContent("Couldn't copy")
      await userEvent.keyboard(ctrl('s'))
      await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent("Couldn't save"))
      expect(called('close_overlay')).toHaveLength(0)
      expect(selection()).not.toBeNull()
      expect(error).toHaveBeenCalled()
      // Gone when the overlay closes.
      await emit('overlay:closed')
      await waitFor(() => expect(screen.queryByRole('alert')).toBeNull())
    })

    it('hides the message after a few seconds', async () => {
      vi.spyOn(console, 'error').mockImplementation(() => {})
      finished = () => {
        throw new Error('broke')
      }
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
      try {
        fireEvent.keyDown(window, { key: 'Enter' })
        await vi.waitFor(() => expect(screen.getByRole('alert')).toBeInTheDocument())
        vi.advanceTimersByTime(3000)
        await vi.waitFor(() => expect(screen.queryByRole('alert')).toBeNull())
      } finally {
        vi.useRealTimers()
      }
    })

    it('sets an exact size from the badge', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.clear(field('Width'))
      // Typing does not nudge, copy or save the selection.
      await userEvent.type(field('Width'), '300{ArrowLeft}{Enter}')
      expect(rect()).toEqual([10, 10, 150, 50])
      expect(field('Width')).not.toHaveFocus()
      expect(size()).toBe('300 × 100')
      await userEvent.click(field('Height'))
      await userEvent.keyboard('{Control>}c{/Control}{Control>}s{/Control}{Escape}')
      expect(calls.filter((c) => c.cmd === 'overlay_copy' || c.cmd === 'overlay_save')).toEqual([])
      // Clicks on the badge do not start a new selection.
      fireEvent.pointerDown(screen.getByTestId('size-badge'), point({ x: 20, y: 5 }))
      expect(rect()).toEqual([10, 10, 150, 50])
    })

    it('Enter on the lock toggle stays in the badge', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      screen.getByRole('button', { name: 'Lock aspect ratio' }).focus()
      await userEvent.keyboard('{Enter}')
      expect(called('overlay_copy')).toHaveLength(0)
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

    it('shows the loupe and guides at the pointer before a selection', async () => {
      context = fakeContext([255, 128, 0, 255])
      await shown({ magnifier: true })
      // Not until the pointer is known.
      expect(screen.queryByTestId('magnifier')).toBeNull()
      expect(screen.queryAllByTestId('guide')).toHaveLength(0)
      fireEvent.pointerMove(overlay(), point({ x: 20, y: 30 }))
      expect(screen.getByTestId('magnifier')).toHaveTextContent('40, 60 · #FF8000')
      const [down, across] = screen.getAllByTestId('guide')
      expect(down.style.transform).toBe('translateX(20px)')
      expect(across.style.transform).toBe('translateY(30px)')
      // Neither once there is a selection.
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      expect(screen.queryByTestId('magnifier')).toBeNull()
      expect(screen.queryAllByTestId('guide')).toHaveLength(0)
    })

    it('shows the keys for each state at the bottom', async () => {
      await shown()
      expect(hint()).toEqual(['Drag select area', 'Esc cancel'])
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      expect(hint()).toEqual(['Enter copy', 'Ctrl+S save', 'Space move', 'Esc clear'])
      // Typing a size: Enter applies it and Esc cancels the edit.
      await userEvent.click(field('Width'))
      expect(hint()).toEqual(['Enter apply', 'Ctrl+S save', 'Space move', 'Esc cancel'])
    })

    it('shows that Esc closes a remembered region', async () => {
      frame = () => rgba(info.width, info.height)
      await mount()
      await emit('overlay:show', { ...info, region: { x: 10, y: 10, width: 100, height: 50 } })
      await waitFor(() => expect(hint()).toContain('Esc cancel'))
    })

    it('shows ⌘ in the keys on macOS', async () => {
      platform.mac = true
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      expect(hint()).toContain('⌘S save')
    })

    it('moves a new selection while Space is held', async () => {
      await shown()
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 60, y: 40 }))
      expect(rect()).toEqual([10, 10, 50, 30])
      await fireEvent.keyDown(window, { key: ' ' })
      fireEvent.pointerMove(overlay(), point({ x: 80, y: 50 }))
      expect(rect()).toEqual([30, 20, 50, 30])
      await fireEvent.keyUp(window, { key: ' ' })
      // Let go, the drag sizes the moved selection again.
      fireEvent.pointerMove(overlay(), point({ x: 90, y: 70 }))
      expect(rect()).toEqual([30, 20, 60, 50])
      fireEvent.pointerUp(overlay(), point({ x: 90, y: 70 }))
    })

    it('moves the press point when Space comes before the first move', async () => {
      await shown()
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      await fireEvent.keyDown(window, { key: ' ' })
      fireEvent.pointerMove(overlay(), point({ x: 50, y: 50 }))
      expect(selection()).toBeNull()
      await fireEvent.keyUp(window, { key: ' ' })
      fireEvent.pointerMove(overlay(), point({ x: 80, y: 70 }))
      expect(rect()).toEqual([50, 50, 30, 20])
      fireEvent.pointerUp(overlay(), point({ x: 80, y: 70 }))
    })

    it('moves a selection being resized while Space is held, until the drag ends', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 60, y: 40 })
      fireEvent.pointerDown(handle('se'), point({ x: 60, y: 40 }))
      fireEvent.pointerMove(overlay(), point({ x: 70, y: 50 }))
      expect(rect()).toEqual([10, 10, 60, 40])
      await fireEvent.keyDown(window, { key: ' ' })
      fireEvent.pointerMove(overlay(), point({ x: 80, y: 55 }))
      expect(rect()).toEqual([20, 15, 60, 40])
      fireEvent.pointerUp(overlay(), point({ x: 80, y: 55 }))
      // Moved into an edge, it stops there whole.
      fireEvent.pointerDown(handle('se'), point({ x: 80, y: 55 }))
      await fireEvent.keyDown(window, { key: ' ' })
      fireEvent.pointerMove(overlay(), point({ x: -500, y: 55 }))
      expect(rect()).toEqual([0, 15, 60, 40])
      await fireEvent.keyUp(window, { key: ' ' })
      fireEvent.pointerMove(overlay(), point({ x: -490, y: 65 }))
      expect(rect()).toEqual([0, 15, 70, 50])
      fireEvent.pointerUp(overlay(), point({ x: -490, y: 65 }))
      // Space without a drag does nothing; a new drag draws as usual.
      await fireEvent.keyDown(window, { key: ' ' })
      drag({ x: 200, y: 200 }, { x: 250, y: 230 })
      expect(rect()).toEqual([200, 200, 50, 30])
    })

    it('keeps the aspect ratio with the lock, for handles and typed sizes', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      const lock = screen.getByRole('button', { name: 'Lock aspect ratio' })
      await userEvent.click(lock)
      expect(lock).toHaveAttribute('aria-pressed', 'true')
      // The lock takes no focus, so keys still reach the overlay.
      expect(lock).not.toHaveFocus()
      drag({ x: 110, y: 60 }, { x: 160, y: 60 }, handle('se'))
      expect(rect()).toEqual([10, 10, 150, 75])
      await userEvent.clear(field('Height'))
      await userEvent.type(field('Height'), '50')
      expect(size()).toBe('100 × 50')
      await userEvent.keyboard('{Enter}')
      expect(rect()).toEqual([10, 10, 50, 25])
      await userEvent.click(lock)
      drag({ x: 60, y: 35 }, { x: 80, y: 35 }, handle('se'))
      expect(rect()).toEqual([10, 10, 70, 25])
    })

    it('follows the magnifier setting by default (off)', async () => {
      await shown()
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 20, y: 30 }))
      expect(screen.queryByTestId('magnifier')).toBeNull()
    })

    it('drops the selection when the overlay closes and on the next capture', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await emit('overlay:closed')
      await waitFor(() => expect(selection()).toBeNull())
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await emit('overlay:show', { ...info, session: 10 })
      await waitFor(() => expect(selection()).toBeNull())
    })
  })

  describe('annotations', () => {
    beforeEach(() => {
      stages.length = 0
    })

    it('draws over the frame, under the selection outline', async () => {
      await annotated()
      const root = overlay()
      const at = (e: Element | null) => [...root.children].indexOf(e!)
      expect(at(root.querySelector('canvas'))).toBeLessThan(at(layer()))
      expect(at(layer())).toBeLessThan(at(selection()))
    })

    it('ignores a press outside the selection while there are annotations', async () => {
      await annotated()
      drag({ x: 500, y: 500 }, { x: 520, y: 530 })
      expect(rect()).toEqual([10, 10, 100, 50])
      expect(shapes()).toHaveLength(1)
    })

    it('starts a new selection from outside once the annotations are out of it', async () => {
      await annotated()
      // Put the tool away, then move the selection off the rect.
      await esc(1)
      await waitFor(() =>
        expect(screen.getByLabelText('Rectangle')).toHaveAttribute('data-state', 'off'),
      )
      drag({ x: 100, y: 55 }, { x: 400, y: 355 }, selection()!)
      expect(rect()).toEqual([310, 310, 100, 50])
      drag({ x: 600, y: 600 }, { x: 620, y: 630 })
      expect(rect()).toEqual([600, 600, 20, 30])
    })

    it('keeps a resize from cutting through the annotations', async () => {
      await annotated()
      // The rect paints from (16, 16) to (64, 54): the right edge stops there.
      drag({ x: 110, y: 30 }, { x: 10, y: 30 }, selection()!.querySelector('[data-handle="e"]')!)
      expect(rect()).toEqual([10, 10, 54, 50])
      // Growing is free.
      drag({ x: 64, y: 60 }, { x: 200, y: 200 }, selection()!.querySelector('[data-handle="se"]')!)
      expect(rect()).toEqual([10, 10, 190, 190])
      expect(shapes()).toHaveLength(1)
    })

    it('focuses the window on a press the annotations take', async () => {
      await annotated()
      const before = called('plugin:window|set_focus').length
      fireEvent.pointerDown(layer(), point({ x: 30, y: 30 }))
      fireEvent.pointerUp(layer(), point({ x: 30, y: 30 }))
      expect(called('plugin:window|set_focus')).toHaveLength(before + 1)
    })

    it('undoes one thing per Esc: size edit, then annotations, then the selection', async () => {
      await annotated()
      await userEvent.click(field('Width'))
      // The size edit goes first, then the tool, then the selection with its annotations.
      await esc(1)
      await waitFor(() => expect(field('Width')).not.toHaveFocus())
      expect(screen.getByLabelText('Rectangle')).toHaveAttribute('data-state', 'on')
      await esc(2)
      await esc(3)
      await waitFor(() =>
        expect(answers()).toEqual([
          { session: 9, token: 1, handled: true },
          { session: 9, token: 2, handled: true },
          { session: 9, token: 3, handled: true },
        ]),
      )
      expect(selection()).toBeNull()
      expect(shapes()).toHaveLength(0)
    })

    it('gives keys to the annotations first', async () => {
      await annotated()
      shapes()[0].fire('pointerdown')
      // With a shape selected, arrows move the shape, not the selection.
      await userEvent.keyboard('{ArrowRight}')
      expect(rect()).toEqual([10, 10, 100, 50])
      await waitFor(() => expect(shapes()[0].attrs.x).toBe(20.5))
      await userEvent.keyboard('{Backspace}')
      await waitFor(() => expect(shapes()).toHaveLength(0))
      await userEvent.keyboard('{Control>}z{/Control}')
      await waitFor(() => expect(shapes()).toHaveLength(1))
    })

    it('leaves every key to the text being typed, and Esc commits it', async () => {
      await annotated()
      await userEvent.keyboard('t')
      fireEvent.pointerDown(layer(), point({ x: 70, y: 30 }))
      fireEvent.pointerUp(layer(), point({ x: 70, y: 30 }))
      const text = await screen.findByRole('textbox', { name: 'Text' })
      await waitFor(() => expect(text).toHaveFocus())
      // Letters, arrows, Enter and Ctrl+C or Ctrl+S type or move the caret: no tool, nudge, copy
      // or save.
      await userEvent.keyboard('ra{ArrowLeft}{Enter}{Control>}cs{/Control}')
      expect(text).toHaveValue('r\na')
      expect(rect()).toEqual([10, 10, 100, 50])
      expect(calls.filter((c) => c.cmd === 'overlay_copy' || c.cmd === 'overlay_save')).toEqual([])
      expect(screen.getByLabelText('Text', { selector: 'button' })).toHaveAttribute(
        'data-state',
        'on',
      )
      await esc(1)
      await waitFor(() => expect(answers()).toEqual([{ session: 9, token: 1, handled: true }]))
      expect(shapes()).toHaveLength(2)
      expect(screen.queryByRole('textbox', { name: 'Text' })).toBeNull()
    })

    it('leaves keys to an open toolbar menu', async () => {
      await annotated()
      await userEvent.click(screen.getByRole('button', { name: 'More' }))
      await screen.findByText('Magnifier')
      fireEvent.keyDown(window, { key: 'Enter' })
      await tick()
      expect(called('overlay_copy')).toHaveLength(0)
    })

    it('closes an open toolbar menu when drawing starts', async () => {
      await annotated()
      await colors()
      fireEvent.pointerDown(layer(), point({ x: 70, y: 20 }))
      fireEvent.pointerMove(layer(), point({ x: 90, y: 40 }))
      fireEvent.pointerUp(layer(), point({ x: 90, y: 40 }))
      await waitFor(() => expect(screen.queryByLabelText('Thick')).toBeNull())
      expect(shapes()).toHaveLength(2)
    })

    it('copies and saves with an open toolbar menu, closing it first', async () => {
      await annotated()
      await colors()
      await userEvent.keyboard(ctrl('c'))
      await waitFor(() => expect(sent).toHaveLength(1))
      expect(sent[0]).toMatchObject({
        cmd: 'overlay_copy',
        header: { session: 9, rect: rect200, layer: { x: 12, y: 12, width: 96, height: 76 } },
      })
      await waitFor(() => expect(screen.queryByLabelText('Thick')).toBeNull())
      await colors()
      await userEvent.keyboard(ctrl('s'))
      await waitFor(() => expect(sent).toHaveLength(2))
      expect(sent[1]).toMatchObject({ cmd: 'overlay_save', header: { rect: rect200 } })
      await waitFor(() => expect(screen.queryByLabelText('Thick')).toBeNull())
    })

    it('drops them for a new selection on another display', async () => {
      await annotated()
      await other(false)
      await tick()
      expect(shapes()).toHaveLength(1)
      await other(true)
      await waitFor(() => expect(shapes()).toHaveLength(0))
    })

    it('drops them on the next capture', async () => {
      await annotated()
      await emit('overlay:show', { ...info, session: 10 })
      await waitFor(() => expect(shapes()).toHaveLength(0))
    })

    it('sends the annotations with a copy', async () => {
      await annotated()
      await userEvent.keyboard('{Enter}')
      await waitFor(() => expect(sent).toHaveLength(1))
      const [{ cmd, header, body }] = sent
      expect(cmd).toBe('overlay_copy')
      // The rect from (20, 20) to (60, 50), its stroke included, at 2x, inside the crop.
      expect(header).toEqual({
        session: 9,
        rect: rect200,
        layer: { x: 12, y: 12, width: 96, height: 76 },
        blurs: [],
      })
      expect(body).toBeInstanceOf(Uint8Array)
      expect((body as Uint8Array).length).toBe(96 * 76 * 4)
    })

    it('shows what Enter and Esc do with annotations', async () => {
      await annotated()
      expect(hint()).toEqual(['Enter copy', 'Ctrl+S save', 'Space move', 'Esc drop tool'])
      await colors()
      expect(hint()).toEqual(['Ctrl+S save', 'Space move', 'Esc close menu'])
    })

    it('presses a focused button with Enter, not copy', async () => {
      await annotated()
      const save = await screen.findByRole('button', { name: 'Save' })
      save.focus()
      await userEvent.keyboard('{Enter}')
      await waitFor(() => expect(called('overlay_save')).toHaveLength(1))
      expect(called('overlay_copy')).toHaveLength(0)
    })

    it('moves the selection with Select, a drag off the shapes going to the overlay', async () => {
      await annotated()
      await userEvent.keyboard('v')
      await waitFor(() =>
        expect(screen.getByLabelText('Select')).toHaveAttribute('data-state', 'on'),
      )
      drag({ x: 80, y: 40 }, { x: 90, y: 45 }, layer())
      expect(rect()).toEqual([20, 15, 100, 50])
      expect(shapes()).toHaveLength(1)
    })

    it('lets a resize moved off the annotations with Space shrink freely', async () => {
      await annotated()
      fireEvent.pointerDown(handle('se'), point({ x: 110, y: 60 }))
      await fireEvent.keyDown(window, { key: ' ' })
      fireEvent.pointerMove(overlay(), point({ x: 310, y: 60 }))
      expect(rect()).toEqual([210, 10, 100, 50])
      await fireEvent.keyUp(window, { key: ' ' })
      fireEvent.pointerMove(overlay(), point({ x: 220, y: 60 }))
      expect(rect()).toEqual([210, 10, 10, 50])
      fireEvent.pointerUp(overlay(), point({ x: 220, y: 60 }))
    })

    it('keeps the locked ratio when the annotations hold a resize back', async () => {
      await annotated()
      await userEvent.click(screen.getByRole('button', { name: 'Lock aspect ratio' }))
      drag({ x: 110, y: 30 }, { x: 10, y: 30 }, handle('e'))
      const [x, y, width, height] = rect()
      expect(width / height).toBeCloseTo(2)
      // It still holds the rect, painted from (16, 16) to (64, 54).
      expect(x).toBeLessThanOrEqual(16)
      expect(y).toBeLessThanOrEqual(16)
      expect(x + width).toBeGreaterThanOrEqual(64)
      expect(y + height).toBeGreaterThanOrEqual(54)
    })

    it('closes the overlay with Cancel', async () => {
      await annotated()
      await userEvent.click(await screen.findByRole('button', { name: 'Cancel' }))
      expect(called('close_overlay')).toEqual([{ cmd: 'close_overlay', args: { session: 9 } }])
    })

    it('copies and saves from the toolbar', async () => {
      await annotated()
      await userEvent.click(screen.getByRole('button', { name: 'Save' }))
      await waitFor(() => expect(called('overlay_save')).toHaveLength(1))
      await userEvent.click(screen.getByRole('button', { name: 'Copy' }))
      await waitFor(() => expect(called('overlay_copy')).toHaveLength(1))
      expect(sent).toHaveLength(2)
    })

    it('turns options on and off in the settings', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await userEvent.click(await screen.findByRole('button', { name: 'More' }, { timeout: 5000 }))
      await userEvent.click(
        (await screen.findByText('Magnifier')).closest('[role=menuitemcheckbox]')!,
      )
      await waitFor(() => expect(settings.magnifier).toBe(true))
      expect(called('set_capture_option')[0].args).toEqual({ key: 'magnifier', on: true })
      resetSettings()
    })
  })
})
