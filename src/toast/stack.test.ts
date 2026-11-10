import { describe, expect, it } from 'vitest'
import {
  FADE,
  SAVED_SHOWN,
  SHOWN,
  TITLES,
  clickable,
  detail,
  dismiss,
  hover,
  leave,
  look,
  next,
  prune,
  push,
  reveal,
  up,
  type ToastEvent,
} from './stack'

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
      seq: 4,
      kind: 'saved',
      detail: 'shot.png',
      count: 1,
      fades: SAVED_SHOWN,
    })
    expect(TITLES).toEqual({ copied: 'Copied', saved: 'Saved', shared: 'Shared' })
  })

  it('puts a new toast at the bottom and keeps three', () => {
    let stack = push([], saved(1), 0)
    stack = push(stack, shared(2), 10)
    stack = push(stack, copied(3), 20)
    expect(stack.map((t) => t.kind)).toEqual(['saved', 'shared', 'copied'])
    // Saved again once its toast is fading: a new one, and the oldest goes.
    stack = push(stack, saved(4), SAVED_SHOWN)
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

  it('keeps the order of the actions when an older toast arrives late', () => {
    // The page got 3 and 4 before the ones it missed while loading.
    let stack = push(push([], saved(3), 0), copied(4, 640), 0)
    stack = push(stack, shared(2), 0)
    expect(stack.map((t) => t.kind)).toEqual(['shared', 'saved', 'copied'])
    // An older copy only counts: the newer size stays.
    stack = push(stack, copied(1), 0)
    expect(stack[2]).toMatchObject({ seq: 4, detail: '640 × 1064', count: 2 })
    // Older than all three: it goes past the top.
    const late = push(stack, { seq: 0, kind: 'saved', file: 'old.png' }, SAVED_SHOWN)
    expect(late.map((t) => t.seq)).toEqual([2, 3, 4])
  })

  it('does not count on a toast already fading', () => {
    const stack = push(push([], copied(1), 0), copied(2), SHOWN)
    expect(stack.map((t) => [t.id, t.count])).toEqual([
      [1, 1],
      [2, 1],
    ])
  })

  it('fades each toast after it shows, then drops it; Saved stays longer', () => {
    const stack = push(push([], copied(1), 0), saved(2), 500)
    expect(up(stack[0], SHOWN - 1)).toBe(true)
    expect(up(stack[0], SHOWN)).toBe(false)
    expect(prune(stack, SHOWN + FADE - 1)).toHaveLength(2)
    expect(prune(stack, SHOWN + FADE)).toEqual([stack[1]])
    expect(up(stack[1], 500 + SAVED_SHOWN - 1)).toBe(true)
    expect(prune(stack, 500 + SAVED_SHOWN + FADE)).toEqual([])
  })

  it('wakes up when a toast starts or ends its fade', () => {
    const stack = push(push([], copied(1), 0), saved(2), 500)
    expect(next(stack, 0)).toBe(SHOWN)
    expect(next(stack, SHOWN)).toBe(SHOWN + FADE)
    expect(next(stack, SHOWN + FADE)).toBe(500 + SAVED_SHOWN)
    expect(next(stack, 500 + SAVED_SHOWN + FADE)).toBe(null)
    expect(next([], 0)).toBe(null)
  })

  it('keeps a Saved toast up while the mouse is on it, then its full time again', () => {
    let stack = push(push([], saved(1), 0), copied(2), 0)
    stack = hover(stack, 1, 3000)
    expect(up(stack[0], 1e9)).toBe(true)
    // Only the copy is waited for; the hovered toast waits for the mouse.
    expect(next(stack, 0)).toBe(SHOWN)
    expect(next(stack, SHOWN + FADE)).toBe(null)
    expect(prune(stack, 1e9)).toEqual([stack[0]])
    // A repeat while hovered stays up.
    expect(push(stack, saved(3), 5000)[1]).toMatchObject({ count: 2, fades: Infinity })
    stack = leave(stack, 1, 9000)
    expect(stack[0].fades).toBe(9000 + SAVED_SHOWN)
  })

  it('does not bring back a toast already fading', () => {
    const stack = push([], saved(1), 0)
    expect(hover(stack, 1, SAVED_SHOWN)).toEqual(stack)
    expect(leave(stack, 1, SAVED_SHOWN)).toEqual(stack)
    expect(hover(stack, 7, 0)).toEqual(stack)
  })

  it('fades a toast at once when acted on', () => {
    const stack = dismiss(push(push([], saved(1), 0), copied(2), 0), 1, 100)
    expect(up(stack[0], 100)).toBe(false)
    expect(up(stack[1], 100)).toBe(true)
    expect(next(stack, 100)).toBe(100 + FADE)
  })

  it('takes clicks only while a Saved toast is up', () => {
    expect(clickable([], 0)).toBe(false)
    expect(clickable(push([], copied(1), 0), 0)).toBe(false)
    const stack = push(push([], copied(1), 0), saved(2), 0)
    expect(clickable(stack, SAVED_SHOWN - 1)).toBe(true)
    expect(clickable(stack, SAVED_SHOWN)).toBe(false)
    expect(clickable(dismiss(stack, 2, 10), 10)).toBe(false)
  })

  it('names the reveal button for the platform', () => {
    expect(reveal(true)).toBe('Show in Finder')
    expect(reveal(false)).toBe('Show in folder')
  })

  it('shows older toasts smaller and dimmer', () => {
    expect(look(0)).toEqual({ scale: 1, opacity: 1 })
    expect(look(1)).toEqual({ scale: 0.97, opacity: 0.8 })
    expect(look(2)).toEqual({ scale: 0.94, opacity: 0.55 })
    expect(look(5)).toEqual(look(2))
  })
})
