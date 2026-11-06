import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { stages, type FakeNode } from '../../test/konva'
import type { Rect } from '../geometry'
import { fakeContext } from '../../test/canvas'
import { isMac } from '$lib/settings.svelte'
import Annotator from './Annotator.svelte'
import { remember } from './memory'
import { OUTPUT_HEADER, outputRequest } from './output'
import type * as board from './board'

// Konva loads as usual, unless a test makes the next loads fail.
const konva = vi.hoisted(() => ({ failures: 0 }))
vi.mock('./board', async (original) => {
  const m = await original<typeof board>()
  return {
    ...m,
    loadKonva: () => (konva.failures-- > 0 ? Promise.reject(new Error('offline')) : m.loadKonva()),
  }
})

// The lazy chunks compile on first import, which is slow on CI; do it once up front.
beforeAll(async () => {
  await Promise.all([import('./Toolbar.svelte'), import('konva')])
}, 30_000)

const display = { width: 1000, height: 700 }
const scale = { x: 2, y: 2 }
const options = { rememberRegion: true, magnifier: false }
const sel: Rect = { x: 100, y: 100, width: 400, height: 300 }

type Api = {
  key: (e: KeyboardEvent) => boolean
  escape: () => boolean
  reset: (frame?: boolean) => void
  exportAnnotations: (
    crop: Rect,
    scale: { x: number; y: number },
  ) => Promise<board.AnnotationExport>
  annotated: () => boolean
  painted: () => Rect | null
  menuOpen: () => boolean
}
type Props = {
  selection: Rect | null
  actions?: Record<string, (...args: never[]) => void>
}

const props = (p: Props) => ({ display, scale, options, ...p })

/** Renders with a selection and waits for the stage and the toolbar. */
async function setup(p: Props = { selection: sel }) {
  const view = render(Annotator, { props: props(p) })
  await waitFor(() => expect(stages).toHaveLength(1))
  await toolbar()
  return { view, api: view.component as unknown as Api }
}

// The toolbar is loaded on demand; the first load in a run compiles it, which can be slow.
const toolbar = () => screen.findByRole('toolbar', {}, { timeout: 5000 })
const stage = () => stages[0] as FakeNode & { hit: FakeNode | null }
/** The shapes' layer, clipped to the selection. */
const layer = () => stage().children[0]
/** The shape being drawn and the handles. */
const top = () => stage().children[1]
const transformer = () => top().children[0] as FakeNode & { attached: FakeNode[] }
const draft = () => top().children[1]
const nodes = () => layer().children
const kinds = () => nodes().map((n) => n.className)
const element = () => screen.getByTestId('annotations')
const at = (x: number, y: number) => ({ clientX: x, clientY: y, button: 0, pointerId: 1 })
const press = (key: string, init: KeyboardEventInit = {}) =>
  new KeyboardEvent('keydown', { key, cancelable: true, ...init })
/** ⌘ on macOS, Ctrl elsewhere. */
const mod = isMac ? { metaKey: true } : { ctrlKey: true }

/** The text editor, if open. */
const editor = () => screen.queryByRole<HTMLTextAreaElement>('textbox', { name: 'Text' })
/** The text drawn by each text node on the board. */
const texts = () => nodes().map((n) => n.children[1]?.attrs.text)

/** Drags on the annotation layer, in window px. */
async function drag(x1: number, y1: number, x2: number, y2: number) {
  await fireEvent.pointerDown(element(), at(x1, y1))
  await fireEvent.pointerMove(element(), at(x2, y2))
  await fireEvent.pointerUp(element(), at(x2, y2))
}

/** Picks the text tool and clicks at (x, y): the editor opens there. */
async function place(api: Api, x = 150, y = 150) {
  api.key(press('t'))
  await drag(x, y, x, y)
  await waitFor(() => expect(editor()).toHaveFocus())
  return editor()!
}

