// Undo and redo as snapshots of the shape list, pure. Lists are immutable and small, so keeping
// whole snapshots is cheap and simple.

import type { Shape } from './shapes'

export type Shapes = readonly Shape[]

export type History = {
  readonly past: readonly Shapes[]
  readonly present: Shapes
  readonly future: readonly Shapes[]
}

/** Steps kept for undo; the oldest is dropped past this. */
export const LIMIT = 200

export const empty: History = { past: [], present: [], future: [] }

/** Makes `next` the current list, undoably. Anything undone can no longer be redone. */
export function commit(h: History, next: Shapes): History {
  if (next === h.present) return h
  return { past: [...h.past, h.present].slice(-LIMIT), present: next, future: [] }
}

export function undo(h: History): History {
  const prev = h.past[h.past.length - 1]
  if (!prev) return h
  return { past: h.past.slice(0, -1), present: prev, future: [h.present, ...h.future] }
}

export function redo(h: History): History {
  const [next, ...future] = h.future
  if (!next) return h
  return { past: [...h.past, h.present], present: next, future }
}

/** The list with `shape` in place of the one with its id, or the same list if nothing changed. */
export function replace(shapes: Shapes, shape: Shape): Shapes {
  const i = shapes.findIndex((s) => s.id === shape.id)
  if (i < 0 || JSON.stringify(shapes[i]) === JSON.stringify(shape)) return shapes
  return shapes.map((s, j) => (j === i ? shape : s))
}

/** The list without the shape `id`, or the same list if it is not there. */
export function without(shapes: Shapes, id: string): Shapes {
  return shapes.some((s) => s.id === id) ? shapes.filter((s) => s.id !== id) : shapes
}
