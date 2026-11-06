import type Konva from 'konva'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fakeContext } from '../../test/canvas'
import { FakeKonva, stages, type FakeNode } from '../../test/konva'
import { Board, attrs, exportShapes, loadKonva, once } from './board'
import { DEFAULT_STYLE, type Shape } from './shapes'

const style = DEFAULT_STYLE
const rect: Shape = { id: 'r', kind: 'rect', style, x: 110, y: 120, width: 30, height: 40 }
const arrow: Shape = { id: 'a', kind: 'arrow', style, points: [110, 120, 140, 160] }
const blur: Shape = { id: 'b', kind: 'blur', style, x: 101, y: 102, width: 3, height: 4 }

function setup() {
  const onchange = vi.fn<(s: Shape) => void>()
  const onselect = vi.fn<(id: string) => void>()
  const board = new Board(FakeKonva as unknown as typeof Konva, document.createElement('div'), {
    onchange,
    onselect,
  })
  const stage = stages.at(-1)! as FakeNode & { hit: FakeNode | null; asked: unknown }
  const layer = stage.children[0]
  const transformer = layer.children[0] as FakeNode & { attached: FakeNode[] }
  const shapes = () => layer.children.filter((n) => n !== transformer)
  return { board, stage, layer, transformer, shapes, onchange, onselect }
}

describe('attrs', () => {
  it('draws an arrow from its start, with a head sized to the stroke', () => {
    expect(attrs(arrow)).toMatchObject({
      x: 110,
      y: 120,
      points: [0, 0, 30, 40],
      stroke: style.color,
      strokeWidth: style.size,
      pointerLength: style.size * 3,
    })
  })

  it('draws a rect outline and a blur placeholder', () => {
    expect(attrs(rect)).toMatchObject({
      x: 110,
      y: 120,
      width: 30,
      height: 40,
      stroke: style.color,
    })
    expect(attrs(blur)).toMatchObject({ x: 101, width: 3, fill: expect.any(String) })
  })
})

describe('loadKonva', () => {
  it('loads Konva once', async () => {
    const first = loadKonva()
    expect(loadKonva()).toBe(first)
    expect(await first).toBe(FakeKonva)
  })

  it('tries again after a failed load', async () => {
    const load = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValue('ok')
    const get = once(load)
    await expect(get()).rejects.toThrow('offline')
    expect(await get()).toBe('ok')
    expect(await get()).toBe('ok')
    expect(load).toHaveBeenCalledTimes(2)
  })
})

