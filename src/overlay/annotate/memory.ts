// The last tool, color and size, kept in the overlay's localStorage. Overlay UI state, not a
// setting: anything missing or unreadable falls back to the defaults. Each is kept on its own,
// so an overlay window that changes one never overwrites another window's newer choice of the
// others.

import { DEFAULT_STYLE, READY, SIZES, type Style, type Tool } from './shapes'

export type Remembered = { tool: Tool; color: string; size: number }

export const KEYS: Record<keyof Remembered, string> = {
  tool: 'xcreenshot.annotate.tool',
  color: 'xcreenshot.annotate.color',
  size: 'xcreenshot.annotate.size',
}

export type Memory = { tool: Tool | null; style: Style }

const HEX = /^#[0-9A-F]{6}$/

/** Storage, or `null` where it is blocked. */
function storage(): Storage | null {
  try {
    return localStorage
  } catch {
    return null
  }
}

function read(key: keyof Remembered): string {
  try {
    return storage()?.getItem(KEYS[key]) ?? ''
  } catch {
    return ''
  }
}

/** What `remember` kept, checked; the defaults for anything missing or wrong. */
export function recall(): Memory {
  const tool = READY.has(read('tool') as Tool) ? (read('tool') as Tool) : null
  const color = HEX.test(read('color')) ? read('color') : DEFAULT_STYLE.color
  const size = SIZES.includes(Number(read('size'))) ? Number(read('size')) : DEFAULT_STYLE.size
  return { tool, style: { color, size } }
}

/** Keeps one of the tool, color and size, leaving the others as they were. */
export function remember<K extends keyof Remembered>(key: K, value: Remembered[K]) {
  try {
    storage()?.setItem(KEYS[key], String(value))
  } catch {
    // Full or blocked: it is only a convenience.
  }
}
