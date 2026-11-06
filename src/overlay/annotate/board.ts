// The Konva side of annotations: draws the shape list and turns drags and resizes on it back
// into shapes. Konva is loaded on first use, so opening the overlay never waits for it.

import type Konva from 'konva'
import { toPhysical, type Point, type Rect, type Scale, type Size } from '../geometry'
import { BLOCK, blocks, type Block, type Shrunk } from './blocks'
import {
  HIGHLIGHTER_WIDTH,
  blurRects,
  isPath,
  origin,
  paintedBounds,
  physicalIn,
  transform,
  type Shape,
  type TextShape,
  type Transform,
} from './shapes'
import { radius, textColors, textFont } from './text'

type K = typeof Konva
/** A shape's node: one Konva shape, or a group for text (its background and the text). */
type Node = Konva.Shape | Konva.Group

/** `load`, run once and shared. A failed load is forgotten, so the next call tries again. */
export function once<T>(load: () => Promise<T>): () => Promise<T> {
  let loading: Promise<T> | undefined
  return () =>
    (loading ??= load().catch((e: unknown) => {
      loading = undefined
      throw e
    }))
}

/** Loads Konva on first use. */
export const loadKonva = once(() => import('konva').then((m) => m.default))

/** Hit area around thin lines, in logical px. */
const HIT = 12
/** A blur before the frame's pixels are ready. */
const BLUR_FILL = 'rgba(128, 128, 128, 0.5)'
/** How much of what is under a highlighter shows through. */
const HIGHLIGHTER_OPACITY = 0.4

/**
 * A shape's Konva attributes. Paths are drawn from their first point and boxes from their
 * corner, so they move as one.
 */
export function attrs(shape: Shape): Konva.ShapeConfig {
  const { x, y } = origin(shape)
  const { color, size } = shape.style
  const stroke = {
    x,
    y,
    stroke: color,
    strokeWidth: size,
    hitStrokeWidth: HIT,
    scaleX: 1,
    scaleY: 1,
  }
  if (shape.kind === 'text') return { x, y, scaleX: 1, scaleY: 1 }
  if (isPath(shape)) {
    const [x0, y0] = shape.points
    const points = shape.points.map((v, i) => v - (i % 2 ? y0 : x0))
    const line = { ...stroke, points, lineCap: 'round', lineJoin: 'round' } as const
    if (shape.kind === 'arrow') {
      return { ...line, fill: color, pointerLength: size * 3, pointerWidth: size * 3 }
    }
    if (shape.kind === 'highlighter') {
      // A marker: wide, flat ends, and what is under it shows through.
      const width = size * HIGHLIGHTER_WIDTH
      return {
        ...line,
        strokeWidth: width,
        hitStrokeWidth: Math.max(width, HIT),
        lineCap: 'butt',
        opacity: HIGHLIGHTER_OPACITY,
      }
    }
    return line
  }
  const box = { x, y, width: shape.width, height: shape.height, scaleX: 1, scaleY: 1 }
  if (shape.kind === 'blur') {
    // The outline shows where a blur is over a flat area. `blocks` come from the board.
    const outline = { stroke: 'white', dash: [4, 4], strokeWidth: 1, strokeScaleEnabled: false }
    return { ...box, ...outline, fill: BLUR_FILL, blocks: [], sceneFunc: drawBlur, hitFunc: hitBox }
  }
  if (shape.kind === 'ellipse') {
    // Konva draws an ellipse around its center; the offset puts (x, y) at the box's corner.
    const rx = shape.width / 2
    const ry = shape.height / 2
    return {
      ...stroke,
      radiusX: rx,
      radiusY: ry,
      offsetX: -rx,
      offsetY: -ry,
      strokeScaleEnabled: false,
    }
  }
  return { ...stroke, ...box, strokeScaleEnabled: false }
}

