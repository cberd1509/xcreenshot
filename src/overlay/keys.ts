import { isMac } from '$lib/settings.svelte'

/**
 * ⌘ on macOS, Ctrl elsewhere, plus `key` (a Latin letter), with Shift only if `shift`, and no
 * other modifier. On a non-Latin layout the physical key counts, as in other apps.
 */
export function shortcut(e: KeyboardEvent, key: string, shift = false): boolean {
  const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
  const latin = /^[a-z]$/i.test(e.key)
  const pressed = latin ? e.key.toLowerCase() === key : e.code === `Key${key.toUpperCase()}`
  return mod && !e.altKey && e.shiftKey === shift && pressed
}
