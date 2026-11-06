// Undo and redo as a command stack, pure. Each command knows how to apply and revert itself on
// an immutable shape list, so a step is cheap and the list itself is never copied into history.

import type { Shape } from './shapes'

export type Command =
  | { kind: 'add'; shape: Shape }
  | { kind: 'update'; before: Shape; after: Shape }
  | { kind: 'remove'; shape: Shape; index: number }
  | { kind: 'clear'; shapes: readonly Shape[] }

export type History = {
  readonly shapes: readonly Shape[]
  readonly done: readonly Command[]
  readonly undone: readonly Command[]
}

/** Steps kept for undo; the oldest is dropped past this. */
export const LIMIT = 200

export const empty: History = { shapes: [], done: [], undone: [] }

const replace = (shapes: readonly Shape[], next: Shape) =>
  shapes.map((s) => (s.id === next.id ? next : s))

function apply(shapes: readonly Shape[], c: Command): readonly Shape[] {
  switch (c.kind) {
    case 'add':
      return [...shapes, c.shape]
    case 'update':
      return replace(shapes, c.after)
    case 'remove':
      return shapes.filter((s) => s.id !== c.shape.id)
    case 'clear':
      return []
  }
}

function revert(shapes: readonly Shape[], c: Command): readonly Shape[] {
  switch (c.kind) {
    case 'add':
      return shapes.filter((s) => s.id !== c.shape.id)
    case 'update':
      return replace(shapes, c.before)
    case 'remove':
      return [...shapes.slice(0, c.index), c.shape, ...shapes.slice(c.index)]
    case 'clear':
      return c.shapes
  }
}

/** Runs a command and makes it the next undo. Anything undone can no longer be redone. */
export function run(h: History, c: Command): History {
  return { shapes: apply(h.shapes, c), done: [...h.done, c].slice(-LIMIT), undone: [] }
}

export function undo(h: History): History {
  const c = h.done.at(-1)
  if (!c) return h
  return { shapes: revert(h.shapes, c), done: h.done.slice(0, -1), undone: [...h.undone, c] }
}

export function redo(h: History): History {
  const c = h.undone.at(-1)
  if (!c) return h
  return { shapes: apply(h.shapes, c), done: [...h.done, c], undone: h.undone.slice(0, -1) }
}

/** Removes a shape by id, or returns `h` if it is not there. */
export function remove(h: History, id: string): History {
  const index = h.shapes.findIndex((s) => s.id === id)
  return index < 0 ? h : run(h, { kind: 'remove', shape: h.shapes[index], index })
}

/** Removes every shape in one undoable step, or returns `h` if there are none. */
export function clear(h: History): History {
  return h.shapes.length === 0 ? h : run(h, { kind: 'clear', shapes: h.shapes })
}

/** Replaces a shape with its edited version in one undoable step, unless nothing changed. */
export function update(h: History, after: Shape): History {
  const before = h.shapes.find((s) => s.id === after.id)
  if (!before || JSON.stringify(before) === JSON.stringify(after)) return h
  return run(h, { kind: 'update', before, after })
}
