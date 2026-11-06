import { describe, expect, it } from 'vitest'
import { GRANTED_MS, MAX_BACKOFF_MS, WAITING_MS, nextDelay } from './poll'

describe('nextDelay', () => {
  it('checks every second while waiting and slower once granted', () => {
    expect(nextDelay(false, 0)).toBe(WAITING_MS)
    expect(nextDelay(true, 0)).toBe(GRANTED_MS)
  })

  it('backs off after failures, up to the maximum', () => {
    expect([1, 2, 3, 4, 5, 9].map((f) => nextDelay(false, f))).toEqual([
      1000,
      2000,
      4000,
      8000,
      MAX_BACKOFF_MS,
      MAX_BACKOFF_MS,
    ])
    expect(nextDelay(true, 1)).toBe(1000)
  })
})
