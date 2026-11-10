import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { emit, listen } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { resetSettings, settings } from '$lib/settings.svelte'
import { box, fakeContext } from '../test/canvas'
import { fakeFrames } from '../test/frames'
import { stages } from '../test/konva'
import { OUTPUT_HEADER } from './annotate/output'
import type { Rect } from './geometry'

type Invoke = (
  cmd: string,
  body: unknown,
  options?: { headers?: Record<string, string> },
) => Promise<unknown>
import Overlay from './Overlay.svelte'

/** The platform the overlay sees (`isMac`, `isWindows`). */
const platform = vi.hoisted(() => ({ mac: false, windows: false }))
vi.mock('$lib/settings.svelte', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/settings.svelte')>()),
  get isMac() {
    return platform.mac
  },
  get isWindows() {
    return platform.windows
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
/** Rust's answer to a double-click (`overlay_window_at`). */
let windowAt: (args: { session: number; x: number; y: number }) => unknown
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

/** Double-clicks at `at`, as the browser does: two clicks, then `dblclick`. */
function dblclick(at: At, keys: { metaKey?: boolean; ctrlKey?: boolean } = {}, target?: Element) {
  const on = target ?? overlay()
  for (let i = 0; i < 2; i++) {
    fireEvent.pointerDown(on, point(at))
    fireEvent.pointerUp(on, point(at))
  }
  fireEvent.dblClick(on, { clientX: at.x, clientY: at.y, ...keys })
}

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

/** Mounts and draws the 2x frame with quick copy on. */
async function quick(props: { magnifier?: boolean } = {}) {
  frame = () => rgba(info.width, info.height)
  const view = await mount(props)
  await emit('overlay:show', { ...info, quickCopy: true })
  await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))
  return view
}
const bar = () => screen.queryByRole('toolbar', { name: 'Quick copy' })
const bars = () => screen.queryAllByRole('toolbar', { name: 'Quick copy' })