describe('Board', () => {
  beforeEach(() => {
    stages.length = 0
  })

  it('starts empty and covers the selection, shapes offset to keep display coordinates', () => {
    const { board, stage, layer } = setup()
    expect(stage.attrs).toMatchObject({ width: 0, height: 0 })
    board.place({ x: 100, y: 50, width: 300, height: 200 })
    expect(stage.attrs).toMatchObject({ width: 300, height: 200 })
    expect(layer.attrs).toMatchObject({ x: -100, y: -50 })
    board.place(null)
    expect(stage.attrs).toMatchObject({ width: 0, height: 0 })
  })

  it('adds, updates and removes nodes to match the shapes', () => {
    const { board, shapes, layer } = setup()
    board.render([rect, arrow], null)
    expect(shapes().map((n) => n.className)).toEqual(['Rect', 'Arrow'])
    const [node] = shapes()
    expect(node.attrs).toMatchObject({ x: 110, draggable: true })

    board.render([{ ...rect, x: 99 }, arrow], null)
    expect(shapes()[0]).toBe(node)
    expect(node.attrs).toMatchObject({ x: 99 })

    board.render([arrow], null)
    expect(node.destroyed).toBe(true)
    expect(shapes().map((n) => n.className)).toEqual(['Arrow'])
    expect((layer as unknown as { draws: number }).draws).toBe(3)
  })

  it('leaves a node alone when its shape did not change', () => {
    const { board, shapes } = setup()
    board.render([rect], null)
    const set = vi.spyOn(shapes()[0], 'setAttrs')
    board.render([rect], null)
    expect(set).not.toHaveBeenCalled()
  })

  it('reuses one node for the shape being drawn', () => {
    const { board, shapes } = setup()
    board.preview(rect)
    const [draft] = shapes()
    expect(draft.attrs).toMatchObject({ listening: false, width: 30 })
    board.preview({ ...rect, width: 60 })
    expect(shapes()).toEqual([draft])
    expect(draft.attrs).toMatchObject({ width: 60 })
    // Another kind needs another node.
    board.preview(arrow)
    expect(draft.destroyed).toBe(true)
    expect(shapes().map((n) => n.className)).toEqual(['Arrow'])
    board.preview(null)
    expect(shapes()).toEqual([])
    board.preview(null)
  })

  it('attaches the resize handles to the selected shape, on top', () => {
    const { board, shapes, transformer, layer } = setup()
    board.render([rect, arrow], 'a')
    expect(transformer.attached).toEqual([shapes()[1]])
    expect(layer.children.at(-1)).toBe(transformer)
    board.render([rect, arrow], 'gone')
    expect(transformer.attached).toEqual([])
    board.render([rect, arrow], null)
    expect(transformer.attached).toEqual([])
  })

  it('reports a press on a shape', () => {
    const { board, shapes, onselect } = setup()
    board.render([rect], null)
    shapes()[0].fire('pointerdown')
    expect(onselect).toHaveBeenCalledWith('r')
  })

  it('reports a moved or resized shape and puts the node back to its model', () => {
    const { board, shapes, onchange } = setup()
    board.render([rect], null)
    const node = shapes()[0]
    node.setAttrs({ x: 115, y: 125 })
    node.fire('dragend')
    expect(onchange).toHaveBeenLastCalledWith({ ...rect, x: 115, y: 125 })
    expect(node.attrs).toMatchObject({ x: 110, y: 120 })

    node.setAttrs({ scaleX: 2, scaleY: 3 })
    node.fire('transformend')
    expect(onchange).toHaveBeenLastCalledWith({ ...rect, width: 60, height: 120 })
    expect(node.attrs).toMatchObject({ scaleX: 1, scaleY: 1 })
  })

  it('ignores events from a node that was removed', () => {
    const { board, shapes, onchange } = setup()
    board.render([rect], null)
    const node = shapes()[0]
    board.render([], null)
    node.fire('dragend')
    expect(onchange).not.toHaveBeenCalled()
  })

  it('hit-tests in stage px', () => {
    const { board, stage } = setup()
    board.place({ x: 100, y: 50, width: 300, height: 200 })
    expect(board.hits({ x: 110, y: 60 })).toBe(false)
    expect(stage.asked).toEqual({ x: 10, y: 10 })
    stage.hit = stage
    expect(board.hits({ x: 110, y: 60 })).toBe(true)
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

  // The selection (100, 100) to (400, 300) at 2x.
  const crop = { x: 200, y: 200, width: 600, height: 400 }

  it('renders only the box the shapes paint, at native size', async () => {
    const context = fakeContext()
    const out = await exportShapes([rect, blur], crop, 2)
    // The rect's box with its stroke, (106, 116) to (144, 164), in px of the crop.
    expect(out).toMatchObject({ x: 12, y: 32, width: 76, height: 96 })
    expect(out.blurs).toEqual([{ x: 2, y: 4, width: 6, height: 8 }])
    expect(context.getImageData).toHaveBeenCalledWith(0, 0, 76, 96)
    expect(out.rgba).toEqual(new Uint8ClampedArray([7, 7, 7, 255]))
    const stage = stages.at(-1)!
    const layer = stage.children[0]
    expect(layer.children.map((n) => n.className)).toEqual(['Rect'])
    expect(layer.attrs.exported).toEqual({ x: 106, y: 116, width: 38, height: 48, pixelRatio: 2 })
    expect(stage.destroyed).toBe(true)
  })

  it('clips the box to the crop', async () => {
    fakeContext()
    const out = await exportShapes([{ ...rect, x: 90 }], crop, 2)
    expect(out).toMatchObject({ x: 0, width: 48 })
  })

  it('draws nothing when there are only blurs, no shapes, or none in the crop', async () => {
    expect(await exportShapes([blur], crop, 2)).toEqual({
      rgba: null,
      x: 0,
      y: 0,
      width: 0,
      height: 0,
      blurs: [{ x: 2, y: 4, width: 6, height: 8 }],
    })
    expect((await exportShapes([], crop, 2)).rgba).toBeNull()
    expect((await exportShapes([{ ...rect, x: 900 }], crop, 2)).rgba).toBeNull()
    expect(stages).toEqual([])
  })
})
