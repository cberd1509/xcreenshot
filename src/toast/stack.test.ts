import { describe, expect, it } from 'vitest'
import { FADE, SHOWN, TITLES, detail, look, next, prune, push, up, type ToastEvent } from './stack'

const copied = (seq: number, width = 1260): ToastEvent => ({
  seq,
  kind: 'copied',
  width,
  height: 1064,
})
const saved = (seq: number): ToastEvent => ({ seq, kind: 'saved', file: 'shot.png' })
const shared = (seq: number, service: string | null = 'AirDrop'): ToastEvent => ({
  seq,
  kind: 'shared',
  service,
})

describe('toast stack', () => {
  it('says what was done and its detail', () => {
    expect(detail(copied(1))).toBe('1260 × 1064')
    expect(detail(saved(1))).toBe('shot.png')
    expect(detail(shared(1))).toBe('AirDrop')
    expect(detail(shared(1, null))).toBe('')
    const [toast] = push([], saved(4), 0)
    expect(toast).toEqual({
      id: 4,
      kind: 'saved',
      detail: 'shot.png',
      count: 1,
      fades: SHOWN,
    })
    expect(TITLES).toEqual({ copied: 'Copied', saved: 'Saved', shared: 'Shared' })
  })

  it('puts a new toast at the bottom and keeps three', () => {
    let stack = push([], saved(1), 0)
    stack = push(stack, shared(2), 10)
    stack = push(stack, copied(3), 20)
    expect(stack.map((t) => t.kind)).toEqual(['saved', 'shared', 'copied'])
    // Saved again once its toast is fading: a new one, and the oldest goes.
    stack = push(stack, saved(4), SHOWN)
    expect(stack.map((t) => t.id)).toEqual([2, 3, 4])
  })

  it('counts a repeat on its toast, which moves to the bottom with the new detail', () => {
    let stack = push([], copied(1), 0)
    stack = push(stack, saved(2), 100)
    stack = push(stack, copied(3, 640), 200)
    expect(stack.map((t) => [t.id, t.count])).toEqual([
      [2, 1],
      [1, 2],
    ])
    expect(stack[1]).toMatchObject({ detail: '640 × 1064', fades: 200 + SHOWN })
    expect(push(stack, copied(4), 300)[1].count).toBe(3)
  })

  it('does not count on a toast already fading', () => {
    const stack = push(push([], copied(1), 0), copied(2), SHOWN)
    expect(stack.map((t) => [t.id, t.count])).toEqual([
      [1, 1],
      [2, 1],
    ])
  })

  it('fades each toast after it shows, then drops it', () => {
    const stack = push(push([], copied(1), 0), saved(2), 500)
    expect(up(stack[0], SHOWN - 1)).toBe(true)
    expect(up(stack[0], SHOWN)).toBe(false)
    expect(prune(stack, SHOWN + FADE - 1)).toHaveLength(2)
    expect(prune(stack, SHOWN + FADE)).toEqual([stack[1]])
    expect(prune(stack, 500 + SHOWN + FADE)).toEqual([])
  })

  it('wakes up when a toast starts or ends its fade', () => {
    const stack = push(push([], copied(1), 0), saved(2), 500)
    expect(next(stack, 0)).toBe(SHOWN)
    expect(next(stack, SHOWN)).toBe(SHOWN + FADE)
    expect(next(stack, SHOWN + FADE)).toBe(500 + SHOWN)
    expect(next(stack, 500 + SHOWN + FADE)).toBe(null)
    expect(next([], 0)).toBe(null)
  })

  it('shows older toasts smaller and dimmer', () => {
    expect(look(0)).toEqual({ scale: 1, opacity: 1 })
    expect(look(1)).toEqual({ scale: 0.97, opacity: 0.8 })
    expect(look(2)).toEqual({ scale: 0.94, opacity: 0.55 })
    expect(look(5)).toEqual(look(2))
  })
})
