import { describe, expect, it } from 'vitest'
import { LIMIT, commit, empty, redo, replace, undo, without, type History } from './history'
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
const ids = (h: History) => h.present.map((s) => s.id)
const add = (h: History, shape: Shape) => commit(h, [...h.present, shape])

describe('history', () => {
  it('starts empty with nothing to undo or redo', () => {
    expect(empty.present).toEqual([])
    expect(undo(empty)).toBe(empty)
    expect(redo(empty)).toBe(empty)
  })

  it('commits, undoes and redoes', () => {
    const h = add(add(empty, box('a')), box('b'))
    expect(ids(h)).toEqual(['a', 'b'])
    expect(ids(undo(h))).toEqual(['a'])
    expect(ids(undo(undo(h)))).toEqual([])
    expect(ids(redo(undo(undo(h))))).toEqual(['a'])
    expect(ids(redo(redo(undo(undo(h)))))).toEqual(['a', 'b'])
  })

  it('drops the redo steps after a new commit', () => {
    const h = add(undo(add(empty, box('a'))), box('b'))
    expect(h.future).toEqual([])
    expect(redo(h)).toBe(h)
  })

  it('skips a commit that changes nothing', () => {
    const h = add(empty, box('a'))
    expect(commit(h, h.present)).toBe(h)
  })

  it(`keeps the last ${LIMIT} steps`, () => {
    let h = empty
    for (let i = 0; i <= LIMIT; i++) h = add(h, box(`s${i}`))
    expect(h.past).toHaveLength(LIMIT)
    for (let i = 0; i < LIMIT + 5; i++) h = undo(h)
    // The first step was dropped, so its shape stays.
    expect(ids(h)).toEqual(['s0'])
  })
})

describe('list edits', () => {
  const list = [box('a'), box('b'), box('c')]

  it('replaces a shape in place', () => {
    const next = replace(list, box('b', 50))
    expect(next.map((s) => s.id)).toEqual(['a', 'b', 'c'])
    expect(next[1]).toMatchObject({ x: 50 })
    expect(list[1]).toMatchObject({ x: 0 })
  })

  it('returns the same list when a replace changes nothing or names no shape', () => {
    expect(replace(list, box('b'))).toBe(list)
    expect(replace(list, box('zzz', 4))).toBe(list)
  })

  it('removes a shape, or returns the same list if it is not there', () => {
    expect(without(list, 'b').map((s) => s.id)).toEqual(['a', 'c'])
    expect(without(list, 'zzz')).toBe(list)
  })
})
