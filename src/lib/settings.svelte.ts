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

export function defaultSettings(mac = isMac): Settings {
  return {
    shortcut: mac ? 'Control+Shift+Super+S' : 'PrintScreen',
    rememberRegion: true,
    magnifier: false,
    openAtLogin: true,
    checkForUpdates: true,
  }
}

export const settings: Settings = $state(defaultSettings())

/** Turns an on/off setting on or off. The one place settings change, so #20 can save here. */
export function setSetting(key: ToggleKey, on: boolean) {
  settings[key] = on
}

const macSymbols: Record<string, string> = {
  Control: '⌃',
  Shift: '⇧',
  Alt: '⌥',
  Super: '⌘',
  CmdOrCtrl: '⌘',
}

/** Splits an accelerator into the key labels to show on the given platform. */
export function shortcutKeys(accelerator: string, mac = isMac): string[] {
  return accelerator.split('+').map((k) => (mac ? (macSymbols[k] ?? k) : k === 'Super' ? 'Win' : k))
}
