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

/** Whether ⌘ (Ctrl off the Mac) is held. */
export const modifier = (e: MouseEvent | KeyboardEvent) => (isMac ? e.metaKey : e.ctrlKey)

/** How a shortcut writes ⌘ (Ctrl+ off the Mac) and ⇧ (Shift+); `key` is ⌘ or Ctrl on its own. */
export const keyLabels = (mac = isMac) =>
  mac ? { mod: '⌘', shift: '⇧', key: '⌘' } : { mod: 'Ctrl+', shift: 'Shift+', key: 'Ctrl' }

/** Whether keys pressed in `target` type into it: a field, or editable text. */
export const typing = (target: EventTarget | null) =>
  target instanceof HTMLElement &&
  (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))

/**
 * Whether Copy (`copy`) or Save pressed in `target` is the field's own rather than the
 * capture's. A one-line field has no Save and owns only a Copy of the text selected in it (the
 * hex color); a number field (the size badge) has no text selection to read, so owns neither.
 * Text areas and editable text own both.
 */
export function fieldShortcut(target: EventTarget | null, copy: boolean): boolean {
  if (!(target instanceof HTMLInputElement)) return typing(target)
  return copy && SELECTABLE.has(target.type) && target.selectionStart !== target.selectionEnd
}

/** Input types with a text selection: reading it on the others (`number`) can throw. */
const SELECTABLE = new Set(['text', 'search', 'url', 'tel', 'password'])

/** Whether `target` is a control that Enter and Space press: a button, menu item or link. */
export const control = (target: EventTarget | null) =>
  target instanceof Element &&
  !!target.closest(
    'button, a[href], [role=button], [role=menuitem], [role=menuitemcheckbox], [role=radio], [role=switch]',
  )
