import type Konva from 'konva'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fakeContext } from '../../test/canvas'
import { FakeKonva, stages, type FakeNode } from '../../test/konva'
import type { Rect } from '../geometry'
import { Board, attrs, exportShapes, loadKonva, once, shrink, textAttrs } from './board'
import { DEFAULT_STYLE, HIGHLIGHTER_WIDTH, type Shape } from './shapes'

const style = DEFAULT_STYLE
const rect: Shape = { id: 'r', kind: 'rect', style, x: 110, y: 120, width: 30, height: 40 }
const arrow: Shape = { id: 'a', kind: 'arrow', style, points: [110, 120, 140, 160] }
const blur: Shape = { id: 'b', kind: 'blur', style, x: 101, y: 102, width: 3, height: 4 }
const text = {
  id: 't',
  kind: 'text',
  style,
  x: 110,
  y: 120,
  width: 50,
  height: 30,
  text: 'Hi',
  fontSize: 20,
  background: false,
} satisfies Shape

/** A frame canvas of `width × height` px. */
function canvas(width: number, height: number) {
  const c = document.createElement('canvas')
  c.width = width
  c.height = height
  return c
}

const blocksOf = (n: FakeNode) => n.attrs.blocks as (Rect & { color: string })[]

function setup(source?: HTMLCanvasElement | null) {
  const onchange = vi.fn<(s: Shape) => void>()
  const onselect = vi.fn<(id: string) => void>()
  const onedit = vi.fn<(id: string) => void>()
  const board = new Board(
    FakeKonva as unknown as typeof Konva,
    document.createElement('div'),
    { onchange, onselect, onedit },
    source === undefined ? undefined : () => source,
  )
  const stage = stages.at(-1)! as FakeNode & { hit: FakeNode | null; asked: unknown }
  const [layer, top] = stage.children as [
    FakeNode & { draws: number },
    FakeNode & { draws: number },
  ]
  const transformer = top.children[0] as FakeNode & { attached: FakeNode[] }
  const shapes = () => layer.children
  const drafts = () => top.children.filter((n) => n !== transformer)
  return { board, stage, layer, top, transformer, shapes, drafts, onchange, onselect, onedit }
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

describe('text attrs', () => {
  it('puts the group at the corner, with a box and the text in the system font', () => {
    expect(attrs(text)).toEqual({ x: 110, y: 120, scaleX: 1, scaleY: 1 })
    const { box, label } = textAttrs(text)
    expect(box).toEqual({ width: 50, height: 30, fill: 'transparent', cornerRadius: 5 })
    expect(label).toMatchObject({ text: 'Hi', fontSize: 20, padding: 6, fill: style.color })
    expect(label.fontFamily).toBe('system-ui, -apple-system, "Segoe UI", sans-serif')
  })

  it('fills the box with the color and contrasts the text on a background', () => {
    const { box, label } = textAttrs({ ...text, background: true })
    expect(box.fill).toBe(style.color)
    // On red, black has the higher contrast ratio.
    expect(label.fill).toBe('#000000')
    expect(
      textAttrs({ ...text, style: { ...style, color: '#000000' }, background: true }).label.fill,
    ).toBe('#FFFFFF')
  })
})

describe('attrs for the other tools', () => {
  const line: Shape = { id: 'l', kind: 'line', style, points: [10, 20, 40, 60] }

  it('draws a line and a pen stroke from their first point', () => {
    expect(attrs(line)).toMatchObject({ x: 10, y: 20, points: [0, 0, 30, 40], lineCap: 'round' })
    const pen: Shape = { ...line, kind: 'pen', points: [10, 20, 15, 22, 30, 10] }
    expect(attrs(pen)).toMatchObject({ points: [0, 0, 5, 2, 20, -10], strokeWidth: style.size })
    expect(attrs(pen).opacity).toBeUndefined()
  })

  it('draws a highlighter wide, see-through and with flat ends', () => {
    const hl: Shape = { ...line, kind: 'highlighter', points: [10, 20, 40, 20] }
    expect(attrs(hl)).toMatchObject({
      strokeWidth: style.size * HIGHLIGHTER_WIDTH,
      lineCap: 'butt',
      opacity: 0.4,
    })
  })

  it('draws an ellipse in its box, its node at the corner', () => {
    const e: Shape = { ...rect, kind: 'ellipse' }
    expect(attrs(e)).toMatchObject({
      x: 110,
      y: 120,
      radiusX: 15,
      radiusY: 20,
      offsetX: -15,
      offsetY: -20,
    })
  })
})

describe('shrink', () => {
  it('shrinks the frame by a block on each side, halving step by step', () => {
    const context = fakeContext()
    const small = shrink(canvas(3024, 1964))!
    expect(small).toMatchObject({ width: 189, height: 123 })
    expect(small.factor).toEqual({ x: 3024 / 189, y: 1964 / 123 })
    expect(small.data).toHaveLength(189 * 123 * 4)
    // 16 is four halvings.
    expect(context.drawImage).toHaveBeenCalledTimes(4)
    expect(context.drawImage).toHaveBeenLastCalledWith(expect.anything(), 0, 0, 189, 123)
  })

  it('gives nothing without a frame or a 2D context', () => {
    fakeContext()
    expect(shrink(canvas(0, 0))).toBeNull()
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
    expect(shrink(canvas(64, 64))).toBeNull()
  })
})

describe('drawing a blur', () => {
  it('fills its blocks, then outlines the box', () => {
    const calls: unknown[][] = []
    const spy =
      (name: string) =>
      (...args: unknown[]) =>
        void calls.push([name, ...args])
    const context = Object.fromEntries(
      ['setAttr', 'fillRect', 'beginPath', 'rect', 'closePath', 'fillStrokeShape'].map((n) => [
        n,
        spy(n),
      ]),
    )
    const shape = {
      getAttr: () => [{ x: 0, y: 0, width: 8, height: 8, color: 'rgb(1, 2, 3)' }],
      width: () => 10,
      height: () => 5,
    }
    const a = attrs(blur) as {
      sceneFunc: (c: unknown, s: unknown) => void
      hitFunc: (c: unknown, s: unknown) => void
    }
    a.sceneFunc(context, shape)
    expect(calls.map((c) => c[0])).toEqual([
      'setAttr',
      'fillRect',
      'beginPath',
      'rect',
      'closePath',
      'fillStrokeShape',
    ])
    expect(calls[0]).toEqual(['setAttr', 'fillStyle', 'rgb(1, 2, 3)'])
    expect(calls[3]).toEqual(['rect', 0, 0, 10, 5])
    calls.length = 0
    a.hitFunc(context, shape)
    expect(calls.map((c) => c[0])).toEqual(['beginPath', 'rect', 'closePath', 'fillStrokeShape'])
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

  it('covers the display while there is a selection, and clips the shapes to it', () => {
    const { board, stage, layer, top } = setup()
    const display = { width: 1000, height: 700 }
    expect(stage.attrs).toMatchObject({ width: 0, height: 0 })
    const sel = { x: 100, y: 50, width: 300, height: 200 }
    board.place(sel, display)
    expect(stage.attrs).toMatchObject(display)
    expect(layer.attrs.clip).toEqual(sel)
    // The handles are not clipped.
    expect(top.attrs.clip).toBeUndefined()
    board.place(null, display)
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
    expect(layer.draws).toBe(3)
  })

  it('stacks the nodes in model order, so undo restores what was on top', () => {
    const { board, shapes } = setup()
    const other: Shape = { ...rect, id: 'r2', x: 120 }
    board.render([rect, arrow, other], null)
    // Delete the middle one, then undo: it comes back between the others.
    board.render([rect, other], null)
    board.render([rect, arrow, other], null)
    expect(shapes().map((n) => n.className)).toEqual(['Rect', 'Arrow', 'Rect'])
    expect(shapes()[2].attrs.x).toBe(120)
  })

  it('leaves a node alone when its shape did not change', () => {
    const { board, shapes } = setup()
    board.render([rect], null)
    const set = vi.spyOn(shapes()[0], 'setAttrs')
    board.render([rect], null)
    expect(set).not.toHaveBeenCalled()
  })

  it('reuses one node on the top layer for the shape being drawn', () => {
    const { board, drafts, layer, top } = setup()
    board.preview(rect)
    const [draft] = drafts()
    expect(draft.attrs).toMatchObject({ listening: false, width: 30 })
    board.preview({ ...rect, width: 60 })
    expect(drafts()).toEqual([draft])
    expect(draft.attrs).toMatchObject({ width: 60 })
    // Another kind needs another node.
    board.preview(arrow)
    expect(draft.destroyed).toBe(true)
    expect(drafts().map((n) => n.className)).toEqual(['Arrow'])
    board.preview(null)
    expect(drafts()).toEqual([])
    board.preview(null)
    // Only the top layer redraws while drawing.
    expect(layer.draws).toBe(0)
    expect(top.draws).toBe(5)
  })

  it('makes a node of the right kind for each shape', () => {
    const { board, shapes } = setup()
    const line: Shape = { id: 'l', kind: 'line', style, points: [1, 2, 30, 40] }
    board.render(
      [
        line,
        { ...line, id: 'p', kind: 'pen' },
        { ...line, id: 'h', kind: 'highlighter' },
        { ...rect, id: 'e', kind: 'ellipse' },
      ],
      null,
    )
    expect(shapes().map((n) => n.className)).toEqual(['Line', 'Line', 'Line', 'Ellipse'])
  })

  describe('blur', () => {
    // 2x: the selection is (200, 200) to (1000, 800) on a 2000 × 1400 frame.
    const display = { width: 1000, height: 700 }
    const sel = { x: 100, y: 100, width: 400, height: 300 }
    const wide: Shape = { ...blur, x: 150, y: 150, width: 100, height: 60 }

    function placed() {
      const context = fakeContext([10, 20, 30, 255])
      const s = setup(canvas(2000, 1400))
      s.board.place(sel, display)
      return { ...s, context }
    }

    it('shows the blocks Rust will paint, from the blur’s corner, under the other shapes', () => {
      const { board, shapes } = placed()
      board.render([rect, wide], null)
      expect(shapes().map((n) => n.className)).toEqual(['Shape', 'Rect'])
      const node = shapes()[0]
      expect(node.attrs).toMatchObject({
        x: 150,
        y: 150,
        width: 100,
        height: 60,
        fill: 'transparent',
      })
      // 200 × 120 frame px: 16 px blocks, 12 across and 7 down, the last row 24 px tall.
      const b = blocksOf(node)
      expect(b).toHaveLength(12 * 7)
      expect(b[0]).toEqual({ x: 0, y: 0, width: 8, height: 8, color: 'rgb(10, 20, 30)' })
      expect(b.at(-1)).toMatchObject({ x: 88, y: 48, width: 12, height: 12 })
    })

    it('grows the blocks with the blur and clips it to the selection, as Rust does', () => {
      const { board, shapes } = placed()
      board.render([{ ...wide, x: 150, y: 100, width: 200, height: 200 }], null)
      // 400 × 400 frame px: 50 px blocks.
      expect(blocksOf(shapes()[0])[0]).toMatchObject({ width: 25, height: 25 })
      // Half out of the selection: blocks start where it does, at x = 1000 - 100 frame px.
      board.render([{ ...wide, x: 450 }], null)
      const b = blocksOf(shapes()[0])
      expect(b[0]).toMatchObject({ x: 0, width: 8 })
      expect(b.filter((k) => k.y === 0).map((k) => k.width)).toEqual([8, 8, 8, 8, 8, 10])
      // All out: no blocks.
      board.render([{ ...wide, x: 600 }], null)
      expect(blocksOf(shapes()[0])).toEqual([])
    })

    it('lines the blocks up again when the selection moves', () => {
      const { board, shapes } = placed()
      board.render([{ ...wide, x: 450 }], null)
      board.place({ ...sel, width: 450 }, display)
      expect(blocksOf(shapes()[0]).filter((k) => k.y === 0)).toHaveLength(12)
      // No selection keeps the last one.
      board.place(null, display)
      expect(blocksOf(shapes()[0]).filter((k) => k.y === 0)).toHaveLength(12)
    })

    it('follows what is under it while it is dragged or resized', () => {
      const { board, shapes, onchange } = placed()
      board.render([wide], null)
      const node = shapes()[0]
      node.setAttrs({ x: 160 })
      node.fire('dragmove')
      expect(blocksOf(node)[0]).toMatchObject({ x: 0, width: 8 })
      // Resized 2x on x: 400 frame px across, still 16 px blocks, in the node's own px.
      node.setAttrs({ scaleX: 2 })
      node.fire('transform')
      expect(blocksOf(node)).toHaveLength(25 * 7)
      expect(blocksOf(node)[0]).toMatchObject({ x: 0, width: 4 })
      expect(onchange).not.toHaveBeenCalled()
      node.fire('transformend')
      expect(onchange).toHaveBeenCalledWith({ ...wide, x: 160, width: 200 })
      // A removed node has nothing to follow.
      board.render([], null)
      node.fire('dragmove')
    })

    it('previews the blur being drawn the same way', () => {
      const { board, drafts } = placed()
      board.preview(wide)
      expect(blocksOf(drafts()[0])).toHaveLength(12 * 7)
    })

    /** A frame whose shrunk px (x, y) is (x, 0, 0), so colors tell where they came from. */
    function gradient() {
      const context = fakeContext()
      context.getImageData.mockImplementation((_x, _y, w = 1, h = 1) => {
        const data = new Uint8ClampedArray(w * h * 4)
        for (let i = 0; i < w * h; i++) data.set([i % w, 0, 0, 255], i * 4)
        return { data }
      })
      const s = setup(canvas(2000, 1400))
      s.board.place(sel, display)
      return s
    }

    it('averages a blur over an earlier one from its blocks, in model order, as Rust does', () => {
      const { board, shapes, drafts } = gradient()
      // One 31 px block over frame px 400 to 431, shrunk px 25 and 26: their average, 26.
      const first: Shape = { ...blur, id: 'b1', x: 200, y: 200, width: 15.5, height: 8 }
      const later: Shape = { ...blur, id: 'b2', x: 200, y: 200, width: 8, height: 8 }
      board.render([first], null)
      expect(blocksOf(shapes()[0])[0].color).toBe('rgb(26, 0, 0)')
      board.render([first, later], null)
      // Over the first, the later one's px 25 is the first's color, not 25.
      expect(blocksOf(shapes()[1])[0].color).toBe('rgb(26, 0, 0)')
      // The other way round, the later one is over nothing.
      board.render([later, first], null)
      expect(blocksOf(shapes()[0])[0].color).toBe('rgb(25, 0, 0)')
      // The blur being drawn comes after all of them.
      board.preview({ ...later, id: 'draft' })
      expect(blocksOf(drafts()[0])[0].color).toBe('rgb(26, 0, 0)')
    })

    it('starts blocks where the crop cuts a blur, off any 16 px grid', () => {
      const { board, shapes } = gradient()
      // The crop starts at frame px 201; the blur at 180 is cut there.
      board.place({ ...sel, x: 100.5 }, display)
      board.render([{ ...wide, x: 90 }], null)
      expect(blocksOf(shapes()[0])[0]).toMatchObject({ x: 10.5, width: 8 })
    })

    it('shows grey boxes with no frame, and keeps the blocks when a change is not kept', () => {
      const { board, shapes } = placed()
      board.render([wide], null)
      const node = shapes()[0]
      node.fire('dragend')
      expect(blocksOf(node)).toHaveLength(12 * 7)
    })

    it('makes the shrunk frame once per frame', () => {
      const { board, context } = placed()
      board.render([wide], null)
      const made = context.getImageData.mock.calls.length
      board.render([{ ...wide, x: 160 }], null)
      expect(context.getImageData).toHaveBeenCalledTimes(made)
      board.newFrame()
      board.render([{ ...wide, x: 170 }], null)
      expect(context.getImageData.mock.calls.length).toBeGreaterThan(made)
    })

    it('is a grey box until there is a frame', () => {
      fakeContext()
      const frame = canvas(0, 0)
      const { board, shapes } = setup(frame)
      board.place(sel, display)
      board.render([wide], null)
      expect(shapes()[0].attrs).toMatchObject({ blocks: [], fill: expect.stringMatching(/^rgba/) })
      // Nor before the display is known, or without a source.
      frame.width = 2000
      frame.height = 1400
      const early = setup(frame)
      early.board.render([wide], null)
      expect(early.shapes()[0].attrs.blocks).toEqual([])
      const none = setup()
      none.board.place(sel, display)
      none.board.render([wide], null)
      expect(none.shapes()[0].attrs.blocks).toEqual([])
    })
  })

  it('attaches the resize handles to the selected shape', () => {
    const { board, shapes, transformer } = setup()
    board.render([rect, arrow], 'a')
    expect(transformer.attached).toEqual([shapes()[1]])
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

  it('draws text as a group of its box and its text, and updates both', () => {
    const { board, shapes } = setup()
    board.render([text], null)
    const [group] = shapes()
    expect(group.className).toBe('Group')
    expect(group.children.map((n) => n.className)).toEqual(['Rect', 'Text'])
    expect(group.attrs).toMatchObject({ x: 110, y: 120, draggable: true })
    board.render([{ ...text, text: 'Hello', background: true, x: 130 }], null)
    expect(shapes()[0]).toBe(group)
    expect(group.attrs.x).toBe(130)
    expect(group.children[0].attrs.fill).toBe(style.color)
    expect(group.children[1].attrs.text).toBe('Hello')
  })

  it('asks to edit a double-clicked text', () => {
    const { board, shapes, onedit } = setup()
    board.render([rect, text], null)
    shapes()[0].fire('dblclick')
    expect(onedit).not.toHaveBeenCalled()
    shapes()[1].fire('dbltap')
    expect(onedit).toHaveBeenCalledWith('t')
  })

  it('resizes text from its corners only, keeping its proportions', () => {
    const { board, transformer } = setup()
    board.render([rect, text], 't')
    expect(transformer.attrs).toMatchObject({ keepRatio: true, shiftBehavior: 'none' })
    expect(transformer.attrs.enabledAnchors).toHaveLength(4)
    board.render([rect, text], 'r')
    expect(transformer.attrs).toMatchObject({ keepRatio: false, shiftBehavior: 'default' })
    expect(transformer.attrs.enabledAnchors).toHaveLength(8)
  })

  it('scales the font of resized text and measures it again', () => {
    const { board, shapes, onchange } = setup()
    board.render([text], null)
    const node = shapes()[0]
    node.setAttrs({ x: 100, scaleX: 2, scaleY: 2 })
    node.fire('transformend')
    // 'Hi' at 40 px in the fake: 2 x 20 + 2 x 12 padding wide, 50 + 24 tall.
    expect(onchange).toHaveBeenLastCalledWith({
      ...text,
      x: 100,
      fontSize: 40,
      width: 64,
      height: 74,
    })
    expect(node.attrs).toMatchObject({ scaleX: 1, scaleY: 1 })
  })

  it('keeps the far corner of flipped text in place when measuring it again', () => {
    const { board, shapes, onchange } = setup()
    board.render([text], null)
    const node = shapes()[0]
    // Flipped through its origin at (160, 150), twice the size.
    node.setAttrs({ x: 160, y: 150, scaleX: -2, scaleY: -2 })
    node.fire('transformend')
    // Measured 64 x 74: the right and bottom edges stay at the origin.
    expect(onchange).toHaveBeenLastCalledWith({
      ...text,
      x: 96,
      y: 76,
      width: 64,
      height: 74,
      fontSize: 40,
    })
  })

  it('previews text like any shape', () => {
    const { board, drafts } = setup()
    board.preview(text)
    board.preview({ ...text, text: 'Yo' })
    const [draft] = drafts()
    expect(draft.children[1].attrs.text).toBe('Yo')
  })

  it('measures text as Konva lays it out, padding included, with one node', () => {
    const { board, stage } = setup()
    // The fake: half the font size per character, lines 1.25 font sizes tall.
    expect(board.measure('abcd\nx', 20)).toEqual({ width: 4 * 10 + 12, height: 2 * 25 + 12 })
    expect(board.measure('ab', 20)).toEqual({ width: 2 * 10 + 12, height: 25 + 12 })
    // Off the stage, and gone with the board.
    expect(stage.children).toHaveLength(2)
    board.destroy()
    expect(stage.destroyed).toBe(true)
  })

  it('measures moved text only when its size changed', () => {
    const { board, shapes, onchange } = setup()
    const measure = vi.spyOn(board, 'measure')
    board.render([text], null)
    shapes()[0].setAttrs({ x: 150 })
    shapes()[0].fire('dragend')
    expect(onchange).toHaveBeenLastCalledWith({ ...text, x: 150 })
    expect(measure).not.toHaveBeenCalled()
  })

  it('ignores events from a node that was removed', () => {
    const { board, shapes, onchange } = setup()
    board.render([rect], null)
    const node = shapes()[0]
    board.render([], null)
    node.fire('dragend')
    expect(onchange).not.toHaveBeenCalled()
  })

  it('hit-tests in window px', () => {
    const { board, stage } = setup()
    expect(board.hits({ x: 110, y: 60 })).toBe(false)
    expect(stage.asked).toEqual({ x: 110, y: 60 })
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
  const two = { x: 2, y: 2 }

  it('renders only the box the shapes paint, at native size', async () => {
    const context = fakeContext()
    const out = await exportShapes([rect, blur], crop, two)
    // The rect's box with its stroke, (106, 116) to (144, 164), in px of the crop.
    expect(out.layer).toMatchObject({ x: 12, y: 32, width: 76, height: 96 })
    expect(out.blurs).toEqual([{ x: 2, y: 4, width: 6, height: 8 }])
    expect(context.getImageData).toHaveBeenCalledWith(0, 0, 76, 96)
    // One RGBA pixel per px of the box, as Rust requires.
    expect(out.layer?.rgba).toHaveLength(76 * 96 * 4)
    expect(out.layer?.rgba.slice(-4)).toEqual(new Uint8ClampedArray([7, 7, 7, 255]))
    const stage = stages.at(-1)!
    const layer = stage.children[0]
    expect(layer.children.map((n) => n.className)).toEqual(['Rect'])
    expect(layer.attrs.exported).toEqual({ x: 106, y: 116, width: 38, height: 48, pixelRatio: 2 })
    expect(stage.destroyed).toBe(true)
  })

  it('renders text at native size within its box', async () => {
    const context = fakeContext()
    const out = await exportShapes([text], crop, two)
    // (109, 119) to (161, 151) at 2x, less the crop's (200, 200).
    expect(out.layer).toMatchObject({ x: 18, y: 38, width: 104, height: 64 })
    expect(context.getImageData).toHaveBeenCalledWith(0, 0, 104, 64)
    const layer = stages.at(-1)!.children[0]
    expect(layer.children.map((n) => n.className)).toEqual(['Group'])
    expect(layer.attrs.exported).toMatchObject({ pixelRatio: 2 })
  })

  it('renders the layer at a larger size, keeping the blurs at 1×', async () => {
    const context = fakeContext()
    const out = await exportShapes([rect, blur], crop, two, 3)
    // Three times the 1× box: the layer is drawn at the output size, not upscaled.
    expect(out.layer).toMatchObject({ x: 36, y: 96, width: 228, height: 288 })
    expect(out.blurs).toEqual([{ x: 2, y: 4, width: 6, height: 8 }])
    expect(context.getImageData).toHaveBeenCalledWith(0, 0, 228, 288)
    expect(out.layer?.rgba).toHaveLength(228 * 288 * 4)
    const layer = stages.at(-1)!.children[0]
    expect(layer.attrs.exported).toEqual({ x: 106, y: 116, width: 38, height: 48, pixelRatio: 6 })
  })

  it('clips the box to the crop', async () => {
    fakeContext()
    const out = await exportShapes([{ ...rect, x: 90 }], crop, two)
    expect(out.layer).toMatchObject({ x: 0, width: 48 })
  })

  it('uses each axis’s own scale', async () => {
    fakeContext()
    const out = await exportShapes([rect], { ...crop, y: 150 }, { x: 2, y: 1.5 })
    // y: (116 to 164) x 1.5 = 174 to 246, less the crop's 150.
    expect(out.layer).toMatchObject({ x: 12, y: 24, width: 76, height: 72 })
    const layer = stages.at(-1)!.children[0]
    // Stretched by 1.5 / 2 on y, then rendered at 2x: y px land at 1.5x.
    expect(layer.attrs.scaleY).toBe(0.75)
    expect(layer.attrs.exported).toEqual({ x: 106, y: 87, width: 38, height: 36, pixelRatio: 2 })
  })

  it('draws nothing when there are only blurs, no shapes, or none in the crop', async () => {
    expect(await exportShapes([blur], crop, two)).toEqual({
      layer: null,
      blurs: [{ x: 2, y: 4, width: 6, height: 8 }],
    })
    expect((await exportShapes([], crop, two)).layer).toBeNull()
    expect((await exportShapes([{ ...rect, x: 900 }], crop, two)).layer).toBeNull()
    expect(stages).toEqual([])
  })
})
