// A small stand-in for Konva in jsdom, which has no canvas. It keeps attributes, children and
// event handlers so tests can check what the board drew and fire Konva events on nodes.
// Exports render to a jsdom canvas, so `fakeContext` controls the pixels they read.

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
  position(p: Attrs) {
    Object.assign(this.attrs, p)
  }
  moveToTop() {
    if (!this.parent) return
    this.parent.children = [...this.parent.children.filter((c) => c !== this), this]
  }
  on(events: string, handler: Handler) {
    for (const e of events.split(' ')) {
      this.handlers.set(e, [...(this.handlers.get(e) ?? []), handler])
    }
  }
  /** Calls the node's handlers for `event`, as Konva would. */
  fire(event: string) {
    for (const h of this.handlers.get(event) ?? []) h({ target: this })
  }
  getChildren() {
    return this.children
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
  /** The last point hit-tested, in stage px. */
  asked: unknown = null
  constructor(attrs: Attrs) {
    super('Stage', attrs)
    Stage.all.push(this)
  }
  getIntersection(p: unknown) {
    this.asked = p
    return this.hit
  }
}

class Layer extends FakeNode {
  draws = 0
  constructor(attrs: Attrs = {}) {
    super('Layer', attrs)
  }
  clip(rect: Attrs) {
    this.attrs.clip = rect
  }
  batchDraw() {
    this.draws++
  }
  toCanvas(config: Attrs) {
    this.attrs.exported = config
    return document.createElement('canvas')
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

/**
 * Konva's text measures with a canvas, which jsdom lacks: here every character is half the font
 * size wide, and lines are `lineHeight` font sizes tall.
 */
class Text extends FakeNode {
  constructor(attrs: Attrs = {}) {
    super('Text', attrs)
  }
  private get num() {
    const { fontSize = 12, padding = 0, lineHeight = 1 } = this.attrs as Record<string, number>
    return { fontSize, padding, lineHeight }
  }
  private get lines() {
    return String(this.attrs.text ?? '').split('\n')
  }
  width() {
    const { fontSize, padding } = this.num
    return Math.max(...this.lines.map((l) => l.length)) * (fontSize / 2) + 2 * padding
  }
  height() {
    const { fontSize, padding, lineHeight } = this.num
    return this.lines.length * fontSize * lineHeight + 2 * padding
  }
}

export const FakeKonva = {
  Stage,
  Layer,
  Transformer,
  Rect: shape('Rect'),
  Arrow: shape('Arrow'),
  Group: shape('Group'),
  Text,
}

/** Every stage made so far, newest last. */
export const stages = Stage.all