/** Two displays' pages, both drawn with quick copy on. */
async function twoDisplays() {
  frame = () => rgba(info.width, info.height)
  render(Overlay)
  render(Overlay)
  await waitFor(() => expect(called('overlay_ready')).toHaveLength(2))
  await emit('overlay:show', { ...info, quickCopy: true })
  await waitFor(() => expect(called('overlay_painted')).toHaveLength(2))
  expect(bars()).toHaveLength(2)
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

/** The toolbar's ⋯ button. */
const more = () => screen.getByRole('button', { name: 'More' })

/** Opens the color popover, as when picking a color. */
async function colors() {
  await userEvent.click(screen.getByRole('button', { name: 'Color and size' }))
  await screen.findByLabelText('Thick')
}

describe('Overlay', () => {
  beforeEach(async () => {
    platform.mac = false
    platform.windows = false
    calls = []
    ready = null
    selected = []
    frame = () => rgba(4, 2)
    finished = () => null
    windowAt = () => null
    mockWindows('overlay-1')
    mockIPC(
      (cmd, args) => {
        calls.push({ cmd, args })
        if (cmd === 'overlay_ready') return ready
        if (cmd === 'overlay_frame') return frame(args as { session: number })
        if (cmd === 'overlay_window_at') return windowAt(args as { session: number } & At)
        if (['overlay_copy', 'overlay_save', 'overlay_share'].includes(cmd)) return finished(cmd)
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
    expect(called('overlay_painted')[0].args).toEqual({ session: 3, rendered: true })
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
    expect(called('overlay_painted').map((c) => c.args)).toEqual([{ session: 2, rendered: true }])
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

  it('reports the paint only once the frame is rendered, and not after a close', async () => {
    const frames = fakeFrames()
    try {
      await mount()
      await emit('overlay:show', small(1))
      await waitFor(() => expect(frames.pending()).toBe(1))
      frames.next()
      await tick()
      expect(called('overlay_painted'), 'drawn, not yet rendered').toHaveLength(0)
      frames.next()
      await waitFor(() => expect(called('overlay_painted')).toHaveLength(1))

      // Closed while waiting for the next session's frame to render: Rust is not told.
      await emit('overlay:closed')
      await emit('overlay:show', small(2))
      await waitFor(() => expect(frames.pending()).toBe(1))
      await emit('overlay:closed')
      frames.next()
      frames.next()
      await tick()
      expect(called('overlay_painted')).toHaveLength(1)
    } finally {
      frames.restore()
    }
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
      expect(hint()).toEqual([
        'Drag select area',
        'Double-click window',
        'Ctrl double-click full screen',
        'Esc cancel',
      ])
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
      expect(hint()).toContain('⌘ double-click full screen')
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
      await userEvent.click(more())
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

    it('copies and saves with the ⋯ menu open, closing it first', async () => {
      await annotated()
      await userEvent.click(more())
      await screen.findByText('Magnifier')
      await userEvent.keyboard(ctrl('c'))
      await waitFor(() => expect(sent).toHaveLength(1))
      expect(sent[0].cmd).toBe('overlay_copy')
      await waitFor(() => expect(screen.queryByText('Magnifier')).toBeNull())
      await userEvent.click(more())
      await screen.findByText('Magnifier')
      await userEvent.keyboard(ctrl('s'))
      await waitFor(() => expect(sent).toHaveLength(2))
      expect(sent[1].cmd).toBe('overlay_save')
      await waitFor(() => expect(screen.queryByText('Magnifier')).toBeNull())
    })

    it('copies with ⌘C on macOS with the color menu open', async () => {
      platform.mac = true
      await annotated()
      await colors()
      await userEvent.keyboard('{Meta>}c{/Meta}')
      await waitFor(() => expect(sent).toHaveLength(1))
      expect(sent[0].cmd).toBe('overlay_copy')
      await waitFor(() => expect(screen.queryByLabelText('Thick')).toBeNull())
    })

    it('saves from the hex field, with the color typed there', async () => {
      await annotated()
      shapes()[0].fire('pointerdown')
      await colors()
      const hex = screen.getByLabelText<HTMLInputElement>('Hex')
      await userEvent.clear(hex)
      await userEvent.type(hex, '#22c55e')
      await userEvent.keyboard(ctrl('s'))
      await waitFor(() => expect(sent).toHaveLength(1))
      expect(sent[0].cmd).toBe('overlay_save')
      // The export stage, built for the save, drew the shape in the typed color.
      expect(stages).toHaveLength(2)
      expect(stages[1].children[0].children[0].attrs.stroke).toBe('#22C55E')
      await waitFor(() => expect(screen.queryByLabelText('Thick')).toBeNull())
    })

    it('copies from the width field', async () => {
      await annotated()
      field('Width').focus()
      expect(document.activeElement).toBe(field('Width'))
      await userEvent.keyboard(ctrl('c'))
      await waitFor(() => expect(sent).toHaveLength(1))
      expect(output('overlay_copy', 0)).toMatchObject({ rect: rect200 })
    })

    it('saves from the height field, with the size typed there', async () => {
      await annotated()
      await userEvent.clear(field('Height'))
      await userEvent.type(field('Height'), '50')
      await userEvent.keyboard(ctrl('s'))
      await waitFor(() => expect(sent).toHaveLength(1))
      expect(output('overlay_save', 0)).toMatchObject({ rect: { ...rect200, height: 50 } })
      expect(size()).toBe('200 × 50')
      expect(field('Height')).not.toHaveFocus()
    })

    it('cancels a size edit with Esc, so a later Copy takes the old size', async () => {
      await annotated()
      await userEvent.clear(field('Width'))
      await userEvent.type(field('Width'), '50')
      await esc()
      await waitFor(() => expect(field('Width')).not.toHaveFocus())
      expect(size()).toBe('200 × 100')
      expect(answers()).toEqual([{ session: 9, token: 1, handled: true }])
      await userEvent.keyboard(ctrl('c'))
      await waitFor(() => expect(sent).toHaveLength(1))
      expect(output('overlay_copy', 0)).toMatchObject({ rect: rect200 })
    })

    it('copies from the hex field, unless its text is selected', async () => {
      await annotated()
      await colors()
      const hex = screen.getByLabelText<HTMLInputElement>('Hex')
      await userEvent.clear(hex)
      await userEvent.type(hex, '#22c5')
      hex.select()
      await userEvent.keyboard(ctrl('c'))
      await tick()
      // The text selected is the field's to copy: what was typed stays, selected.
      expect(sent).toHaveLength(0)
      expect(hex).toHaveValue('#22c5')
      expect([hex.selectionStart, hex.selectionEnd]).toEqual([0, 5])
      expect(screen.getByLabelText('Thick')).toBeInTheDocument()
      hex.setSelectionRange(5, 5)
      await userEvent.keyboard(ctrl('c'))
      await waitFor(() => expect(sent).toHaveLength(1))
      expect(sent[0].cmd).toBe('overlay_copy')
    })

    it('closes an open toolbar menu with Esc first, then goes on as without it', async () => {
      await shown()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await screen.findByRole('button', { name: 'Color and size' }, { timeout: 5000 })
      await colors()
      await esc(1)
      await waitFor(() => expect(screen.queryByLabelText('Thick')).toBeNull())
      expect(selection()).not.toBeNull()
      await userEvent.click(more())
      await screen.findByText('Magnifier')
      await esc(2)
      await waitFor(() => expect(screen.queryByText('Magnifier')).toBeNull())
      expect(selection()).not.toBeNull()
      // Then the selection, then the overlay.
      await esc(3)
      await waitFor(() => expect(selection()).toBeNull())
      await esc(4)
      await waitFor(() =>
        expect(answers().map((a) => (a as { handled: boolean }).handled)).toEqual([
          true,
          true,
          true,
          false,
        ]),
      )
    })

    it('leaves Copy to the text being typed with the color menu open', async () => {
      await annotated()
      await userEvent.keyboard('t')
      fireEvent.pointerDown(layer(), point({ x: 70, y: 30 }))
      fireEvent.pointerUp(layer(), point({ x: 70, y: 30 }))
      const text = await screen.findByRole('textbox', { name: 'Text' })
      await waitFor(() => expect(text).toHaveFocus())
      await userEvent.click(screen.getByRole('button', { name: 'Color and size' }))
      await screen.findByLabelText('Thick')
      await userEvent.keyboard(`a${ctrl('c')}${ctrl('s')}`)
      await tick()
      expect(text).toHaveValue('a')
      expect(sent).toHaveLength(0)
      // In the hex field meanwhile, they neither take the color typed there nor go on.
      const hex = screen.getByLabelText<HTMLInputElement>('Hex')
      await userEvent.clear(hex)
      await userEvent.type(hex, '#22c55e')
      await userEvent.keyboard(ctrl('s'))
      await tick()
      expect(hex).toHaveValue('#22c55e')
      expect(screen.getByTestId('current-color')).not.toHaveStyle({ background: '#22C55E' })
      expect(sent).toHaveLength(0)
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

    it('copies and saves at a larger size from the carets, the annotations rendered at it', async () => {
      await annotated()
      await userEvent.click(await screen.findByRole('button', { name: 'Copy at a larger size' }))
      // The selection is 200 × 100 physical px.
      expect(await screen.findByText('800 × 400')).toBeInTheDocument()
      await userEvent.click(screen.getByText('Copy at 4×'))
      await waitFor(() => expect(called('overlay_copy')).toHaveLength(1))
      // The layer at 4x: (12, 12, 96, 76) times four, drawn at that size.
      expect(output('overlay_copy', 0)).toEqual({
        session: 9,
        rect: rect200,
        layer: { x: 48, y: 48, width: 384, height: 304 },
        blurs: [],
        factor: 4,
      })
      expect(sent[0].body as Uint8Array).toHaveLength(384 * 304 * 4)
      await userEvent.click(screen.getByRole('button', { name: 'Save at a larger size' }))
      await userEvent.click(await screen.findByText('Save at 2×'))
      await waitFor(() => expect(called('overlay_save')).toHaveLength(1))
      expect(output('overlay_save', 0)).toMatchObject({ rect: rect200, factor: 2 })
      // The main buttons stay at 1×, once the menu is gone.
      await waitFor(() => expect(document.body.style.pointerEvents).not.toBe('none'))
      await userEvent.click(screen.getByRole('button', { name: 'Save' }))
      await waitFor(() => expect(called('overlay_save')).toHaveLength(2))
      expect(output('overlay_save', 1)).not.toHaveProperty('factor')
    })

    it('shares from the toolbar on macOS, under the Share button', async () => {
      platform.mac = true
      await annotated()
      const share = screen.getByRole('button', { name: 'Share' })
      // Made up at run time, so only the button's real rect can match it.
      const anchor = { x: Math.random() * 500, y: Math.random() * 500, width: 80, height: 36 }
      vi.spyOn(share, 'getBoundingClientRect').mockReturnValue(DOMRect.fromRect(anchor) as DOMRect)
      await userEvent.click(share)
      await waitFor(() => expect(called('overlay_share')).toHaveLength(1))
      const head = output('overlay_share', 0) as { rect: Rect; anchor: Rect; layer?: Rect }
      expect(head.anchor).toEqual(anchor)
      // The same request as a save, annotations included.
      expect(head.rect).toEqual(rect200)
      // The annotation's pixels go as the raw body, drawn (not blank) and sized to the layer.
      const drawn = head.layer!
      const body = sent.find((c) => c.cmd === 'overlay_share')!.body as Uint8Array
      expect(body).toHaveLength(drawn.width * drawn.height * 4)
      expect(body.some((b) => b !== 0)).toBe(true)
    })

    it('says so when sharing fails, and stays open', async () => {
      vi.spyOn(console, 'error').mockImplementation(() => {})
      platform.mac = true
      finished = () => {
        throw new Error('no sheet')
      }
      await annotated()
      await userEvent.click(screen.getByRole('button', { name: 'Share' }))
      expect(await screen.findByRole('alert')).toHaveTextContent("Couldn't share")
      expect(selection()).not.toBeNull()
    })

    it('shares on Windows too', async () => {
      platform.windows = true
      await annotated()
      await userEvent.click(screen.getByRole('button', { name: 'Share' }))
      await waitFor(() => expect(called('overlay_share')).toHaveLength(1))
    })

    it('has no Share button without a share sheet', async () => {
      await annotated()
      expect(screen.getByRole('button', { name: 'Save' })).toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Share' })).toBeNull()
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

  describe('double-click', () => {
    beforeEach(() => {
      stages.length = 0
    })
    const win = { x: 100, y: 50, width: 400, height: 300 }

    it('selects the window under the pointer, as Rust found it', async () => {
      windowAt = () => win
      await shown()
      dblclick({ x: 150, y: 80 })
      await waitFor(() => expect(selection()).not.toBeNull())
      expect(rect()).toEqual([100, 50, 400, 300])
      expect(size()).toBe('800 × 600')
      expect(called('overlay_window_at')[0].args).toEqual({
        session: 9,
        x: 150,
        y: 80,
        width: 1000,
        height: 700,
      })
      // Then it is a normal selection.
      expect(hint()).toContain('Enter copy')
      drag({ x: 500, y: 350 }, { x: 520, y: 360 }, handle('se'))
      expect(rect()).toEqual([100, 50, 420, 310])
    })

    it('selects the whole display when Rust has no window there', async () => {
      await shown()
      dblclick({ x: 10, y: 10 })
      await waitFor(() => expect(selection()).not.toBeNull())
      expect(rect()).toEqual([0, 0, 1000, 700])
    })

    it('selects the whole display when Rust cannot answer', async () => {
      const error = vi.spyOn(console, 'error').mockImplementation(() => {})
      windowAt = () => {
        throw new Error('gone')
      }
      await shown()
      dblclick({ x: 10, y: 10 })
      await waitFor(() => expect(selection()).not.toBeNull())
      expect(rect()).toEqual([0, 0, 1000, 700])
      expect(error).toHaveBeenCalled()
    })

    it('selects the whole display with Ctrl, without asking Rust', async () => {
      windowAt = () => win
      await shown()
      // ⌘ is not the modifier off the Mac: it picks the window.
      dblclick({ x: 150, y: 80 }, { metaKey: true })
      await waitFor(() => expect(rect()).toEqual([100, 50, 400, 300]))
      dblclick({ x: 600, y: 600 }, { ctrlKey: true })
      expect(rect()).toEqual([0, 0, 1000, 700])
      expect(called('overlay_window_at')).toHaveLength(1)
    })

    it('selects the whole display with ⌘ on macOS', async () => {
      platform.mac = true
      windowAt = () => win
      await shown()
      dblclick({ x: 150, y: 80 }, { metaKey: true })
      expect(rect()).toEqual([0, 0, 1000, 700])
      expect(called('overlay_window_at')).toHaveLength(0)
    })

    it('replaces a selection when double-clicked outside it', async () => {
      windowAt = () => win
      await shown()
      drag({ x: 600, y: 500 }, { x: 700, y: 600 })
      dblclick({ x: 150, y: 80 })
      await waitFor(() => expect(rect()).toEqual([100, 50, 400, 300]))
    })

    it('ignores a double-click on the selection or on a control', async () => {
      windowAt = () => win
      await shown()
      drag({ x: 600, y: 500 }, { x: 700, y: 600 })
      dblclick({ x: 650, y: 550 }, {}, selection()!)
      dblclick({ x: 650, y: 550 }, {}, handle('se'))
      dblclick({ x: 650, y: 550 }, {}, field('Width'))
      await tick()
      expect(rect()).toEqual([600, 500, 100, 100])
      expect(called('overlay_window_at')).toHaveLength(0)
    })

    it('keeps shown annotations rather than pick a window', async () => {
      windowAt = () => win
      await annotated()
      dblclick({ x: 600, y: 600 })
      await tick()
      expect(rect()).toEqual([10, 10, 100, 50])
      expect(called('overlay_window_at')).toHaveLength(0)
    })

    it('drops an answer that comes after the overlay closed or a new drag began', async () => {
      let answer!: (r: unknown) => void
      windowAt = () => new Promise((resolve) => (answer = resolve))
      await shown()
      dblclick({ x: 150, y: 80 })
      await waitFor(() => expect(called('overlay_window_at')).toHaveLength(1))
      fireEvent.pointerDown(overlay(), point({ x: 300, y: 300 }))
      answer(win)
      await tick()
      expect(selection()).toBeNull()
      fireEvent.pointerUp(overlay(), point({ x: 300, y: 300 }))

      dblclick({ x: 150, y: 80 })
      await waitFor(() => expect(called('overlay_window_at')).toHaveLength(2))
      await emit('overlay:closed')
      answer(win)
      await tick()
      expect(selection()).toBeNull()
    })

    it('keeps a region drawn by the second press of a double-click', async () => {
      windowAt = () => win
      await shown()
      fireEvent.pointerDown(overlay(), point({ x: 600, y: 500 }))
      fireEvent.pointerUp(overlay(), point({ x: 600, y: 500 }))
      drag({ x: 600, y: 500 }, { x: 700, y: 600 })
      fireEvent.dblClick(overlay(), { clientX: 700, clientY: 600 })
      await tick()
      expect(rect()).toEqual([600, 500, 100, 100])
      expect(called('overlay_window_at')).toHaveLength(0)
    })

    it('drops an answer that comes after a newer click or double-click', async () => {
      const replies: ((r: unknown) => void)[] = []
      windowAt = () => new Promise((resolve) => replies.push(resolve))
      await shown()
      dblclick({ x: 150, y: 80 })
      await waitFor(() => expect(replies).toHaveLength(1))
      // A click: the first answer no longer counts.
      fireEvent.pointerDown(overlay(), point({ x: 900, y: 650 }))
      fireEvent.pointerUp(overlay(), point({ x: 900, y: 650 }))
      replies[0](win)
      await tick()
      expect(selection()).toBeNull()

      // Two double-clicks: only the newer one's answer selects.
      dblclick({ x: 150, y: 80 })
      dblclick({ x: 800, y: 600 })
      await waitFor(() => expect(replies).toHaveLength(3))
      replies[1](win)
      await tick()
      expect(selection()).toBeNull()
      replies[2]({ x: 700, y: 500, width: 200, height: 100 })
      await waitFor(() => expect(rect()).toEqual([700, 500, 200, 100]))
    })

    it('does not select on the wobble of a click', async () => {
      await shown()
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 12, y: 11 }))
      fireEvent.pointerUp(overlay(), point({ x: 12, y: 11 }))
      expect(selection()).toBeNull()
      // Past the slop, it draws from the press, and can then come back.
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 20, y: 20 }))
      fireEvent.pointerMove(overlay(), point({ x: 12, y: 11 }))
      fireEvent.pointerUp(overlay(), point({ x: 12, y: 11 }))
      expect(rect()).toEqual([10, 10, 2, 1])
    })

    it('copies at once with quick copy on', async () => {
      windowAt = () => win
      await quick()
      dblclick({ x: 150, y: 80 })
      await waitFor(() => expect(called('overlay_copy')).toHaveLength(1))
      expect(output('overlay_copy', 0)).toMatchObject({
        rect: { x: 200, y: 100, width: 800, height: 600 },
        remember: false,
      })
    })

    it('takes no double-click while a quick copy is on its way', async () => {
      finished = () => new Promise(() => {})
      await quick()
      dblclick({ x: 150, y: 80 }, { ctrlKey: true })
      await waitFor(() => expect(called('overlay_copy')).toHaveLength(1))
      dblclick({ x: 150, y: 80 }, { ctrlKey: true })
      await tick()
      expect(called('overlay_copy')).toHaveLength(1)
    })
  })

  describe('quick copy', () => {
    beforeEach(() => {
      stages.length = 0
    })

    it('shows the Quick copy bar instead of the key hint', async () => {
      await quick()
      expect(bar()).toHaveTextContent(
        'Quick copy · drag or double-click a window to copy Annotate this time Turn off Esc cancel',
      )
      expect(screen.queryByTestId('hint')).toBeNull()
    })

    it('copies as soon as the drag ends, and says it is a quick copy', async () => {
      await quick()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await waitFor(() =>
        expect(output('overlay_copy', 0)).toEqual({
          session: 9,
          rect: rect200,
          blurs: [],
          remember: false,
        }),
      )
      expect(called('overlay_copy')).toHaveLength(1)
      // No toolbar or annotations meanwhile.
      expect(stages).toHaveLength(0)
      expect(bar()).toBeInTheDocument()
    })

    it('copies a drag let go while Space moves it', async () => {
      await quick()
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 60, y: 40 }))
      await fireEvent.keyDown(window, { key: ' ' })
      fireEvent.pointerMove(overlay(), point({ x: 80, y: 50 }))
      fireEvent.pointerUp(overlay(), point({ x: 80, y: 50 }))
      await waitFor(() => expect(called('overlay_copy')).toHaveLength(1))
      expect(output('overlay_copy', 0)).toMatchObject({ remember: false })
    })

    it('copies nothing for a click, a cancelled drag or a move', async () => {
      await quick()
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerUp(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 110, y: 60 }))
      fireEvent.pointerCancel(overlay(), point({ x: 110, y: 60 }))
      expect(selection()).not.toBeNull()
      fireEvent.pointerDown(overlay(), point({ x: 20, y: 20 }))
      fireEvent.pointerMove(overlay(), point({ x: 30, y: 30 }))
      fireEvent.pointerUp(overlay(), point({ x: 30, y: 30 }))
      await tick()
      expect(called('overlay_copy')).toHaveLength(0)
    })

    it('remembers nothing in a quick copy capture, a save included', async () => {
      vi.spyOn(console, 'error').mockImplementation(() => {})
      finished = (cmd) => {
        if (cmd === 'overlay_copy') throw new Error('busy')
        return null
      }
      await quick()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await waitFor(() => expect(called('overlay_copy')).toHaveLength(1))
      await userEvent.keyboard(ctrl('s'))
      await waitFor(() =>
        expect(output('overlay_save', 0)).toEqual({
          session: 9,
          rect: rect200,
          blurs: [],
          remember: false,
        }),
      )
    })

    it('keeps the magnifier', async () => {
      await quick({ magnifier: true })
      fireEvent.pointerDown(overlay(), point({ x: 10, y: 10 }))
      fireEvent.pointerMove(overlay(), point({ x: 20, y: 30 }))
      expect(screen.getByTestId('magnifier')).toHaveTextContent('40, 60')
    })

    it('leaves the bar alone when pressed', async () => {
      await quick()
      fireEvent.pointerDown(bar()!, point({ x: 500, y: 680 }))
      fireEvent.pointerMove(overlay(), point({ x: 600, y: 600 }))
      fireEvent.pointerUp(overlay(), point({ x: 600, y: 600 }))
      await tick()
      expect(selection()).toBeNull()
      expect(called('overlay_copy')).toHaveLength(0)
    })

    it('takes no second drag while a quick copy is on its way', async () => {
      let copied!: () => void
      const copying = new Promise<void>((resolve) => {
        copied = resolve
      })
      finished = (cmd) => (cmd === 'overlay_copy' ? copying : null)
      await quick()
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await waitFor(() => expect(called('overlay_copy')).toHaveLength(1))
      drag({ x: 300, y: 300 }, { x: 400, y: 400 })
      expect(rect()).toEqual([10, 10, 100, 50])
      copied()
      await tick()
      expect(called('overlay_copy')).toHaveLength(1)
    })

    it('annotates this time on every display', async () => {
      await twoDisplays()
      const [annotate] = screen.getAllByRole('button', { name: 'Annotate this time' })
      await userEvent.click(annotate)
      await waitFor(() => expect(bars()).toHaveLength(0))
      expect(screen.getAllByTestId('hint')).toHaveLength(2)
    })

    it('annotates this time only, as usual', async () => {
      await quick()
      await userEvent.click(screen.getByRole('button', { name: 'Annotate this time' }))
      await waitFor(() => expect(bar()).toBeNull())
      expect(hint()).toEqual([
        'Drag select area',
        'Double-click window',
        'Ctrl double-click full screen',
        'Esc cancel',
      ])
      drag({ x: 10, y: 10 }, { x: 110, y: 60 })
      await waitFor(() => expect(stages).toHaveLength(1))
      expect(called('overlay_copy')).toHaveLength(0)
      expect(called('set_capture_option')).toHaveLength(0)
    })

    it('follows another display, but not an older capture', async () => {
      await twoDisplays()
      await emit('overlay:annotate', { session: 8 })
      await tick()
      expect(bars()).toHaveLength(2)
      await emit('overlay:annotate', { session: 9 })
      await waitFor(() => expect(bars()).toHaveLength(0))
    })

    it('turns quick copy off and goes on as usual', async () => {
      await quick()
      await userEvent.click(screen.getByRole('button', { name: 'Turn off' }))
      await waitFor(() => expect(bar()).toBeNull())
      expect(called('set_capture_option')[0].args).toEqual({ key: 'quickCopy', on: false })
      expect(screen.getByTestId('hint')).toBeInTheDocument()
      resetSettings()
    })

    it('says where to turn it back on, on hover and keyboard focus', async () => {
      await quick()
      const tip = 'You can turn it back on in Settings.'
      expect(screen.queryByText(tip)).toBeNull()
      await userEvent.tab()
      expect(screen.getByRole('button', { name: 'Annotate this time' })).toHaveFocus()
      await userEvent.tab()
      expect(screen.getByRole('button', { name: 'Turn off' })).toHaveFocus()
      const content = { selector: '[data-slot=tooltip-content]' }
      expect(await screen.findByText(tip, content)).toBeInTheDocument()
      // Enter presses the focused button only: it does not copy.
      await userEvent.keyboard('{Enter}')
      await waitFor(() => expect(bar()).toBeNull())
      expect(called('overlay_copy')).toHaveLength(0)
      resetSettings()
    })

    it('says it on hover too', async () => {
      await quick()
      await userEvent.hover(screen.getByRole('button', { name: 'Turn off' }))
      const tip = 'You can turn it back on in Settings.'
      expect(
        await screen.findByText(tip, { selector: '[data-slot=tooltip-content]' }),
      ).toBeVisible()
    })

    it('is off again for the next capture', async () => {
      await quick()
      await emit('overlay:closed')
      await emit('overlay:show', { ...info, session: 10 })
      await waitFor(() => expect(called('overlay_painted')).toHaveLength(2))
      expect(bar()).toBeNull()
      expect(screen.getByTestId('hint')).toBeInTheDocument()
    })
  })
})
