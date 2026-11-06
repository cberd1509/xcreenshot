// The last tool, color and size, kept in the overlay's localStorage. Overlay UI state, not a
// setting: anything missing or unreadable falls back to the defaults.

import { DEFAULT_STYLE, READY, SIZES, type Style, type Tool } from './shapes'

export const KEY = 'xcreenshot.annotate'

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

/** What `remember` kept, checked; the defaults for anything missing or wrong. */
export function recall(): Memory {
  let saved: Partial<{ tool: unknown; color: unknown; size: unknown }> = {}
  try {
    saved = JSON.parse(storage()?.getItem(KEY) ?? '{}') ?? {}
  } catch {
    // Unreadable: the defaults.
  }
  const tool = READY.has(saved.tool as Tool) ? (saved.tool as Tool) : null
  const color =
    typeof saved.color === 'string' && HEX.test(saved.color) ? saved.color : DEFAULT_STYLE.color
  const size = SIZES.includes(saved.size as number) ? (saved.size as number) : DEFAULT_STYLE.size
  return { tool, style: { color, size } }
}

export function remember({ tool, style }: Memory) {
  try {
    storage()?.setItem(KEY, JSON.stringify({ tool, ...style }))
  } catch {
    // Full or blocked: it is only a convenience.
  }
}
