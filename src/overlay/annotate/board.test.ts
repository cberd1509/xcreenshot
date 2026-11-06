import type Konva from 'konva'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { FakeKonva, stages, type FakeNode } from '../../test/konva'
import { Board, attrs, exportShapes, loadKonva } from './board'
import { DEFAULT_STYLE, type Shape } from './shapes'

const style = DEFAULT_STYLE
const rect: Shape = { id: 'r', kind: 'rect', style, x: 10, y: 20, width: 30, height: 40 }
const arrow: Shape = { id: 'a', kind: 'arrow', style, points: [10, 20, 40, 60] }
const blur: Shape = { id: 'b', kind: 'blur', style, x: 1, y: 2, width: 3, height: 4 }

function setup() {
  const onchange = vi.fn<(s: Shape) => void>()
  const onselect = vi.fn<(id: string) => void>()
  const board = new Board(
    FakeKonva as unknown as typeof Konva,
    document.createElement('div'),
    { width: 1000, height: 700 },
    { onchange, onselect },
  )
  const stage = stages.at(-1)!
  const layer = stage.children[0]
  const [group, transformer] = layer.children as [FakeNode, FakeNode & { attached: FakeNode[] }]
  return { board, stage, layer, group, transformer, onchange, onselect }
}

describe('attrs', () => {
  it('draws an arrow from its start, with a head sized to the stroke', () => {
    expect(attrs(arrow)).toMatchObject({
      x: 10,
      y: 20,
      points: [0, 0, 30, 40],
      stroke: style.color,
      strokeWidth: style.size,
      pointerLength: style.size * 3,
    })
  })

  it('draws a rect outline and a blur placeholder', () => {
    expect(attrs(rect)).toMatchObject({ x: 10, y: 20, width: 30, height: 40, stroke: style.color })
    expect(attrs(blur)).toMatchObject({ x: 1, y: 2, width: 3, height: 4, fill: expect.any(String) })
  })
})