/** The attributes of a text group's children: the background box and the text. */
export function textAttrs(shape: TextShape): { box: Konva.RectConfig; label: Konva.TextConfig } {
  const { fill, box } = textColors(shape.style.color, shape.background)
  return {
    box: {
      width: shape.width,
      height: shape.height,
      fill: box,
      cornerRadius: radius(shape.fontSize),
    },
    label: {
      text: shape.text,
      ...textFont(shape.fontSize),
      fill,
    },
  }
}

function create(K: K, shape: Shape, a = attrs(shape)): Node {
  switch (shape.kind) {
    case 'text': {
      const { box, label } = textAttrs(shape)
      const group = new K.Group(a)
      group.add(new K.Rect(box))
      group.add(new K.Text(label))
      return group
    }
    case 'arrow':
      return new K.Arrow(a as Konva.ArrowConfig)
    case 'line':
    case 'pen':
    case 'highlighter':
      return new K.Line(a as Konva.LineConfig)
    case 'ellipse':
      return new K.Ellipse(a as Konva.EllipseConfig)
    case 'blur':
      return new K.Shape(a)
    default:
      return new K.Rect(a)
  }
}

/** The box, filled and outlined: a blur's outline, and all of it for hit tests. */
function hitBox(context: Konva.Context, shape: Konva.Shape) {
  context.beginPath()
  context.rect(0, 0, shape.width(), shape.height())
  context.closePath()
  context.fillStrokeShape(shape)
}

/** A blur: its blocks (`blocks`, in the node's own px), then its outline. */
function drawBlur(context: Konva.Context, shape: Konva.Shape) {
  for (const b of shape.getAttr('blocks') as Block[]) {
    context.setAttr('fillStyle', b.color)
    context.fillRect(b.x, b.y, b.width, b.height)
  }
  hitBox(context, shape)
}

/**
 * The frame shrunk by `block` on each side (the smallest blur block), for the blur preview.
 * Halved step by step, so every frame pixel counts. `null` without a frame or a 2D context.
 */
export function shrink(source: HTMLCanvasElement, block = BLOCK): Shrunk | null {
  const w = Math.ceil(source.width / block)
  const h = Math.ceil(source.height / block)
  let image = source
  while (image.width > w || image.height > h) {
    const next = document.createElement('canvas')
    next.width = Math.max(w, Math.ceil(image.width / 2))
    next.height = Math.max(h, Math.ceil(image.height / 2))
    const context = next.getContext('2d')
    if (!context) return null
    context.drawImage(image, 0, 0, next.width, next.height)
    image = next
  }
  if (image === source) return null
  const { width, height } = image
  const data = image.getContext('2d')!.getImageData(0, 0, width, height).data
  return { data, width, height, factor: { x: source.width / width, y: source.height / height } }
}

/** Frame px `v` at `s` per logical px, in the px of a node at `at` scaled by `k`. */
const local = (v: number, s: number, at: number, k: number) => (v / s - at) / k

/** Blurs stack under the other shapes. */
const layerOf = (s: Shape) => (s.kind === 'blur' ? 0 : 1)

/** Where a node is and how it is scaled. */
function placement(node: Node): Transform {
  return { x: node.x(), y: node.y(), scaleX: node.scaleX(), scaleY: node.scaleY() }
}

/** The shape as its node now shows it, moved or resized. */
function moved({ node, shape }: { node: Node; shape: Shape }): Shape {
  return transform(shape, placement(node))
}

const CORNERS = ['top-left', 'top-right', 'bottom-left', 'bottom-right']
const ANCHORS = [...CORNERS, 'top-center', 'middle-right', 'middle-left', 'bottom-center']

export type BoardEvents = {
  /** A shape was moved or resized on the board. */
  onchange: (shape: Shape) => void
  /** A shape was pressed. */
  onselect: (id: string) => void
  /** A text was double-clicked, to edit it. */
  onedit: (id: string) => void
}

/**
 * One stage per overlay window, covering the display while there is a selection (and nothing
 * otherwise). Shapes sit on a layer clipped to the selection; the shape being drawn and the
 * resize handles sit on a small top layer, unclipped, so handles at the edge stay reachable
 * and drawing redraws only that layer. Blurs show the frame under them in blocks, from
 * `source` (the frozen frame's canvas), and always sit under the other shapes, as in the
 * exported image.
 */
