// App settings with their defaults. Saving them between launches comes in #20.
export const isMac = navigator.userAgent.includes('Mac')

export type Settings = {
  /** Tauri global-shortcut accelerator, e.g. "Control+Shift+Super+S". */
  shortcut: string
  rememberRegion: boolean
  magnifier: boolean
  openAtLogin: boolean
  checkForUpdates: boolean
}

export type ToggleKey = {
  [K in keyof Settings]: Settings[K] extends boolean ? K : never
}[keyof Settings]

export const defaultSettings: Settings = {
  shortcut: isMac ? 'Control+Shift+Super+S' : 'PrintScreen',
  rememberRegion: true,
  magnifier: false,
  openAtLogin: true,
  checkForUpdates: true,
}

export const settings: Settings = $state({ ...defaultSettings })

const macSymbols: Record<string, string> = {
  Control: '⌃',
  Shift: '⇧',
  Alt: '⌥',
  Super: '⌘',
  CmdOrCtrl: '⌘',
}

/** Splits an accelerator into the key labels to show for this platform. */
export function shortcutKeys(accelerator: string): string[] {
  return accelerator
    .split('+')
    .map((k) => (isMac ? (macSymbols[k] ?? k) : k === 'Super' ? 'Win' : k))
}
