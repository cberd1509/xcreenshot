// A small stand-in for Konva in jsdom, which has no canvas. It keeps attributes, children and
// event handlers so tests can check what the board drew and fire Konva events on nodes.

type Attrs = Record<string, unknown>
type Handler = (e: unknown) => void

export class FakeNode {
  attrs: Attrs
  children: FakeNode[] = []
  parent: FakeNode | null = null
  handlers = new Map<string, Handler[]>()
  destroyed = false

  constructor(
    readonly className: string,
    attrs: Attrs = {},
  ) {
    this.attrs = { ...attrs }
  }

  setAttrs(attrs: Attrs) {
    Object.assign(this.attrs, attrs)
    return this
  }
  x() {
    return (this.attrs.x as number | undefined) ?? 0
  }
  y() {
    return (this.attrs.y as number | undefined) ?? 0
  }
  scaleX() {
    return (this.attrs.scaleX as number | undefined) ?? 1
  }
  scaleY() {
    return (this.attrs.scaleY as number | undefined) ?? 1
  }
  draggable(on: boolean) {
    this.attrs.draggable = on
  }
  listening(on: boolean) {
    this.attrs.listening = on
  }
  size(size: Attrs) {
    Object.assign(this.attrs, size)
  }
  on(events: string, handler: Handler) {
    for (const e of events.split(' '))
      this.handlers.set(e, [...(this.handlers.get(e) ?? []), handler])
  }
  /** Calls the node's handlers for `event`, as Konva would. */
  fire(event: string) {
    for (const h of this.handlers.get(event) ?? []) h({ target: this })
  }
  add(child: FakeNode) {
    child.parent = this
    this.children.push(child)
  }
  destroy() {
    this.destroyed = true
    if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this)
  }
}

class Stage extends FakeNode {
  static all: Stage[] = []
  /** What `getIntersection` returns. */
  hit: FakeNode | null = null
  constructor(attrs: Attrs) {
    super('Stage', attrs)
    Stage.all.push(this)
  }
  getIntersection() {
    return this.hit
  }
}

class Layer extends FakeNode {
  draws = 0
  constructor() {
    super('Layer')
  }
  batchDraw() {
    this.draws++
  }
  /** A canvas whose pixels are all `7`, so tests can tell it apart from empty. */
  toCanvas(config: Attrs) {
    this.attrs.exported = config
    return {
      getContext: () => ({
        getImageData: (_x: number, _y: number, w: number, h: number) => ({
          data: new Uint8ClampedArray(w * h * 4).fill(7),
        }),
      }),
    }
  }
}

class Transformer extends FakeNode {
  attached: FakeNode[] = []
  constructor(attrs: Attrs) {
    super('Transformer', attrs)
  }
  nodes(nodes: FakeNode[]) {
    this.attached = nodes
  }
}

const shape = (className: string) =>
  class extends FakeNode {
    constructor(attrs: Attrs = {}) {
      super(className, attrs)
    }
  }

export const FakeKonva = {
  Stage,
  Layer,
  Transformer,
  Group: shape('Group'),
  Rect: shape('Rect'),
  Arrow: shape('Arrow'),
}

/** Every stage made so far, newest last. */
export const stages = Stage.all