describe('Annotator', () => {
  let passed: ReturnType<typeof vi.fn<(e: Event) => void>>

  beforeEach(() => {
    stages.length = 0
    passed = vi.fn<(e: Event) => void>()
    document.addEventListener('pointerdown', passed)
  })

  afterEach(() => {
    document.removeEventListener('pointerdown', passed)
    vi.restoreAllMocks()
  })

  it('builds no stage until there is a selection', async () => {
    const view = render(Annotator, { props: props({ selection: null }) })
    await new Promise((r) => setTimeout(r, 300))
    expect(stages).toHaveLength(0)
    expect(screen.queryByRole('toolbar')).not.toBeInTheDocument()
    expect(element().style.width).toBe('0px')

    await view.rerender(props({ selection: sel }))
    await waitFor(() => expect(stages).toHaveLength(1))
    await toolbar()
  })

  it('covers the display and clips to the selection, and shrinks to nothing without one', async () => {
    const { view } = await setup()
    expect(element().style).toMatchObject({ width: '1000px', height: '700px' })
    expect(stage().attrs).toMatchObject(display)
    expect(layer().attrs.clip).toEqual(sel)
    await view.rerender(props({ selection: null }))
    expect(stage().attrs).toMatchObject({ width: 0, height: 0 })
    expect(screen.queryByRole('toolbar')).not.toBeInTheDocument()
  })

  it('puts the toolbar below the selection', async () => {
    await setup()
    const bar = screen.getByRole('toolbar').closest<HTMLElement>('[style*=left]')!
    // jsdom lays nothing out, so the toolbar has no size: right-aligned is the right edge.
    expect(bar.style.left).toBe('500px')
    expect(bar.style.top).toBe('406px')
  })

  it('draws a rect with R and an arrow with A, in display coordinates', async () => {
    const { api } = await setup()
    expect(api.key(press('r'))).toBe(true)
    await drag(150, 150, 250, 200)
    expect(kinds()).toEqual(['Rect'])
    expect(nodes()[0].attrs).toMatchObject({ x: 150, y: 150, width: 100, height: 50 })
    api.key(press('A'))
    await drag(150, 150, 50, 50)
    expect(kinds()).toEqual(['Rect', 'Arrow'])
    // Clamped to the selection.
    expect(nodes()[1].attrs).toMatchObject({ x: 150, y: 150, points: [0, 0, -50, -50] })
    expect(passed).not.toHaveBeenCalled()
  })

  it('keeps shapes on the same pixels when the selection moves', async () => {
    const { api, view } = await setup()
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    await view.rerender(props({ selection: { ...sel, x: 120, y: 90 } }))
    expect(nodes()[0].attrs).toMatchObject({ x: 150, y: 150 })
    expect(layer().attrs.clip).toEqual({ ...sel, x: 120, y: 90 })
  })

  it('shows the shape while it is drawn, and drops it on cancel', async () => {
    const { api } = await setup()
    api.key(press('r'))
    await fireEvent.pointerDown(element(), at(150, 150))
    await fireEvent.pointerMove(element(), at(200, 220))
    expect(draft().attrs).toMatchObject({ width: 50, height: 70, listening: false })
    await fireEvent.pointerCancel(element(), at(200, 220))
    expect(kinds()).toEqual([])
    // A click (no drag) draws nothing either.
    await drag(150, 150, 151, 151)
    expect(kinds()).toEqual([])
    // Moves with no press do nothing.
    await fireEvent.pointerMove(element(), at(300, 300))
    await fireEvent.pointerUp(element(), at(300, 300))
    expect(kinds()).toEqual([])
  })

  it('lets a press outside the selection through, even with a tool', async () => {
    const { api } = await setup()
    api.key(press('r'))
    await drag(600, 500, 700, 600)
    expect(kinds()).toEqual([])
    expect(passed).toHaveBeenCalledOnce()
  })

  it('shows a cursor for what a press on the selection does', async () => {
    const { api } = await setup()
    await fireEvent.pointerMove(element(), at(150, 150))
    expect(element().style.cursor).toBe('move')
    api.key(press('r'))
    await fireEvent.pointerMove(element(), at(150, 151))
    expect(element().style.cursor).toBe('crosshair')
    await fireEvent.pointerMove(element(), at(600, 500))
    expect(element().style.cursor).toBe('')
  })

  it('leaves keys to an open menu', async () => {
    const user = userEvent.setup()
    const { api } = await setup()
    await user.click(screen.getByRole('button', { name: 'More' }))
    await screen.findByText('Magnifier')
    expect(api.menuOpen()).toBe(true)
    expect(api.key(press('r'))).toBe(false)
  })

  it('lets a press with no tool through to the overlay, to move the selection', async () => {
    await setup()
    await drag(150, 150, 250, 250)
    expect(kinds()).toEqual([])
    expect(passed).toHaveBeenCalledOnce()
    await fireEvent.pointerDown(element(), { ...at(150, 150), button: 2 })
    expect(passed).toHaveBeenCalledTimes(2)
  })

  it('selects, moves, nudges and deletes a shape, with undo', async () => {
    const { api } = await setup()
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    const node = nodes()[0]
    // A press on the shape is Konva's: it selects it and the overlay never sees it.
    stage().hit = node
    node.fire('pointerdown')
    await fireEvent.pointerDown(element(), at(160, 160))
    expect(passed).not.toHaveBeenCalled()
    await waitFor(() => expect(transformer().attached).toEqual([node]))

    node.setAttrs({ x: 170, y: 180 })
    node.fire('dragend')
    await waitFor(() => expect(node.attrs).toMatchObject({ x: 170, y: 180 }))

    // Arrow keys move the shape by whole physical pixels (0.5 logical px at 2x).
    expect(api.key(press('ArrowRight'))).toBe(true)
    expect(api.key(press('ArrowDown', { shiftKey: true }))).toBe(true)
    await waitFor(() => expect(node.attrs).toMatchObject({ x: 170.5, y: 185 }))

    expect(api.key(press('Backspace'))).toBe(true)
    await waitFor(() => expect(kinds()).toEqual([]))
    api.key(press('z', mod))
    await waitFor(() => expect(kinds()).toEqual(['Rect']))
    expect(nodes()[0].attrs).toMatchObject({ x: 170.5, y: 185 })
    api.key(press('z', mod))
    api.key(press('z', mod))
    api.key(press('z', mod))
    await waitFor(() => expect(nodes()[0].attrs).toMatchObject({ x: 150, y: 150 }))
    api.key(press('Z', { ...mod, shiftKey: true }))
    await waitFor(() => expect(nodes()[0].attrs).toMatchObject({ x: 170, y: 180 }))
  })

  it('pulls a shape dragged out of the selection back in', async () => {
    const { api } = await setup()
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    const node = nodes()[0]
    node.setAttrs({ x: 900, y: 650 })
    node.fire('dragend')
    // 8 px of it still shows inside the selection's bottom-right corner.
    await waitFor(() => expect(node.attrs).toMatchObject({ x: 492, y: 392 }))
  })

  it('drops the shape selection when its shape goes away', async () => {
    const { api } = await setup()
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    nodes()[0].fire('pointerdown')
    await waitFor(() => expect(transformer().attached).toHaveLength(1))
    api.key(press('z', mod))
    await waitFor(() => expect(transformer().attached).toEqual([]))
    api.key(press('z', { ...mod, shiftKey: true }))
    await waitFor(() => expect(kinds()).toEqual(['Rect']))
    // Nothing selected now: Delete is not ours.
    expect(api.key(press('Delete'))).toBe(false)
  })

  it('drops the shape selection on a press elsewhere', async () => {
    const { api } = await setup()
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    nodes()[0].fire('pointerdown')
    await waitFor(() => expect(transformer().attached).toHaveLength(1))
    await drag(400, 300, 400, 300)
    await waitFor(() => expect(transformer().attached).toEqual([]))
  })

  it('undoes Esc one step at a time', async () => {
    const user = userEvent.setup()
    const { api } = await setup()
    expect(api.escape()).toBe(false)
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    // Drawing: Esc drops the shape being drawn.
    await fireEvent.pointerDown(element(), at(300, 300))
    await fireEvent.pointerMove(element(), at(350, 350))
    expect(api.escape()).toBe(true)
    await fireEvent.pointerUp(element(), at(350, 350))
    expect(kinds()).toEqual(['Rect'])
    // Then an open menu.
    nodes()[0].fire('pointerdown')
    await user.click(screen.getByRole('button', { name: 'More' }))
    await screen.findByText('Magnifier')
    expect(api.escape()).toBe(true)
    await waitFor(() => expect(screen.queryByText('Magnifier')).not.toBeInTheDocument())
    // Then the selected shape, then the tool.
    await waitFor(() => expect(transformer().attached).toHaveLength(1))
    expect(api.escape()).toBe(true)
    await waitFor(() => expect(transformer().attached).toEqual([]))
    expect(api.escape()).toBe(true)
    await waitFor(() =>
      expect(screen.getByLabelText('Rectangle')).toHaveAttribute('data-state', 'off'),
    )
    expect(api.escape()).toBe(false)
  })

  it('clears all from the toolbar, and undoes it', async () => {
    const user = userEvent.setup()
    const { api } = await setup()
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    await drag(300, 300, 350, 350)
    await user.click(screen.getByRole('button', { name: 'Clear all' }))
    expect(kinds()).toEqual([])
    await user.click(screen.getByRole('button', { name: 'Undo' }))
    expect(kinds()).toEqual(['Rect', 'Rect'])
    await user.click(screen.getByRole('button', { name: 'Redo' }))
    expect(kinds()).toEqual([])
  })

  it('picks a tool from the toolbar', async () => {
    const user = userEvent.setup()
    await setup()
    await user.click(screen.getByLabelText('Arrow'))
    await drag(150, 150, 250, 250)
    expect(kinds()).toEqual(['Arrow'])
  })

  it('leaves keys alone while typing, with modifiers, for tools not ready, or unknown', async () => {
    const { api } = await setup()
    const input = document.createElement('input')
    document.body.append(input)
    const typed = press('r')
    Object.defineProperty(typed, 'target', { value: input })
    expect(api.key(typed)).toBe(false)
    expect(api.key(press('r', { altKey: true }))).toBe(false)
    expect(api.key(press('c', mod))).toBe(false)
    // Ctrl on macOS, or ⌘ elsewhere, is not undo.
    expect(api.key(press('z', isMac ? { ctrlKey: true } : { metaKey: true }))).toBe(false)
    expect(api.key(press('x'))).toBe(false)
    expect(api.key(press('ArrowLeft'))).toBe(false)
    input.remove()
  })

  it('leaves keys alone with no selection', () => {
    const view = render(Annotator, { props: props({ selection: null }) })
    const api = view.component as unknown as Api
    expect(api.key(press('r'))).toBe(false)
  })

  it('says whether there are annotations and where they paint, and resets', async () => {
    const { api } = await setup()
    expect(api.annotated()).toBe(false)
    expect(api.painted()).toBeNull()
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    expect(api.annotated()).toBe(true)
    expect(api.painted()).toEqual({ x: 146, y: 146, width: 108, height: 108 })
    api.reset()
    expect(api.annotated()).toBe(false)
    await waitFor(() => expect(kinds()).toEqual([]))
    // The last tool stays for the next capture.
    await drag(150, 150, 250, 250)
    expect(kinds()).toEqual(['Rect'])
  })

  it('remembers the last tool, color and size for the next time', async () => {
    const user = userEvent.setup()
    const first = await setup()
    first.api.key(press('e'))
    await user.click(screen.getByRole('button', { name: 'Color and size' }))
    await user.click(await screen.findByLabelText('Thick'))
    // Dropping the tool keeps the last one used.
    first.api.escape()
    first.api.escape()
    await waitFor(() =>
      expect(screen.getByLabelText('Ellipse')).toHaveAttribute('data-state', 'off'),
    )
    first.api.reset()
    await waitFor(() =>
      expect(screen.getByLabelText('Ellipse')).toHaveAttribute('data-state', 'on'),
    )
    first.view.unmount()

    stages.length = 0
    const second = await setup()
    expect(screen.getByLabelText('Ellipse')).toHaveAttribute('data-state', 'on')
    // Without a selection, the remembered tool does not keep Esc from closing the overlay.
    await second.view.rerender(props({ selection: null }))
    expect(second.api.escape()).toBe(false)
    await second.view.rerender(props({ selection: sel }))
    await drag(150, 150, 250, 200)
    expect(nodes()[0]).toMatchObject({ className: 'Ellipse' })
    expect(nodes()[0].attrs).toMatchObject({ strokeWidth: 8, radiusX: 50, radiusY: 25 })
  })

  it('takes up what another overlay window remembered on the next capture', async () => {
    const { api } = await setup()
    remember({ tool: 'line', style: { color: '#007AFF', size: 2 } })
    api.reset(true)
    await waitFor(() => expect(screen.getByLabelText('Line')).toHaveAttribute('data-state', 'on'))
    await drag(150, 150, 250, 150)
    expect(nodes()[0].attrs).toMatchObject({ stroke: '#007AFF', strokeWidth: 2 })
  })

  it('restyles the selected shape, one undo step each; a blur keeps none', async () => {
    const user = userEvent.setup()
    const { api } = await setup()
    api.key(press('e'))
    await drag(150, 150, 250, 200)
    api.key(press('b'))
    await drag(300, 300, 350, 350)
    const [ellipse, blur] = [nodes()[1], nodes()[0]]
    ellipse.fire('pointerdown')
    await user.click(screen.getByRole('button', { name: 'Color and size' }))
    await user.click(await screen.findByLabelText('Color #007AFF'))
    await waitFor(() => expect(ellipse.attrs.stroke).toBe('#007AFF'))
    await user.click(screen.getByLabelText('Thick'))
    await waitFor(() => expect(ellipse.attrs.strokeWidth).toBe(8))
    // The toolbar shows the shape's style, not the tool's.
    expect(screen.getByTestId('current-color').style.background).toBe('rgb(0, 122, 255)')
    api.escape()
    api.escape()
    await waitFor(() =>
      expect(screen.getByTestId('current-color').style.background).toBe('rgb(255, 59, 48)'),
    )
    api.key(press('z', mod))
    await waitFor(() => expect(ellipse.attrs.strokeWidth).toBe(4))
    expect(ellipse.attrs.stroke).toBe('#007AFF')
    api.key(press('z', mod))
    await waitFor(() => expect(ellipse.attrs.stroke).toBe('#FF3B30'))
    // A selected blur has no color: the tool takes it, and the blur is left alone.
    blur.fire('pointerdown')
    await user.click(screen.getByRole('button', { name: 'Color and size' }))
    await user.click(await screen.findByLabelText('Color #34C759'))
    await waitFor(() =>
      expect(screen.getByTestId('current-color').style.background).toBe('rgb(52, 199, 89)'),
    )
    // No undo step: the next undo takes the blur itself away.
    api.escape()
    api.key(press('z', mod))
    await waitFor(() => expect(kinds()).toEqual(['Ellipse']))
  })

  it('draws a freehand pen stroke through the points it passes', async () => {
    const { api } = await setup()
    api.key(press('p'))
    await fireEvent.pointerDown(element(), at(150, 150))
    await fireEvent.pointerMove(element(), at(160, 150))
    const line = draft()
    expect(line).toMatchObject({ className: 'Line' })
    expect(line.attrs.points).toEqual([0, 0, 10, 0])
    // A move within a pixel or so adds nothing, and does not redraw.
    const draws = (top() as FakeNode & { draws: number }).draws
    await fireEvent.pointerMove(element(), at(161, 150))
    expect((top() as FakeNode & { draws: number }).draws).toBe(draws)
    await fireEvent.pointerMove(element(), at(170, 170))
    // Past the selection, the stroke stays on its edge.
    await fireEvent.pointerUp(element(), at(170, 900))
    expect(kinds()).toEqual(['Line'])
    const points = nodes()[0].attrs.points as number[]
    expect(points.slice(0, 2)).toEqual([0, 0])
    expect(points.slice(-2)).toEqual([20, 250])
    // Smoothed: more points than the four it went through.
    expect(points.length).toBeGreaterThan(8)
  })

  it('draws a wide, see-through highlighter with flat ends', async () => {
    const { api } = await setup()
    api.key(press('h'))
    await drag(150, 150, 250, 150)
    expect(nodes()[0].attrs).toMatchObject({ lineCap: 'butt', opacity: expect.any(Number) })
    expect(nodes()[0].attrs.strokeWidth).toBeGreaterThan(8)
  })

  it('snaps or unsnaps the shape being drawn when Shift alone changes', async () => {
    const { api } = await setup()
    api.key(press('l'))
    await fireEvent.pointerDown(element(), at(150, 150))
    await fireEvent.pointerMove(element(), at(250, 155))
    expect(draft().attrs.points).toEqual([0, 0, 100, 5])
    await fireEvent.keyDown(window, { key: 'Shift', shiftKey: true })
    expect(draft().attrs.points).toEqual([0, 0, expect.closeTo(100.12, 1), 0])
    // Other keys, and Shift with no drag, change nothing.
    await fireEvent.keyDown(window, { key: 'a' })
    await fireEvent.keyUp(window, { key: 'Shift' })
    expect(draft().attrs.points).toEqual([0, 0, 100, 5])
    await fireEvent.pointerUp(element(), at(250, 155))
    await fireEvent.keyUp(window, { key: 'Shift' })
    expect(nodes()[0].attrs.points).toEqual([0, 0, 100, 5])
  })

  it('snaps lines to 15° and squares boxes while Shift is down', async () => {
    const { api } = await setup()
    api.key(press('l'))
    await fireEvent.pointerDown(element(), at(150, 150))
    await fireEvent.pointerMove(element(), { ...at(250, 155), shiftKey: true })
    await fireEvent.pointerUp(element(), { ...at(250, 155), shiftKey: true })
    expect(nodes()[0].attrs.points).toEqual([0, 0, expect.closeTo(100.12, 1), 0])
    api.key(press('r'))
    await fireEvent.pointerDown(element(), at(150, 150))
    await fireEvent.pointerUp(element(), { ...at(250, 180), shiftKey: true })
    expect(nodes()[1].attrs).toMatchObject({ width: 100, height: 100 })
  })

  it('previews a blur as blocks of the frame, and exports it for Rust to pixelate', async () => {
    fakeContext()
    const source = document.createElement('canvas')
    source.width = 2000
    source.height = 1400
    const view = render(Annotator, { props: { ...props({ selection: sel }), source } })
    await waitFor(() => expect(stages).toHaveLength(1))
    await toolbar()
    const api = view.component as unknown as Api
    api.key(press('b'))
    await drag(150, 160, 250, 200)
    const node = nodes()[0]
    // 200 × 80 frame px in Rust's 16 px blocks.
    expect(node.attrs.blocks).toHaveLength(12 * 5)
    node.fire('pointerdown')
    api.key(press('ArrowRight'))
    await waitFor(() => expect(node.attrs.x).toBe(150.5))

    const crop = { x: 200, y: 200, width: 800, height: 600 }
    const out = await api.exportAnnotations(crop, scale)
    // The blur in physical px of the crop: (150.5, 160) at 2x, less the crop's corner.
    expect(out.blurs).toEqual([{ x: 101, y: 120, width: 200, height: 80 }])
    const { headers } = outputRequest(9, crop, out)
    expect(JSON.parse(headers[OUTPUT_HEADER])).toMatchObject({
      blurs: [{ x: 101, y: 120, width: 200, height: 80 }],
    })
  })

  it('keeps annotations while the selection is gone', async () => {
    const { api, view } = await setup()
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    await view.rerender(props({ selection: null }))
    await view.rerender(props({ selection: sel }))
    expect(kinds()).toEqual(['Rect'])
  })

  it('exports the annotations', async () => {
    fakeContext()
    const { api } = await setup()
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    const out = await api.exportAnnotations({ x: 200, y: 200, width: 800, height: 600 }, scale)
    expect(out.layer).toMatchObject({ x: 92, y: 92, width: 216, height: 216 })
    expect(out.blurs).toEqual([])
  })

  it('runs the given actions', async () => {
    const user = userEvent.setup()
    const copy = vi.fn<() => void>()
    const option = vi.fn<(key: string, on: boolean) => void>()
    await setup({ selection: sel, actions: { copy, option } })
    await user.click(screen.getByRole('button', { name: 'Copy' }))
    expect(copy).toHaveBeenCalledOnce()
    // The defaults do nothing yet.
    await user.click(screen.getByRole('button', { name: 'Save' }))
    await user.click(screen.getByRole('button', { name: 'More' }))
    await user.click((await screen.findByText('Magnifier')).closest('[role=menuitemcheckbox]')!)
    expect(option).toHaveBeenCalledWith('magnifier', true)
  })

  it('shows the tools as unavailable when Konva cannot load, and retries on a new selection', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    konva.failures = Infinity
    const view = render(Annotator, { props: props({ selection: sel }) })
    await toolbar()
    await waitFor(() => expect(screen.getByLabelText('Rectangle')).toBeDisabled())
    const api = view.component as unknown as Api
    expect(api.key(press('r'))).toBe(false)
    expect(error).toHaveBeenCalledTimes(1)
    // Moving the selection does not try again, nor does waiting.
    await view.rerender(props({ selection: { ...sel, x: 110 } }))
    await new Promise((r) => setTimeout(r, 50))
    expect(error).toHaveBeenCalledTimes(1)
    // A new selection tries once more.
    await view.rerender(props({ selection: null }))
    await view.rerender(props({ selection: sel }))
    await waitFor(() => expect(error).toHaveBeenCalledTimes(2))
    konva.failures = 0
    await view.rerender(props({ selection: null }))
    await view.rerender(props({ selection: sel }))
    await waitFor(() => expect(stages).toHaveLength(1))
    await waitFor(() => expect(screen.getByLabelText('Rectangle')).toBeEnabled())
  })

  it('destroys the stage when it goes away', async () => {
    const { view } = await setup()
    view.unmount()
    expect(stage().destroyed).toBe(true)
  })

  describe('text', () => {
    it('places a caret with a click, and commits the typed text on Esc', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      const field = await place(api)
      // Medium (24 px) with 7 px padding: the first line is centered on the click.
      expect(field.style).toMatchObject({ left: '143px', top: '128px', fontSize: '24px' })
      expect(kinds()).toEqual([])
      await user.type(field, 'Hello')
      expect(api.escape()).toBe(true)
      await waitFor(() => expect(editor()).not.toBeInTheDocument())
      expect(kinds()).toEqual(['Group'])
      expect(texts()).toEqual(['Hello'])
      // Measured as Konva lays it out (the fake: 12 px a character), padding included.
      expect(nodes()[0].attrs).toMatchObject({ x: 143, y: 128 })
      expect(nodes()[0].children[0].attrs).toMatchObject({ width: 74, height: 44 })
      expect(passed).not.toHaveBeenCalled()
      // One undo step.
      api.key(press('z', mod))
      await waitFor(() => expect(kinds()).toEqual([]))
    })

    it('commits on blur and on a press elsewhere, and drops empty text', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      await user.type(await place(api), 'A')
      await fireEvent.blur(editor()!)
      await waitFor(() => expect(texts()).toEqual(['A']))

      await user.type(await place(api, 300, 300), 'B')
      // A press on the board only commits: it starts nothing and the overlay never sees it.
      await fireEvent.pointerDown(element(), at(400, 200))
      await waitFor(() => expect(editor()).not.toBeInTheDocument())
      expect(texts()).toEqual(['A', 'B'])
      expect(passed).not.toHaveBeenCalled()

      await user.type(await place(api, 300, 200), '  \n ')
      api.escape()
      await place(api, 300, 250)
      api.escape()
      await waitFor(() => expect(editor()).not.toBeInTheDocument())
      expect(texts()).toEqual(['A', 'B'])
      // The empty ones left no undo step.
      api.key(press('z', mod))
      await waitFor(() => expect(texts()).toEqual(['A']))
    })

    it('only commits on a press on the layer itself, opening no other text', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      await user.type(await place(api), 'A')
      // The whole click lands on the layer: it commits, and the text tool starts nothing.
      await fireEvent.pointerDown(element(), at(300, 300))
      await fireEvent.pointerUp(element(), at(300, 300))
      await waitFor(() => expect(texts()).toEqual(['A']))
      expect(editor()).not.toBeInTheDocument()
      // The next click places a caret as usual.
      await drag(300, 300, 300, 300)
      await waitFor(() => expect(editor()).toBeInTheDocument())
    })

    it('keeps text as typed and where the editor was, clipped by the selection', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      // Near the selection's right edge (500): the text runs past it.
      await user.type(await place(api, 480, 200), 'Hello world{Enter}  ')
      expect(editor()!.style).toMatchObject({ left: '473px', top: '178px' })
      api.escape()
      await waitFor(() => expect(texts()).toEqual(['Hello world\n  ']))
      // Not moved: it is clipped like any shape. Both lines are measured, the last one too.
      expect(nodes()[0].attrs).toMatchObject({ x: 473, y: 178 })
      expect(nodes()[0].children[0].attrs).toMatchObject({ width: 146, height: 74 })
    })

    it('caps a long paste, and measures only what is kept', async () => {
      const { api } = await setup()
      const field = await place(api)
      expect(field.maxLength).toBe(2000)
      // jsdom does not apply maxlength to a set value: the measure and the commit still cut it.
      field.value = 'x'.repeat(5000)
      await fireEvent.input(field)
      // 2000 characters at 12 px, plus padding.
      await waitFor(() => expect(field.style.width).toBe(`${2000 * 12 + 14 + 2}px`))
      api.escape()
      await waitFor(() => expect(texts()).toEqual(['x'.repeat(2000)]))
    })

    it('adds no undo step for a text committed unchanged', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      api.key(press('r'))
      await drag(300, 300, 350, 350)
      await user.type(await place(api), 'Hi')
      api.escape()
      await waitFor(() => expect(texts()).toEqual([undefined, 'Hi']))
      nodes()[1].fire('dblclick')
      await waitFor(() => expect(editor()).toHaveValue('Hi'))
      api.escape()
      await waitFor(() => expect(editor()).not.toBeInTheDocument())
      // One undo takes the text away: the unchanged commit left no step.
      api.key(press('z', mod))
      await waitFor(() => expect(texts()).toEqual([undefined]))
    })

    it('keeps presses on the editor from the overlay', async () => {
      const { api } = await setup()
      await fireEvent.pointerDown(await place(api), at(150, 150))
      expect(passed).not.toHaveBeenCalled()
      expect(editor()).toBeInTheDocument()
    })

    it('edits a text again on double-click or Enter, as one undo step', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      await user.type(await place(api), 'Hi')
      api.escape()
      await waitFor(() => expect(texts()).toEqual(['Hi']))

      // Double-click: the text leaves the board while it is in the editor.
      nodes()[0].fire('dblclick')
      await waitFor(() => expect(editor()).toHaveValue('Hi'))
      expect(editor()!.style).toMatchObject({ left: '143px', top: '128px' })
      expect(kinds()).toEqual([])
      await user.type(editor()!, '!')
      api.escape()
      await waitFor(() => expect(texts()).toEqual(['Hi!']))
      api.key(press('z', mod))
      await waitFor(() => expect(texts()).toEqual(['Hi']))

      // Opened and closed with no change: no undo step.
      nodes()[0].fire('dblclick')
      await waitFor(() => expect(editor()).toBeInTheDocument())
      api.escape()
      await waitFor(() => expect(texts()).toEqual(['Hi']))

      // Enter on the selected text; emptying it deletes it.
      nodes()[0].fire('pointerdown')
      await waitFor(() => expect(transformer().attached).toHaveLength(1))
      const enter = press('Enter')
      expect(api.key(enter)).toBe(true)
      expect(enter.defaultPrevented).toBe(true)
      await waitFor(() => expect(editor()).toHaveValue('Hi'))
      expect(transformer().attached).toEqual([])
      await user.clear(editor()!)
      api.escape()
      await waitFor(() => expect(kinds()).toEqual([]))
      api.key(press('z', mod))
      await waitFor(() => expect(texts()).toEqual(['Hi']))
      api.key(press('z', mod))
      await waitFor(() => expect(kinds()).toEqual([]))
    })

    it('owns every key while text is edited, leaving it to the textarea', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      await user.type(await place(api), 'A')
      await fireEvent.blur(editor()!)
      await place(api, 300, 300)
      for (const e of [
        press('r'),
        press('c', mod),
        press('s', mod),
        press('z', mod),
        press('Enter'),
        press('ArrowLeft'),
        press('Backspace'),
      ]) {
        expect(api.key(e)).toBe(true)
        expect(e.defaultPrevented).toBe(false)
      }
      // Nothing was undone, and the tool is still Text.
      expect(texts()).toEqual(['A'])
      expect(screen.getByLabelText('Text', { selector: 'button' })).toHaveAttribute(
        'data-state',
        'on',
      )
    })

    it('shows a text cursor over the selection', async () => {
      const { api } = await setup()
      api.key(press('t'))
      await fireEvent.pointerMove(element(), at(150, 150))
      expect(element().style.cursor).toBe('text')
    })

    it('applies the text options, also to the text being typed', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      expect(screen.queryByLabelText('Text size')).not.toBeInTheDocument()
      const field = await place(api)
      await user.type(field, 'Hi')
      await user.click(screen.getByLabelText('Large text'))
      await user.click(screen.getByRole('button', { name: 'Background' }))
      expect(field.style.fontSize).toBe('36px')
      expect(field.style.background).not.toBe('transparent')
      api.escape()
      await waitFor(() => expect(texts()).toEqual(['Hi']))
      const [box, label] = nodes()[0].children
      expect(label.attrs).toMatchObject({ fontSize: 36, fill: '#000000' })
      expect(box.attrs.fill).toBe('#FF3B30')
      // The next text uses them too.
      await place(api, 300, 300)
      expect(editor()!.style.fontSize).toBe('36px')
    })

    it('shows the options of a text edited with another tool, keeping a resized size', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      await user.type(await place(api), 'Hi')
      api.escape()
      api.key(press('r'))
      await waitFor(() => expect(screen.queryByLabelText('Text size')).not.toBeInTheDocument())
      const node = nodes()[0]
      node.setAttrs({ scaleX: 1.25, scaleY: 1.25 })
      node.fire('transformend')
      await waitFor(() => expect(nodes()[0].children[1].attrs.fontSize).toBe(30))
      nodes()[0].fire('dblclick')
      await waitFor(() => expect(editor()!.style.fontSize).toBe('30px'))
      // No size option matches, and Background keeps the size.
      expect(screen.getByLabelText('Medium text')).toHaveAttribute('data-state', 'off')
      await user.click(screen.getByRole('button', { name: 'Background' }))
      expect(editor()!.style.fontSize).toBe('30px')
      await user.click(screen.getByLabelText('Small text'))
      expect(editor()!.style.fontSize).toBe('16px')
    })

    it('keeps a press on another shape from Konva while text is edited', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      api.key(press('r'))
      await drag(300, 300, 350, 350)
      await user.type(await place(api), 'Hi')
      // Konva listens on its own element in the layer.
      const content = document.createElement('div')
      element().append(content)
      const pressed = vi.fn<(e: Event) => void>()
      content.addEventListener('pointerdown', pressed)
      // The press commits the text, and neither Konva nor the overlay sees it.
      await fireEvent.pointerDown(content, at(310, 310))
      await waitFor(() => expect(editor()).not.toBeInTheDocument())
      expect(texts()).toEqual([undefined, 'Hi'])
      expect(pressed).not.toHaveBeenCalled()
      expect(passed).not.toHaveBeenCalled()
      // A right press does nothing.
      await user.type(await place(api, 200, 200), 'Yo')
      await fireEvent.pointerDown(element(), { ...at(310, 310), button: 2 })
      expect(editor()).toBeInTheDocument()
    })

    it('changes the color of the text being typed from the color menu', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      const field = await place(api)
      await user.type(field, 'Hi')
      await user.click(screen.getByRole('button', { name: 'Color and size' }))
      await user.click(await screen.findByLabelText('Color #007AFF'))
      // Still the active edit, in the new color.
      expect(field).toHaveFocus()
      expect(editor()!.style.color).toBe('rgb(0, 122, 255)')
      await user.type(field, '!')
      // Esc closes the menu first, then commits.
      expect(api.escape()).toBe(true)
      await waitFor(() => expect(screen.queryByLabelText('Color #007AFF')).toBeNull())
      expect(editor()).toHaveFocus()
      expect(api.escape()).toBe(true)
      await waitFor(() => expect(texts()).toEqual(['Hi!']))
      expect(nodes()[0].children[1].attrs.fill).toBe('#007AFF')
    })

    it('lets the hue slider move while text is typed, then gives the text its focus back', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      const field = await place(api)
      await user.type(field, 'Hi')
      field.setSelectionRange(1, 1)
      await user.click(screen.getByRole('button', { name: 'Color and size' }))
      const slider = await screen.findByLabelText<HTMLInputElement>('Custom color')
      const capture = vi.fn<(id: number) => void>()
      slider.setPointerCapture = capture
      // The press is not cancelled, so the slider can move and take focus.
      const down = new MouseEvent('mousedown', { bubbles: true, cancelable: true })
      slider.dispatchEvent(down)
      expect(down.defaultPrevented).toBe(false)
      await fireEvent.pointerDown(slider, { pointerId: 3 })
      expect(capture).toHaveBeenCalledWith(3)
      slider.focus()
      // The text stays open while the slider has focus.
      expect(editor()).toBe(field)
      await fireEvent.change(slider, { target: { value: '240' } })
      expect(field).toHaveFocus()
      expect(field.selectionStart).toBe(1)
      expect(field.style.color).toBe('rgb(0, 0, 255)')
      // A release without a change gives focus back too.
      await fireEvent.pointerDown(slider, { pointerId: 3 })
      slider.focus()
      await fireEvent.pointerUp(slider, { pointerId: 3 })
      expect(field).toHaveFocus()
      // Focus going anywhere else still commits.
      field.blur()
      await waitFor(() => expect(texts()).toEqual(['Hi']))
      expect(api.escape()).toBe(true)
    })

    it('drops the text being typed on Undo, and commits it before a tool or clear', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      // Undo, then type: Redo waits, since committing would drop what can be redone.
      api.key(press('r'))
      await drag(300, 300, 350, 350)
      api.key(press('z', mod))
      await waitFor(() => expect(kinds()).toEqual([]))
      await user.type(await place(api), 'A')
      expect(screen.getByRole('button', { name: 'Redo' })).toBeDisabled()
      // Undo drops the text being typed, and only that.
      await user.click(screen.getByRole('button', { name: 'Undo' }))
      await waitFor(() => expect(editor()).not.toBeInTheDocument())
      expect(kinds()).toEqual([])
      await user.click(screen.getByRole('button', { name: 'Redo' }))
      await waitFor(() => expect(kinds()).toEqual(['Rect']))

      await user.type(await place(api, 200, 200), 'C')
      await user.click(screen.getByLabelText('Rectangle'))
      await waitFor(() => expect(texts()).toEqual([undefined, 'C']))
      expect(editor()).not.toBeInTheDocument()
      await user.type(await place(api, 200, 250), 'D')
      await user.click(screen.getByRole('button', { name: 'Clear all' }))
      expect(kinds()).toEqual([])
      expect(editor()).not.toBeInTheDocument()
      // Clear is one step: undo brings the text back with the rest.
      await user.click(screen.getByRole('button', { name: 'Undo' }))
      await waitFor(() => expect(texts()).toEqual([undefined, 'C', 'D']))
    })

    it('applies size, Background and color to the selected text, one undo step each', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      await user.type(await place(api), 'Hi')
      api.escape()
      api.key(press('r'))
      await waitFor(() => expect(texts()).toEqual(['Hi']))
      expect(screen.queryByLabelText('Text size')).not.toBeInTheDocument()
      nodes()[0].fire('pointerdown')
      // Its options show, even with another tool.
      await waitFor(() =>
        expect(screen.getByLabelText('Medium text')).toHaveAttribute('data-state', 'on'),
      )
      const label = () => nodes()[0].children[1].attrs
      const box = () => nodes()[0].children[0].attrs
      await user.click(screen.getByLabelText('Large text'))
      // Measured again: 2 x 18 + 2 x 11 padding wide in the fake.
      await waitFor(() => expect(label().fontSize).toBe(36))
      expect(box()).toMatchObject({ width: 58, fill: 'transparent' })
      await user.click(screen.getByRole('button', { name: 'Background' }))
      await waitFor(() => expect(box().fill).toBe('#FF3B30'))
      expect(label().fontSize).toBe(36)
      await user.click(screen.getByRole('button', { name: 'Color and size' }))
      await user.click(await screen.findByLabelText('Color #FFCC00'))
      await waitFor(() => expect(box().fill).toBe('#FFCC00'))
      expect(label().fill).toBe('#000000')
      // The stroke size does not show on text: no undo step.
      await user.click(screen.getByLabelText('Thick'))
      api.escape()
      api.key(press('z', mod))
      await waitFor(() => expect(box().fill).toBe('#FF3B30'))
      api.key(press('z', mod))
      await waitFor(() => expect(box().fill).toBe('transparent'))
      expect(label().fontSize).toBe(36)
      api.key(press('z', mod))
      await waitFor(() => expect(label().fontSize).toBe(24))
      // The tool's own options did not change.
      expect(screen.getByTestId('current-color').style.background).toBe('rgb(255, 59, 48)')
      // A rect selected: the text options are the tool's again.
      await drag(300, 300, 350, 350)
      nodes()[1].fire('pointerdown')
      await waitFor(() => expect(screen.queryByLabelText('Text size')).not.toBeInTheDocument())
    })

    it('shows the selected text’s color, and keeps it in the selection when it grows', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      // Small text near the selection's bottom-right corner (500, 400).
      await user.click(screen.getByLabelText('Text'))
      await user.click(screen.getByLabelText('Small text'))
      await user.click(screen.getByRole('button', { name: 'Color and size' }))
      await user.click(await screen.findByLabelText('Color #007AFF'))
      api.escape()
      await user.type(await place(api, 470, 385), 'Hello')
      api.escape()
      await waitFor(() => expect(texts()).toEqual(['Hello']))
      // Back to red for the tool; the text stays blue.
      await user.click(screen.getByRole('button', { name: 'Color and size' }))
      await user.click(await screen.findByLabelText('Color #FF3B30'))
      api.escape()
      nodes()[0].fire('pointerdown')
      await waitFor(() =>
        expect(screen.getByTestId('current-color').style.background).toBe('rgb(0, 122, 255)'),
      )
      // Red, the tool's color, can be picked for it.
      await user.click(screen.getByRole('button', { name: 'Color and size' }))
      await user.click(await screen.findByLabelText('Color #FF3B30'))
      await waitFor(() => expect(nodes()[0].children[1].attrs.fill).toBe('#FF3B30'))
      api.escape()
      await user.click(screen.getByLabelText('Large text'))
      // 5 x 18 + 2 x 11 = 112 wide, 45 + 22 = 67 tall: moved into the corner at (500, 400).
      await waitFor(() => expect(nodes()[0].children[1].attrs.fontSize).toBe(36))
      const { x, y } = nodes()[0].attrs as { x: number; y: number }
      const { width, height } = nodes()[0].children[0].attrs as { width: number; height: number }
      expect([x, y, width, height]).toEqual([388, 333, 112, 67])
      expect(x).toBeGreaterThanOrEqual(sel.x)
      expect(y).toBeGreaterThanOrEqual(sel.y)
      expect(x + width).toBeLessThanOrEqual(sel.x + sel.width)
      expect(y + height).toBeLessThanOrEqual(sel.y + sel.height)
    })

    it('commits the text being typed before exporting', async () => {
      fakeContext()
      const user = userEvent.setup()
      const { api } = await setup()
      await user.type(await place(api), 'Hi')
      const out = await api.exportAnnotations({ x: 200, y: 200, width: 800, height: 600 }, scale)
      // The 38 x 44 box at (143, 128), plus a pixel, at 2x, less the crop.
      expect(out.layer).toMatchObject({ x: 84, y: 54, width: 80, height: 92 })
      expect(editor()).not.toBeInTheDocument()
    })

    it('drops the text being typed on reset', async () => {
      const user = userEvent.setup()
      const { api } = await setup()
      await user.type(await place(api), 'Hi')
      api.reset()
      await waitFor(() => expect(editor()).not.toBeInTheDocument())
      expect(kinds()).toEqual([])
      expect(api.annotated()).toBe(false)
    })
  })
})
