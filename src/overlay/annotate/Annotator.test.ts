import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { stages, type FakeNode } from '../../test/konva'
import type { Rect } from '../geometry'
import { fakeContext } from '../../test/canvas'
import Annotator from './Annotator.svelte'
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

const display = { width: 1000, height: 700 }
const scale = { x: 2, y: 2 }
const options = { rememberRegion: true, magnifier: false }
const sel: Rect = { x: 100, y: 100, width: 400, height: 300 }

type Api = {
  key: (e: KeyboardEvent) => boolean
  escape: () => boolean
  reset: () => void
  exportAnnotations: (crop: Rect, scale: number) => Promise<board.AnnotationExport>
}
type Props = {
  selection: Rect | null
  actions?: Record<string, (...args: never[]) => void>
  annotated?: boolean
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
const layer = () => stage().children[0]
const transformer = () =>
  layer().children.find((n) => n.className === 'Transformer') as FakeNode & {
    attached: FakeNode[]
  }
const nodes = () => layer().children.filter((n) => n.className !== 'Transformer')
const kinds = () => nodes().map((n) => n.className)
const element = () => screen.getByTestId('annotations')
const at = (x: number, y: number) => ({ clientX: x, clientY: y, button: 0, pointerId: 1 })
const press = (key: string, init: KeyboardEventInit = {}) =>
  new KeyboardEvent('keydown', { key, cancelable: true, ...init })

/** Drags on the annotation layer, in window px. */
async function drag(x1: number, y1: number, x2: number, y2: number) {
  await fireEvent.pointerDown(element(), at(x1, y1))
  await fireEvent.pointerMove(element(), at(x2, y2))
  await fireEvent.pointerUp(element(), at(x2, y2))
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

  it('covers the selection, and shrinks to nothing without one', async () => {
    const { view } = await setup()
    expect(element().style).toMatchObject({
      left: '100px',
      top: '100px',
      width: '400px',
      height: '300px',
    })
    expect(stage().attrs).toMatchObject({ width: 400, height: 300 })
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
    expect(layer().attrs).toMatchObject({ x: -120, y: -90 })
  })

  it('shows the shape while it is drawn, and drops it on cancel', async () => {
    const { api } = await setup()
    api.key(press('r'))
    await fireEvent.pointerDown(element(), at(150, 150))
    await fireEvent.pointerMove(element(), at(200, 220))
    expect(nodes()[0].attrs).toMatchObject({ width: 50, height: 70, listening: false })
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
    api.key(press('z', { metaKey: true }))
    await waitFor(() => expect(kinds()).toEqual(['Rect']))
    expect(nodes()[0].attrs).toMatchObject({ x: 170.5, y: 185 })
    api.key(press('z', { ctrlKey: true }))
    api.key(press('z', { ctrlKey: true }))
    api.key(press('z', { ctrlKey: true }))
    await waitFor(() => expect(nodes()[0].attrs).toMatchObject({ x: 150, y: 150 }))
    api.key(press('Z', { metaKey: true, shiftKey: true }))
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
    api.key(press('z', { metaKey: true }))
    await waitFor(() => expect(transformer().attached).toEqual([]))
    api.key(press('z', { metaKey: true, shiftKey: true }))
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
    expect(api.key(press('c', { metaKey: true }))).toBe(false)
    expect(api.key(press('p'))).toBe(false)
    expect(api.key(press('x'))).toBe(false)
    expect(api.key(press('ArrowLeft'))).toBe(false)
    input.remove()
  })

  it('leaves keys alone with no selection', () => {
    const view = render(Annotator, { props: props({ selection: null }) })
    const api = view.component as unknown as Api
    expect(api.key(press('r'))).toBe(false)
  })

  it('says whether there are annotations, and resets', async () => {
    let annotated = false
    const view = render(Annotator, {
      props: {
        ...props({ selection: sel }),
        get annotated() {
          return annotated
        },
        set annotated(v: boolean) {
          annotated = v
        },
      },
    })
    await waitFor(() => expect(stages).toHaveLength(1))
    const api = view.component as unknown as Api
    api.key(press('r'))
    await drag(150, 150, 250, 250)
    await waitFor(() => expect(annotated).toBe(true))
    api.reset()
    await waitFor(() => expect(annotated).toBe(false))
    expect(kinds()).toEqual([])
    // The tool went too.
    await drag(150, 150, 250, 250)
    expect(kinds()).toEqual([])
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
    const out = await api.exportAnnotations({ x: 200, y: 200, width: 800, height: 600 }, 2)
    expect(out).toMatchObject({ x: 92, y: 92, width: 216, height: 216, blurs: [] })
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
    await user.click(screen.getByRole('button', { name: 'Share' }))
    await user.click(screen.getByRole('button', { name: 'More' }))
    await user.click((await screen.findByText('Magnifier')).closest('[role=menuitemcheckbox]')!)
    expect(option).toHaveBeenCalledWith('magnifier', true)
  })

  it('shows the tools as unavailable when Konva cannot load, and retries later', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    konva.failures = 1
    const view = render(Annotator, { props: props({ selection: sel }) })
    await toolbar()
    await waitFor(() => expect(screen.getByLabelText('Rectangle')).toBeDisabled())
    const api = view.component as unknown as Api
    expect(api.key(press('r'))).toBe(false)
    // The next selection change tries again.
    await view.rerender(props({ selection: { ...sel, x: 110 } }))
    await waitFor(() => expect(stages).toHaveLength(1))
    await waitFor(() => expect(screen.getByLabelText('Rectangle')).toBeEnabled())
  })

  it('destroys the stage when it goes away', async () => {
    const { view } = await setup()
    view.unmount()
    expect(stage().destroyed).toBe(true)
  })
})