export class Board {
  private readonly stage: Konva.Stage
  private readonly shapes: Konva.Layer
  private readonly top: Konva.Layer
  private readonly transformer: Konva.Transformer
  private readonly nodes = new Map<string, { node: Node; shape: Shape }>()
  private draft: { node: Node; kind: Shape['kind'] } | null = null
  /** Whether the handles are set up for text. */
  private textHandles = false
  /** Lays out text off the stage, to measure it. Reused for every keystroke. */
  private measurer: Konva.Text | null = null
  private display: Size = { width: 0, height: 0 }
  private selection: Rect | null = null
  /**
   * The shrunk frame for the blur preview, the frame's size, and its px per logical px. Made
   * on first use.
   */
  private pixels: { shrunk: Shrunk; size: Size; scale: Scale } | null = null
  /** The shrunk frame with every blur painted on it, for the blur being drawn. */
  private painted: Shrunk | null = null
  /** The blurs' ids in model order: the order Rust pixelates them in. */
  private blurs: string[] = []

  constructor(
    private readonly K: K,
    container: HTMLDivElement,
    private readonly events: BoardEvents,
    private readonly source: () => HTMLCanvasElement | null = () => null,
  ) {
    this.stage = new K.Stage({ container, width: 0, height: 0 })
    this.shapes = new K.Layer()
    this.top = new K.Layer()
    this.transformer = new K.Transformer({
      rotateEnabled: false,
      ignoreStroke: true,
      keepRatio: false,
      anchorSize: 8,
      borderStroke: 'white',
      anchorStroke: 'black',
    })
    this.top.add(this.transformer)
    this.stage.add(this.shapes)
    this.stage.add(this.top)
  }

  /** Covers `display` and clips the shapes to `selection`; with no selection, nothing. */
  place(selection: Rect | null, display: Size) {
    this.display = display
    this.stage.size(selection ? display : { width: 0, height: 0 })
    if (!selection) return
    this.shapes.clip(selection)
    // Blur blocks line up with the part of the blur in the selection, as in the export.
    this.selection = selection
    this.paintBlurs()
    this.shapes.batchDraw()
  }

  /** The frame changed: blurs drawn from now on preview the new one. */
  newFrame() {
    this.pixels = null
    this.painted = null
  }

  /** The shrunk frame, made on first use; `null` while there is no frame. */
  private frame() {
    if (this.pixels || !this.display.width || !this.display.height) return this.pixels
    const source = this.source()
    const shrunk = source && shrink(source)
    if (!shrunk) return null
    const size = { width: source.width, height: source.height }
    const scale = { x: size.width / this.display.width, y: size.height / this.display.height }
    return (this.pixels = { shrunk, size, scale })
  }

  /**
   * Shows the blocks Rust will paint for the blur `r` (logical px) on `node`, placed by `t`,
   * and paints them on `work`, a copy of the shrunk frame: in whole frame px, clipped to the
   * selection like the export, from the corner of what is left. Without a frame, a grey box.
   */
  private showBlocks(node: Node, r: Rect, t: Transform, work: Shrunk | null) {
    const frame = this.pixels
    const crop = frame && this.selection && toPhysical(this.selection, frame.scale, frame.size)
    const p = crop && physicalIn(r, crop, frame.scale)
    const shown = !p || !work ? [] : blocks(p, crop, work)
    const { x: sx, y: sy } = frame?.scale ?? { x: 1, y: 1 }
    node.setAttrs({
      blocks: shown.map((b) => ({
        color: b.color,
        x: local(b.x, sx, t.x, t.scaleX),
        y: local(b.y, sy, t.y, t.scaleY),
        width: b.width / sx / t.scaleX,
        height: b.height / sy / t.scaleY,
      })),
      // With blocks the fill is clear, but a press on it still hits.
      fill: shown.length ? 'transparent' : BLUR_FILL,
    })
  }

