import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { settings } from '$lib/settings.svelte'
import { stages, type FakeNode } from '../../test/konva'
import type { Rect } from '../geometry'
import Annotator from './Annotator.svelte'

const display = { width: 1000, height: 700 }
const badge = { width: 80, height: 24 }
const sel: Rect = { x: 100, y: 100, width: 400, height: 300 }

type Props = { selection: Rect | null; actions?: Record<string, () => void> }

async function setup(props: Props = { selection: sel }) {
  const view = render(Annotator, { props: { display, badge, ...props } })
  await waitFor(() => expect(stages).toHaveLength(1))
  return view
}

const stage = () => stages[0] as FakeNode & { hit: FakeNode | null }
const group = () => stage().children[0].children[0]
const transformer = () => stage().children[0].children[1] as FakeNode & { attached: FakeNode[] }
const kinds = () => group().children.map((n) => n.className)
const layer = () => screen.getByTestId('annotations')
const at = (x: number, y: number) => ({ clientX: x, clientY: y, button: 0, pointerId: 1 })
const key = (k: string, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(window, { key: k, ...init })

/** Drags on the annotation layer, in window px. */
async function drag(x1: number, y1: number, x2: number, y2: number) {
  await fireEvent.pointerDown(layer(), at(x1, y1))
  await fireEvent.pointerMove(layer(), at(x2, y2))
  await fireEvent.pointerUp(layer(), at(x2, y2))
}

describe('Annotator', () => {
  let passed: ReturnType<typeof vi.fn<(e: Event) => void>>

  beforeEach(() => {
    stages.length = 0
    passed = vi.fn<(e: Event) => void>()
    document.addEventListener('pointerdown', passed)
    settings.magnifier = false
  })

  afterEach(() => {
    document.removeEventListener('pointerdown', passed)
  })

  it('loads nothing until there is a selection', async () => {
    const view = render(Annotator, { props: { display, badge, selection: null } })
    await new Promise((r) => setTimeout(r, 0))
    expect(stages).toHaveLength(0)
    expect(screen.queryByRole('toolbar')).not.toBeInTheDocument()
    expect(layer().style.pointerEvents).toBe('none')

    await view.rerender({ selection: sel })
    await waitFor(() => expect(stages).toHaveLength(1))
    expect(stage().attrs).toMatchObject(display)
    expect(screen.getByRole('toolbar')).toBeInTheDocument()
  })

  it('puts the shapes on the selection and follows it', async () => {
    const view = await setup()
    expect(group().attrs).toMatchObject({ x: 100, y: 100 })
    await view.rerender({ selection: { ...sel, x: 300, width: 100 } })
    expect(group().attrs).toMatchObject({ x: 300, y: 100, clip: { width: 100, height: 300 } })
  })

  it('clips the layer to the selection, leaving its edge for the handles', async () => {
    await setup()
    expect(layer().style.clipPath).toBe('inset(104px 504px 304px 104px)')
  })

  it('puts the toolbar below the selection', async () => {
    await setup()
    const bar = screen.getByRole('toolbar').closest<HTMLElement>('[style*=left]')!
    // jsdom lays nothing out, so the toolbar has no size: right-aligned is the right edge.
    expect(bar.style.left).toBe('500px')
    expect(bar.style.top).toBe('406px')
  })

  it('draws a rect with R and an arrow with A', async () => {
    await setup()
    key('r')
    await drag(150, 150, 250, 200)
    expect(kinds()).toEqual(['Rect'])
    // In the selection's own space.
    expect(group().children[0].attrs).toMatchObject({ x: 50, y: 50, width: 100, height: 50 })
    key('A')
    await drag(150, 150, 50, 50)
    expect(kinds()).toEqual(['Rect', 'Arrow'])
    // Clamped to the selection.
    expect(group().children[1].attrs).toMatchObject({ x: 50, y: 50, points: [0, 0, -50, -50] })
    expect(passed).not.toHaveBeenCalled()
  })

  it('shows the shape while it is drawn', async () => {
    await setup()
    key('r')
    await fireEvent.pointerDown(layer(), at(150, 150))
    await fireEvent.pointerMove(layer(), at(200, 220))
    expect(group().children[0].attrs).toMatchObject({ width: 50, height: 70, listening: false })
    await fireEvent.pointerUp(layer(), at(150, 150))
    // Back where it started: a click, so nothing is drawn.
    expect(kinds()).toEqual([])
  })

  it('lets a press with no tool through to the overlay, to move the selection', async () => {
    await setup()
    await drag(150, 150, 250, 250)
    expect(kinds()).toEqual([])
    expect(passed).toHaveBeenCalledOnce()
  })

  it('ignores presses before Konva loads and other buttons', async () => {
    const view = render(Annotator, { props: { display, badge, selection: sel } })
    await fireEvent.pointerDown(layer(), at(150, 150))
    await waitFor(() => expect(stages).toHaveLength(1))
    key('r')
    await fireEvent.pointerDown(layer(), { ...at(150, 150), button: 2 })
    expect(passed).toHaveBeenCalledTimes(2)
    view.unmount()
  })

  it('selects, moves and deletes a shape, with undo', async () => {
    await setup()
    key('r')
    await drag(150, 150, 250, 250)
    const node = group().children[0]
    // A press on the shape is Konva's: it selects the shape and the overlay never sees it.
    stage().hit = node
    node.fire('pointerdown')
    await fireEvent.pointerDown(layer(), at(160, 160))
    expect(passed).not.toHaveBeenCalled()
    await waitFor(() => expect(transformer().attached).toEqual([node]))

    node.setAttrs({ x: 70, y: 80 })
    node.fire('dragend')
    await waitFor(() => expect(node.attrs).toMatchObject({ x: 70, y: 80 }))

    key('Backspace')
    await waitFor(() => expect(kinds()).toEqual([]))
    key('z', { metaKey: true })
    await waitFor(() => expect(kinds()).toEqual(['Rect']))
    expect(group().children[0].attrs).toMatchObject({ x: 70, y: 80 })
    key('z', { ctrlKey: true })
    await waitFor(() => expect(group().children[0].attrs).toMatchObject({ x: 50, y: 50 }))
    key('Z', { metaKey: true, shiftKey: true })
    await waitFor(() => expect(group().children[0].attrs).toMatchObject({ x: 70, y: 80 }))
  })

  it('drops the shape selection on a press elsewhere', async () => {
    await setup()
    key('r')
    await drag(150, 150, 250, 250)
    group().children[0].fire('pointerdown')
    await waitFor(() => expect(transformer().attached).toHaveLength(1))
    await fireEvent.pointerDown(layer(), at(400, 300))
    await fireEvent.pointerUp(layer(), at(400, 300))
    await waitFor(() => expect(transformer().attached).toEqual([]))
    // Nothing selected: Delete does nothing.
    key('Delete')
    expect(kinds()).toEqual(['Rect'])
  })

  it('clears all from the toolbar, and undoes it', async () => {
    const user = userEvent.setup()
    await setup()
    key('r')
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
    await user.click(screen.getByRole('button', { name: 'Arrow' }))
    await drag(150, 150, 250, 250)
    expect(kinds()).toEqual(['Arrow'])
  })

  it('ignores shortcuts while typing, with modifiers, or for tools not ready', async () => {
    await setup()
    const input = document.createElement('input')
    document.body.append(input)
    fireEvent.keyDown(input, { key: 'r' })
    key('r', { altKey: true })
    key('p')
    key('x')
    await drag(150, 150, 250, 250)
    expect(kinds()).toEqual([])
    input.remove()
  })

  it('ignores keys with no selection', async () => {
    render(Annotator, { props: { display, badge, selection: null } })
    key('z', { metaKey: true })
    key('r')
    expect(stages).toHaveLength(0)
  })

  it('starts over when the selection goes away', async () => {
    const view = await setup()
    key('r')
    await drag(150, 150, 250, 250)
    await view.rerender({ selection: null })
    expect(screen.queryByRole('toolbar')).not.toBeInTheDocument()
    await view.rerender({ selection: sel })
    expect(kinds()).toEqual([])
    // The tool is dropped too.
    await drag(150, 150, 250, 250)
    expect(kinds()).toEqual([])
    // The stage is kept.
    expect(stages).toHaveLength(1)
  })

  it('exports the annotations at the crop size', async () => {
    const view = await setup()
    key('r')
    await drag(150, 150, 250, 250)
    const out = await view.component.exportAnnotations(
      { x: 200, y: 200, width: 800, height: 600 },
      2,
    )
    expect(out).toMatchObject({ width: 800, height: 600, blurs: [] })
    expect(out.rgba).toHaveLength(800 * 600 * 4)
  })

  it('runs the given actions, and keeps menu options for this run', async () => {
    const user = userEvent.setup()
    const copy = vi.fn<() => void>()
    await setup({ selection: sel, actions: { copy } })
    await user.click(screen.getByRole('button', { name: 'Copy' }))
    expect(copy).toHaveBeenCalledOnce()
    // The defaults do nothing yet.
    await user.click(screen.getByRole('button', { name: 'Save' }))
    await user.click(screen.getByRole('button', { name: 'Share' }))
    await user.click(screen.getByRole('button', { name: 'More' }))
    await user.click((await screen.findByText('Magnifier')).closest('[role=menuitemcheckbox]')!)
    expect(settings.magnifier).toBe(true)
  })

  it('destroys the stage when it goes away', async () => {
    const view = await setup()
    view.unmount()
    expect(stage().destroyed).toBe(true)
  })
})
