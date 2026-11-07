/** Re-check delay while waiting for the permission. */
export const WAITING_MS = 1000
/** Slower re-check once granted, to notice if it is turned off again. */
export const GRANTED_MS = 5000
/** Longest delay after failed checks. */
export const MAX_BACKOFF_MS = 10_000
/** Failed checks in a row before the page says so. */
export const FAILURES_SHOWN = 3

/** Delay before the next check: backs off 1 s, 2 s, 4 s… up to 10 s after failures. */
export function nextDelay(granted: boolean, failures: number): number {
  if (failures > 0) return Math.min(WAITING_MS * 2 ** (failures - 1), MAX_BACKOFF_MS)
  return granted ? GRANTED_MS : WAITING_MS
}