  /**
   * Shows every blur's blocks, worked out in model order over one copy of the shrunk frame, as
   * Rust pixelates them one after another: a blur over another averages its blocks. `moving`
   * is a blur being dragged or resized, shown where its node is. What the blurs leave is kept
   * for the blur being drawn, which comes after them.
   */
  private paintBlurs(moving?: string) {
    const blurs = this.blurs.map((id) => this.nodes.get(id)!)
    const frame = blurs.length ? this.frame() : null
    const work = frame && { ...frame.shrunk, data: frame.shrunk.data.slice() }
    for (const entry of blurs) {
      const here = entry.shape.id === moving
      const shape = (here ? moved(entry) : entry.shape) as Rect
      const t = here ? placement(entry.node) : { x: shape.x, y: shape.y, scaleX: 1, scaleY: 1 }
      this.showBlocks(entry.node, shape, t, work)
    }
    this.painted = work
  }

  /** Keeps a blur showing the blocks under it while it is dragged or resized. */
  private follow(id: string) {
    this.paintBlurs(id)
  }

  /** Makes the board show `shapes`, with handles on the selected one. */
  render(shapes: readonly Shape[], selected: string | null) {
    const seen = new Set<string>()
    for (const shape of shapes) {
      seen.add(shape.id)
      const entry = this.nodes.get(shape.id)
      if (entry) {
        if (entry.shape !== shape) this.update(entry.node, shape)
        entry.shape = shape
        continue
      }
      const node = create(this.K, shape)
      node.draggable(true)
      // Konva's `on` overloads differ between Shape and Group; Node's are shared.
      const events: Konva.Node = node
      events.on('pointerdown', () => this.events.onselect(shape.id))
      events.on('dragend transformend', () => this.changed(shape.id))
      if (shape.kind === 'blur') events.on('dragmove transform', () => this.follow(shape.id))
      if (shape.kind === 'text') events.on('dblclick dbltap', () => this.events.onedit(shape.id))
      this.shapes.add(node)
      this.nodes.set(shape.id, { node, shape })
    }
    for (const [id, { node }] of this.nodes) {
      if (seen.has(id)) continue
      node.destroy()
      this.nodes.delete(id)
    }
    // Stacked in model order, so undo and redo restore what was on top. Blurs go under the
    // rest: Rust pixelates the frame before it draws the other shapes.
    for (const shape of shapes.toSorted((a, b) => layerOf(a) - layerOf(b))) {
      this.nodes.get(shape.id)!.node.moveToTop()
    }
    const entry = selected ? this.nodes.get(selected) : undefined
    // Text resizes from its corners and keeps its proportions: only the font size changes.
    const text = entry?.shape.kind === 'text'
    if (text !== this.textHandles) {
      this.textHandles = text
      this.transformer.setAttrs({
        keepRatio: text,
        // Shift would let text stretch; it only scales.
        shiftBehavior: text ? 'none' : 'default',
        enabledAnchors: text ? CORNERS : ANCHORS,
      })
    }
    this.transformer.nodes(entry ? [entry.node] : [])
    this.blurs = shapes.filter((s) => s.kind === 'blur').map((s) => s.id)
    this.paintBlurs()
    this.shapes.batchDraw()
    this.top.batchDraw()
  }

  /** Shows the shape being drawn, reusing one node; `null` hides it. Redraws the top only. */
  preview(shape: Shape | null) {
    if (this.draft && (!shape || this.draft.kind !== shape.kind)) {
      this.draft.node.destroy()
      this.draft = null
    }
    if (shape && this.draft) this.update(this.draft.node, shape)
    else if (shape) {
      const node = create(this.K, shape)
      node.listening(false)
      this.top.add(node)
      this.draft = { node, kind: shape.kind }
    }
    // A new blur comes after the others, over what they leave.
    if (shape?.kind === 'blur') {
      const work = this.painted ?? this.frame()?.shrunk
      const after = work ? { ...work, data: work.data.slice() } : null
      this.showBlocks(this.draft!.node, shape, { ...shape, scaleX: 1, scaleY: 1 }, after)
    }
    this.top.batchDraw()
  }

