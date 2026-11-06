import { describe, expect, it } from 'vitest'
import { LIMIT, clear, empty, redo, remove, run, undo, update, type History } from './history'
import { DEFAULT_STYLE, type Shape } from './shapes'

const box = (id: string, x = 0): Shape => ({
  id,
  kind: 'rect',
  style: DEFAULT_STYLE,
  x,
  y: 0,
  width: 10,
  height: 10,
})
const ids = (h: History) => h.shapes.map((s) => s.id)
const add = (h: History, shape: Shape) => run(h, { kind: 'add', shape })

describe('history', () => {
  it('starts empty with nothing to undo or redo', () => {
    expect(empty.shapes).toEqual([])
    expect(undo(empty)).toBe(empty)
    expect(redo(empty)).toBe(empty)
  })

  it('adds, undoes and redoes', () => {
    const h = add(add(empty, box('a')), box('b'))
    expect(ids(h)).toEqual(['a', 'b'])
    expect(ids(undo(h))).toEqual(['a'])
    expect(ids(undo(undo(h)))).toEqual([])
    expect(ids(redo(undo(undo(h))))).toEqual(['a'])
    expect(ids(redo(redo(undo(undo(h)))))).toEqual(['a', 'b'])
  })

  it('drops the redo steps after a new command', () => {
    const h = add(undo(add(empty, box('a'))), box('b'))
    expect(h.undone).toEqual([])
    expect(redo(h)).toBe(h)
  })

  it('updates a shape and undoes back to the old one', () => {
    const h = update(add(empty, box('a')), box('a', 50))
    expect(h.shapes[0]).toMatchObject({ x: 50 })
    expect(undo(h).shapes[0]).toMatchObject({ x: 0 })
    expect(redo(undo(h)).shapes[0]).toMatchObject({ x: 50 })
  })

  it('skips an update that changes nothing or names no shape', () => {
    const h = add(empty, box('a'))
    expect(update(h, box('a'))).toBe(h)
    expect(update(h, box('zzz', 4))).toBe(h)
  })

  it('removes a shape and puts it back in its place on undo', () => {
    const h = add(add(add(empty, box('a')), box('b')), box('c'))
    const removed = remove(h, 'b')
    expect(ids(removed)).toEqual(['a', 'c'])
    expect(ids(undo(removed))).toEqual(['a', 'b', 'c'])
    expect(remove(h, 'zzz')).toBe(h)
  })

  it('clears everything in one undoable step', () => {
    const h = add(add(empty, box('a')), box('b'))
    const cleared = clear(h)
    expect(ids(cleared)).toEqual([])
    expect(ids(undo(cleared))).toEqual(['a', 'b'])
    expect(ids(redo(undo(cleared)))).toEqual([])
    expect(clear(empty)).toBe(empty)
  })

  it(`keeps the last ${LIMIT} steps`, () => {
    let h = empty
    for (let i = 0; i <= LIMIT; i++) h = add(h, box(`s${i}`))
    expect(h.done).toHaveLength(LIMIT)
    for (let i = 0; i < LIMIT + 5; i++) h = undo(h)
    // The first shape's step was dropped, so it stays.
    expect(ids(h)).toEqual(['s0'])
  })
})