describe('Board', () => {
  beforeEach(() => {
    stages.length = 0
  })

  it('loads Konva once', async () => {
    const first = loadKonva()
    expect(loadKonva()).toBe(first)
    expect(await first).toBe(FakeKonva)
  })

  it('builds a stage the size of the display', () => {
    const { board, stage } = setup()
    expect(stage.attrs).toMatchObject({ width: 1000, height: 700 })
    board.resize({ width: 800, height: 600 })
    expect(stage.attrs).toMatchObject({ width: 800, height: 600 })
  })

  it('puts the shapes at the selection, clipped to it', () => {
    const { board, group } = setup()
    board.place({ x: 100, y: 50, width: 300, height: 200 })
    expect(group.attrs).toEqual({
      x: 100,
      y: 50,
      clip: { x: 0, y: 0, width: 300, height: 200 },
    })
  })

  it('adds, updates and removes nodes to match the shapes', () => {
    const { board, group, layer } = setup()
    board.render([rect, arrow], null, null)
    expect(group.children.map((n) => n.className)).toEqual(['Rect', 'Arrow'])
    const [node] = group.children
    expect(node.attrs).toMatchObject({ x: 10, draggable: true })

    board.render([{ ...rect, x: 99 }, arrow], null, null)
    expect(group.children[0]).toBe(node)
    expect(node.attrs).toMatchObject({ x: 99 })

    board.render([arrow], null, null)
    expect(node.destroyed).toBe(true)
    expect(group.children.map((n) => n.className)).toEqual(['Arrow'])
    expect((layer as unknown as { draws: number }).draws).toBe(3)
  })

  it('leaves a node alone when its shape did not change', () => {
    const { board, group } = setup()
    board.render([rect], null, null)
    const node = group.children[0]
    const set = vi.spyOn(node, 'setAttrs')
    board.render([rect], null, null)
    expect(set).not.toHaveBeenCalled()
  })

  it('shows the shape being drawn, which ignores the pointer', () => {
    const { board, group } = setup()
    board.render([], arrow, null)
    const draft = group.children[0]
    expect(draft.attrs).toMatchObject({ listening: false })
    board.render([], null, null)
    expect(draft.destroyed).toBe(true)
    expect(group.children).toEqual([])
  })

  it('attaches the resize handles to the selected shape', () => {
    const { board, group, transformer } = setup()
    board.render([rect, arrow], null, 'a')
    expect(transformer.attached).toEqual([group.children[1]])
    board.render([rect, arrow], null, 'gone')
    expect(transformer.attached).toEqual([])
    board.render([rect, arrow], null, null)
    expect(transformer.attached).toEqual([])
  })

  it('reports a click on a shape', () => {
    const { board, group, onselect } = setup()
    board.render([rect], null, null)
    group.children[0].fire('pointerdown')
    expect(onselect).toHaveBeenCalledWith('r')
  })

  it('reports a moved or resized shape and resets its scale', () => {
    const { board, group, onchange } = setup()
    board.render([rect], null, null)
    const node = group.children[0]
    node.setAttrs({ x: 15, y: 25 })
    node.fire('dragend')
    expect(onchange).toHaveBeenLastCalledWith({ ...rect, x: 15, y: 25 })

    node.setAttrs({ scaleX: 2, scaleY: 3 })
    node.fire('transformend')
    expect(onchange).toHaveBeenLastCalledWith({ ...rect, x: 15, y: 25, width: 60, height: 120 })
    expect(node.attrs).toMatchObject({ scaleX: 1, scaleY: 1 })
  })

  it('ignores events from a node that was removed', () => {
    const { board, group, onchange } = setup()
    board.render([rect], null, null)
    const node = group.children[0]
    board.render([], null, null)
    node.fire('dragend')
    expect(onchange).not.toHaveBeenCalled()
  })

  it('hit-tests the stage', () => {
    const { board, stage } = setup()
    expect(board.hits({ x: 1, y: 2 })).toBe(false)
    ;(stage as unknown as { hit: FakeNode }).hit = stage
    expect(board.hits({ x: 1, y: 2 })).toBe(true)
  })

  it('destroys its stage', () => {
    const { board, stage } = setup()
    board.destroy()
    expect(stage.destroyed).toBe(true)
  })
})

describe('exportShapes', () => {
  beforeEach(() => {
    stages.length = 0
  })

  const crop = { x: 200, y: 100, width: 600, height: 400 }

  it('renders the shapes alone at the crop’s physical size', async () => {
    const out = await exportShapes([rect, arrow, blur], crop, 2)
    expect(out.width).toBe(600)
    expect(out.height).toBe(400)
    expect(out.rgba).toHaveLength(600 * 400 * 4)
    expect(out.blurs).toEqual([{ x: 2, y: 4, width: 6, height: 8 }])
    const stage = stages.at(-1)!
    // Logical size, drawn at 2 px per logical px.
    expect(stage.attrs).toMatchObject({ width: 300, height: 200 })
    const layer = stage.children[0]
    expect(layer.children.map((n) => n.className)).toEqual(['Rect', 'Arrow'])
    expect(layer.attrs.exported).toEqual({ width: 300, height: 200, x: 0, y: 0, pixelRatio: 2 })
    expect(stage.destroyed).toBe(true)
  })

  it('draws nothing when there are only blurs, or no shapes', async () => {
    expect(await exportShapes([blur], crop, 1)).toEqual({
      rgba: null,
      width: 600,
      height: 400,
      blurs: [{ x: 1, y: 2, width: 3, height: 4 }],
    })
    expect((await exportShapes([], crop, 1)).rgba).toBeNull()
    expect(stages).toEqual([])
  })

  it('draws nothing for an empty crop', async () => {
    expect((await exportShapes([rect], { ...crop, width: 0 }, 1)).rgba).toBeNull()
  })
})
