// The Konva side of annotations: draws the shape list and turns drags and resizes on it back
// into shapes. Konva is loaded on first use, so opening the overlay never waits for it.

import type Konva from 'konva'
import type { Point, Rect, Size } from '../geometry'
import { blurRects, origin, transform, type Shape } from './shapes'

type K = typeof Konva
type Node = Konva.Shape

/** Loads Konva once. Kept separate so tests can swap it. */
let loading: Promise<K> | undefined
export const loadKonva = () => (loading ??= import('konva').then((m) => m.default))

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
    }
  }
  const common = { x, y, width: shape.width, height: shape.height, strokeScaleEnabled: false }
  if (shape.kind === 'blur')
    return { ...common, fill: BLUR_FILL, stroke: 'white', dash: [4, 4], strokeWidth: 1 }
  return { ...common, stroke: color, strokeWidth: size, hitStrokeWidth: HIT }
}

function create(K: K, shape: Shape): Node {
  return shape.kind === 'arrow'
    ? new K.Arrow(attrs(shape) as Konva.ArrowConfig)
    : new K.Rect(attrs(shape))
}

export type BoardEvents = {
  /** A shape was moved or resized on the board. */
  onchange: (shape: Shape) => void
  /** A shape was clicked. */
  onselect: (id: string) => void
}

/** One stage per overlay window, sized to the display. Shapes sit in a group at the selection. */
export class Board {
  private readonly stage: Konva.Stage
  private readonly layer: Konva.Layer
  private readonly group: Konva.Group
  private readonly transformer: Konva.Transformer
  private readonly nodes = new Map<string, { node: Node; shape: Shape }>()
  private draft: Node | null = null

  constructor(
    private readonly K: K,
    container: HTMLDivElement,
    size: Size,
    private readonly events: BoardEvents,
  ) {
    this.stage = new K.Stage({ container, ...size })
    this.layer = new K.Layer()
    this.group = new K.Group()
    this.transformer = new K.Transformer({
      rotateEnabled: false,
      ignoreStroke: true,
      keepRatio: false,
      anchorSize: 8,
      borderStroke: 'white',
      anchorStroke: 'black',
    })
    this.layer.add(this.group)
    this.layer.add(this.transformer)
    this.stage.add(this.layer)
  }

  resize(size: Size) {
    this.stage.size(size)
  }

  /** Puts the shapes at the selection and clips them to it. */
  place(selection: Rect) {
    this.group.setAttrs({
      x: selection.x,
      y: selection.y,
      clip: { x: 0, y: 0, width: selection.width, height: selection.height },
    })
  }

  /** Makes the board show `shapes`, the shape being drawn and the selected shape. */
  render(shapes: readonly Shape[], draft: Shape | null, selected: string | null) {
    const seen = new Set<string>()
    for (const shape of shapes) {
      seen.add(shape.id)
      const entry = this.nodes.get(shape.id)
      if (entry) {
        if (entry.shape !== shape) entry.node.setAttrs({ ...attrs(shape), scaleX: 1, scaleY: 1 })
        entry.shape = shape
        continue
      }
      const node = create(this.K, shape)
      node.draggable(true)
      node.on('pointerdown', () => this.events.onselect(shape.id))
      node.on('dragend transformend', () => this.changed(shape.id))
      this.group.add(node)
      this.nodes.set(shape.id, { node, shape })
    }
    for (const [id, { node }] of this.nodes) {
      if (seen.has(id)) continue
      node.destroy()
      this.nodes.delete(id)
    }
    this.draft?.destroy()
    this.draft = draft && create(this.K, draft)
    if (this.draft) {
      this.draft.listening(false)
      this.group.add(this.draft)
    }
    const node = selected ? this.nodes.get(selected)?.node : undefined
    this.transformer.nodes(node ? [node] : [])
    this.layer.batchDraw()
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
    node.setAttrs({ scaleX: 1, scaleY: 1 })
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
   * The shapes (blur excluded) as straight-alpha RGBA, row by row, on a transparent background
   * the size of the crop. `null` when there is nothing to draw.
   */
  rgba: Uint8ClampedArray | null
  width: number
  height: number
  /** Blur rects in physical px of the crop. Rust pixelates these. */
  blurs: Rect[]
}

/**
 * Renders `shapes` (logical px, relative to the selection) on their own at the crop's physical
 * size. `crop` is the physical rect being captured; `scale` is physical px per logical px.
 */
export async function exportShapes(
  shapes: readonly Shape[],
  crop: Rect,
  scale: number,
): Promise<AnnotationExport> {
  const { width, height } = crop
  const blurs = blurRects(shapes, crop, scale)
  const drawn = shapes.filter((s) => s.kind !== 'blur')
  if (drawn.length === 0 || width <= 0 || height <= 0) return { rgba: null, width, height, blurs }
  const K = await loadKonva()
  const size = { width: width / scale, height: height / scale }
  const stage = new K.Stage({ container: document.createElement('div'), ...size })
  try {
    const layer = new K.Layer()
    stage.add(layer)
    for (const shape of drawn) layer.add(create(K, shape))
    const canvas = layer.toCanvas({ ...size, x: 0, y: 0, pixelRatio: scale })
    // Read exactly the crop's size: rounding can leave the canvas a pixel short.
    const rgba = canvas.getContext('2d')!.getImageData(0, 0, width, height).data
    return { rgba, width, height, blurs }
  } finally {
    stage.destroy()
  }
}
