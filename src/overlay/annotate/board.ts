// The Konva side of annotations: draws the shape list and turns drags and resizes on it back
// into shapes. Konva is loaded on first use, so opening the overlay never waits for it.

import type Konva from 'konva'
import type { Point, Rect } from '../geometry'
import { blurRects, origin, paintBounds, physicalIn, transform, type Shape } from './shapes'

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
 * One stage per overlay window, covering only the selection (so it clips to it). The layer is
 * offset by the selection, so shapes keep their display coordinates.
 */
export class Board {
  private readonly stage: Konva.Stage
  private readonly layer: Konva.Layer
  private readonly transformer: Konva.Transformer
  private readonly nodes = new Map<string, { node: Node; shape: Shape }>()
  private draft: { node: Node; kind: Shape['kind'] } | null = null
  private at: Point = { x: 0, y: 0 }

  constructor(
    private readonly K: K,
    container: HTMLDivElement,
    private readonly events: BoardEvents,
  ) {
    this.stage = new K.Stage({ container, width: 0, height: 0 })
    this.layer = new K.Layer()
    this.transformer = new K.Transformer({
      rotateEnabled: false,
      ignoreStroke: true,
      keepRatio: false,
      anchorSize: 8,
      borderStroke: 'white',
      anchorStroke: 'black',
    })
    this.layer.add(this.transformer)
    this.stage.add(this.layer)
  }

  /** Covers the selection, or nothing when there is none. */
  place(selection: Rect | null) {
    const r = selection ?? { x: 0, y: 0, width: 0, height: 0 }
    this.at = { x: r.x, y: r.y }
    this.stage.size({ width: r.width, height: r.height })
    this.layer.position({ x: -r.x, y: -r.y })
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
      this.layer.add(node)
      this.nodes.set(shape.id, { node, shape })
    }
    for (const [id, { node }] of this.nodes) {
      if (seen.has(id)) continue
      node.destroy()
      this.nodes.delete(id)
    }
    const node = selected ? this.nodes.get(selected)?.node : undefined
    this.transformer.nodes(node ? [node] : [])
    this.transformer.moveToTop()
    this.layer.batchDraw()
  }

  /** Shows the shape being drawn, reusing one node; `null` hides it. */
  preview(shape: Shape | null) {
    if (this.draft && (!shape || this.draft.kind !== shape.kind)) {
      this.draft.node.destroy()
      this.draft = null
    }
    if (shape && this.draft) this.draft.node.setAttrs(attrs(shape))
    else if (shape) {
      const node = create(this.K, shape)
      node.listening(false)
      this.layer.add(node)
      this.draft = { node, kind: shape.kind }
    }
    this.layer.batchDraw()
  }

  /** Whether `p` (window px) is on a shape or a resize handle. */
  hits(p: Point): boolean {
    return this.stage.getIntersection({ x: p.x - this.at.x, y: p.y - this.at.y }) !== null
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

/** What `exportShapes` returns. See docs/annotations.md. */
export type AnnotationExport = {
  /**
   * The shapes (blur excluded) as straight-alpha RGBA, row by row, `width × height` px. `null`
   * when nothing is drawn inside the crop.
   */
  rgba: Uint8ClampedArray | null
  /** Where the RGBA goes, in physical px relative to the crop's top-left corner. */
  x: number
  y: number
  width: number
  height: number
  /** Blur rects in physical px relative to the crop. Rust pixelates these. */
  blurs: Rect[]
}

/**
 * Renders `shapes` (logical px on the display) on their own at native size. Only the box the
 * shapes paint is rendered, clipped to `crop` (the physical rect being captured); `scale` is
 * physical px per logical px.
 */
export async function exportShapes(
  shapes: readonly Shape[],
  crop: Rect,
  scale: number,
): Promise<AnnotationExport> {
  const blurs = blurRects(shapes, crop, scale)
  const drawn = shapes.filter((s) => s.kind !== 'blur')
  const nothing = { rgba: null, x: 0, y: 0, width: 0, height: 0, blurs }
  const box = drawn.length ? physicalIn(union(drawn.map(paintBounds)), crop, scale) : null
  if (!box) return nothing
  const K = await loadKonva()
  const stage = new K.Stage({ container: document.createElement('div'), width: 0, height: 0 })
  try {
    const layer = new K.Layer()
    stage.add(layer)
    for (const shape of drawn) layer.add(create(K, shape))
    const canvas = layer.toCanvas({
      x: (crop.x + box.x) / scale,
      y: (crop.y + box.y) / scale,
      width: box.width / scale,
      height: box.height / scale,
      pixelRatio: scale,
    })
    // Read exactly the box's size: rounding can leave the canvas a pixel short.
    const rgba = canvas.getContext('2d')!.getImageData(0, 0, box.width, box.height).data
    return { rgba, ...box, blurs }
  } finally {
    stage.destroy()
  }
}

function union(rects: Rect[]): Rect {
  const x = Math.min(...rects.map((r) => r.x))
  const y = Math.min(...rects.map((r) => r.y))
  const right = Math.max(...rects.map((r) => r.x + r.width))
  const bottom = Math.max(...rects.map((r) => r.y + r.height))
  return { x, y, width: right - x, height: bottom - y }
}
