// The Konva side of annotations: draws the shape list and turns drags and resizes on it back
// into shapes. Konva is loaded on first use, so opening the overlay never waits for it.

import type Konva from 'konva'
import type { Point, Rect, Scale, Size } from '../geometry'
import { blurRects, origin, paintedBounds, physicalIn, transform, type Shape } from './shapes'

type K = typeof Konva
type Node = Konva.Shape

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
const BLUR_FILL = 'rgba(128, 128, 128, 0.5)'

/** A shape's Konva attributes. Arrows are drawn from their start, so they move as one. */
export function attrs(shape: Shape): Konva.ShapeConfig {
  const { x, y } = origin(shape)
  const { color, size } = shape.style
  if (shape.kind === 'arrow') {
    const [x1, y1, x2, y2] = shape.points
    const head = size * 3
    return {
      x,
      y,
      points: [0, 0, x2 - x1, y2 - y1],
      stroke: color,
      fill: color,
      strokeWidth: size,
      pointerLength: head,
      pointerWidth: head,
      hitStrokeWidth: HIT,
      lineCap: 'round',
      lineJoin: 'round',
      scaleX: 1,
      scaleY: 1,
    }
  }
  const box = { x, y, width: shape.width, height: shape.height, scaleX: 1, scaleY: 1 }
  if (shape.kind === 'blur') {
    return { ...box, fill: BLUR_FILL, stroke: 'white', dash: [4, 4], strokeWidth: 1 }
  }
  return {
    ...box,
    stroke: color,
    strokeWidth: size,
    hitStrokeWidth: HIT,
    strokeScaleEnabled: false,
  }
}

function create(K: K, shape: Shape): Node {
  return shape.kind === 'arrow'
    ? new K.Arrow(attrs(shape) as Konva.ArrowConfig)
    : new K.Rect(attrs(shape))
}

export type BoardEvents = {
  /** A shape was moved or resized on the board. */
  onchange: (shape: Shape) => void
  /** A shape was pressed. */
  onselect: (id: string) => void
}

/**
 * One stage per overlay window, covering the display while there is a selection (and nothing
 * otherwise). Shapes sit on a layer clipped to the selection; the shape being drawn and the
 * resize handles sit on a small top layer, unclipped, so handles at the edge stay reachable
 * and drawing redraws only that layer.
 */
export class Board {
  private readonly stage: Konva.Stage
  private readonly shapes: Konva.Layer
  private readonly top: Konva.Layer
  private readonly transformer: Konva.Transformer
  private readonly nodes = new Map<string, { node: Node; shape: Shape }>()
  private draft: { node: Node; kind: Shape['kind'] } | null = null

  constructor(
    private readonly K: K,
    container: HTMLDivElement,
    private readonly events: BoardEvents,
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
    this.stage.size(selection ? display : { width: 0, height: 0 })
    if (selection) this.shapes.clip(selection)
  }

  /** Makes the board show `shapes`, with handles on the selected one. */
  render(shapes: readonly Shape[], selected: string | null) {
    const seen = new Set<string>()
    for (const shape of shapes) {
      seen.add(shape.id)
      const entry = this.nodes.get(shape.id)
      if (entry) {
        if (entry.shape !== shape) entry.node.setAttrs(attrs(shape))
        entry.shape = shape
        continue
      }
      const node = create(this.K, shape)
      node.draggable(true)
      node.on('pointerdown', () => this.events.onselect(shape.id))
      node.on('dragend transformend', () => this.changed(shape.id))
      this.shapes.add(node)
      this.nodes.set(shape.id, { node, shape })
    }
    for (const [id, { node }] of this.nodes) {
      if (seen.has(id)) continue
      node.destroy()
      this.nodes.delete(id)
    }
    const node = selected ? this.nodes.get(selected)?.node : undefined
    this.transformer.nodes(node ? [node] : [])
    this.shapes.batchDraw()
    this.top.batchDraw()
  }

  /** Shows the shape being drawn, reusing one node; `null` hides it. Redraws the top only. */
  preview(shape: Shape | null) {
    if (this.draft && (!shape || this.draft.kind !== shape.kind)) {
      this.draft.node.destroy()
      this.draft = null
    }
    if (shape && this.draft) this.draft.node.setAttrs(attrs(shape))
    else if (shape) {
      const node = create(this.K, shape)
      node.listening(false)
      this.top.add(node)
      this.draft = { node, kind: shape.kind }
    }
    this.top.batchDraw()
  }

  /** Whether `p` (window px) is on a shape or a resize handle. */
  hits(p: Point): boolean {
    return this.stage.getIntersection(p) !== null
  }

  private changed(id: string) {
    const entry = this.nodes.get(id)
    if (!entry) return
    const { node, shape } = entry
    const next = transform(shape, {
      x: node.x(),
      y: node.y(),
      scaleX: node.scaleX(),
      scaleY: node.scaleY(),
    })
    // Back to the model: the change comes back through `render` once it is kept.
    node.setAttrs(attrs(shape))
    this.events.onchange(next)
  }

  destroy() {
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
