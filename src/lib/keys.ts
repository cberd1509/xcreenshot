// Keyboard shortcuts: showing an accelerator as keys, and turning key presses into one.
// Accelerators use the tauri-plugin-global-shortcut format, e.g. "Control+Shift+Super+S".
// Rust checks every new shortcut again before using it.

export const isMac = navigator.userAgent.includes('Mac')
export const isWindows = navigator.userAgent.includes('Windows')

const macSymbols: Record<string, string> = {
  Control: '⌃',
  Shift: '⇧',
  Alt: '⌥',
  Super: '⌘',
  CmdOrCtrl: '⌘',
}

const keyNames: Record<string, string> = {
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Minus: '-',
  Equal: '=',
  Comma: ',',
  Period: '.',
  Slash: '/',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Backquote: '`',
  BracketLeft: '[',
  BracketRight: ']',
}

/** Splits an accelerator into the key labels to show on the given platform. */
export function shortcutKeys(accelerator: string, mac = isMac): string[] {
  if (!accelerator) return []
  return accelerator.split('+').map((k) => {
    if (mac && k in macSymbols) return macSymbols[k]
    if (k === 'Super') return 'Win'
    return keyNames[k] ?? k
  })
}

/** The keys held in a key event, in accelerator order. */
export type Combo = { modifiers: string[]; key: string | null }

type KeyEventLike = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>

const modifierCodes = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/

/** The modifiers held and the main key pressed (null for a modifier on its own). */
export function comboFromEvent(e: KeyEventLike): Combo {
  const modifiers = [
    e.ctrlKey && 'Control',
    e.altKey && 'Alt',
    e.shiftKey && 'Shift',
    e.metaKey && 'Super',
  ].filter((m): m is string => !!m)
  return { modifiers, key: keyName(e.code) }
}

/** `KeyS` → `S`, `Digit1` → `1`; other codes (F5, Space, ArrowUp...) stay as they are. */
function keyName(code: string): string | null {
  if (!code || modifierCodes.test(code)) return null
  return code.replace(/^(Key|Digit)(?=.$)/, '')
}

export function toAccelerator(combo: Combo): string {
  return [...combo.modifiers, ...(combo.key ? [combo.key] : [])].join('+')
}

/** Modifiers a shortcut needs one of. Shift alone would take over typing. */
const holdKeys = new Set(['Control', 'Alt', 'Super'])

/** Why `key` with `modifiers` can't be a shortcut, or null if it can. Bare PrintScreen is fine
 * on Windows. */
export function comboProblem(modifiers: string[], key: string, windows = isWindows): string | null {
  if (modifiers.some((m) => holdKeys.has(m)) || (windows && key === 'PrintScreen')) return null
  return windows ? 'Add Ctrl, Alt or Win.' : 'Add ⌘, ⌃ or ⌥.'
}
