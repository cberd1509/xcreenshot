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

/** How a shortcut writes ⌘ (Ctrl+ off the Mac) and ⇧ (Shift+). */
export const keyLabels = (mac = isMac) =>
  mac ? { mod: '⌘', shift: '⇧' } : { mod: 'Ctrl+', shift: 'Shift+' }

/** Whether keys pressed in `target` type into it: a field, or editable text. */
export const typing = (target: EventTarget | null) =>
  target instanceof HTMLElement &&
  (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))

/** Whether `target` is a control that Enter and Space press: a button, menu item or link. */
export const control = (target: EventTarget | null) =>
  target instanceof Element &&
  !!target.closest(
    'button, a[href], [role=button], [role=menuitem], [role=menuitemcheckbox], [role=radio], [role=switch]',
  )