  /** Sets a node's attributes to match its shape. */
  private update(node: Node, shape: Shape) {
    node.setAttrs(attrs(shape))
    if (shape.kind !== 'text') return
    const { box, label } = textAttrs(shape)
    const [background, text] = (node as Konva.Group).getChildren()
    background.setAttrs(box)
    text.setAttrs(label)
  }

  /** The box `text` takes at `fontSize`, padding included, as Konva lays it out. */
  measure = (text: string, fontSize: number): Size => {
    this.measurer ??= new this.K.Text()
    this.measurer.setAttrs({ text, ...textFont(fontSize) })
    return { width: this.measurer.width(), height: this.measurer.height() }
  }

  /** Whether `p` (window px) is on a shape or a resize handle. */
  hits(p: Point): boolean {
    return this.stage.getIntersection(p) !== null
  }

  private changed(id: string) {
    const entry = this.nodes.get(id)
    if (!entry) return
    const { node, shape } = entry
    let next = moved(entry)
    // Resized text is measured again at its new font size.
    // A flipped resize keeps the node's origin, now the box's far corner, in place.
    if (next.kind === 'text' && shape.kind === 'text' && next.fontSize !== shape.fontSize) {
      const m = this.measure(next.text, next.fontSize)
      const x = node.scaleX() < 0 ? next.x + next.width - m.width : next.x
      const y = node.scaleY() < 0 ? next.y + next.height - m.height : next.y
      next = { ...next, x, y, ...m }
    }
    // Back to the model: the change comes back through `render` once it is kept.
    node.setAttrs(attrs(shape))
    this.paintBlurs()
    this.events.onchange(next)
  }

  destroy() {
    this.measurer?.destroy()
    this.stage.destroy()
    this.nodes.clear()
  }
}

/** The drawn shapes as pixels, and where they go on the crop. */
export type Layer = {
  /** Straight-alpha RGBA, row by row, `width × height` px. */
  rgba: Uint8ClampedArray
  /** Where it goes, in physical px relative to the crop's top-left corner. */
  x: number
  y: number
  width: number
  height: number
}

/** What `exportShapes` returns. See docs/annotations.md. */
export type AnnotationExport = {
  /** The shapes (blur excluded), or `null` when none is drawn inside the crop. */
  layer: Layer | null
  /** Blur rects in physical px relative to the crop. Rust pixelates these. */
  blurs: Rect[]
}

/**
 * Renders `shapes` (logical px on the display) on their own at native size. Only the box the
 * shapes paint is rendered, clipped to `crop` (the physical rect being captured); `scale` is
 * physical px per logical px on each axis.
 */
export async function exportShapes(
  shapes: readonly Shape[],
  crop: Rect,
  scale: Scale,
): Promise<AnnotationExport> {
  const blurs = blurRects(shapes, crop, scale)
  const drawn = shapes.filter((s) => s.kind !== 'blur')
  const painted = paintedBounds(drawn)
  const box = painted && physicalIn(painted, crop, scale)
  if (!box) return { layer: null, blurs }
  const K = await loadKonva()
  const stage = new K.Stage({ container: document.createElement('div'), width: 0, height: 0 })
  try {
    // Konva has one pixel ratio, the x one; stretching the layer gives y its own.
    const layer = new K.Layer({ scaleY: scale.y / scale.x })
    stage.add(layer)
    for (const shape of drawn) layer.add(create(K, shape))
    const canvas = layer.toCanvas({
      x: (crop.x + box.x) / scale.x,
      y: (crop.y + box.y) / scale.x,
      width: box.width / scale.x,
      height: box.height / scale.x,
      pixelRatio: scale.x,
    })
    // Read exactly the box's size: rounding can leave the canvas a pixel short.
    const rgba = canvas.getContext('2d')!.getImageData(0, 0, box.width, box.height).data
    return { layer: { rgba, ...box }, blurs }
  } finally {
    stage.destroy()
  }
}
