// The last color and size, kept in the overlay's localStorage. Overlay UI state, not a
// setting: anything missing or unreadable falls back to the defaults. Each is kept on its own,
// so an overlay window that changes one never overwrites another window's newer choice of the
// other. The tool is not kept: every capture starts in Select, so Esc closes it at once.

import { DEFAULT_STYLE, SIZES, type Style } from './shapes'

export const KEYS: Record<keyof Style, string> = {
  color: 'xcreenshot.annotate.color',
  size: 'xcreenshot.annotate.size',
}

const HEX = /^#[0-9A-F]{6}$/

/** Storage, or `null` where it is blocked. */
function storage(): Storage | null {
  try {
    return localStorage
  } catch {
    return null
  }
}

function read(key: keyof Style): string {
  try {
    return storage()?.getItem(KEYS[key]) ?? ''
  } catch {
    return ''
  }
}

/** What `remember` kept, checked; the defaults for anything missing or wrong. */
export function recall(): Style {
  const color = read('color')
  const size = Number(read('size'))
  return {
    color: HEX.test(color) ? color : DEFAULT_STYLE.color,
    size: SIZES.includes(size) ? size : DEFAULT_STYLE.size,
  }
}

/** Keeps the color or the size, leaving the other as it was. */
export function remember<K extends keyof Style>(key: K, value: Style[K]) {
  try {
    storage()?.setItem(KEYS[key], String(value))
  } catch {
    // Full or blocked: it is only a convenience.
  }
}
