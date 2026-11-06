// App settings with their defaults. Saving them between launches comes in #20.
export type Settings = {
  shortcut: string[]
  rememberRegion: boolean
  magnifier: boolean
  openAtLogin: boolean
  checkForUpdates: boolean
}

export const defaultSettings: Settings = {
  shortcut: ['⌃', '⇧', '⌘', 'S'],
  rememberRegion: true,
  magnifier: false,
  openAtLogin: true,
  checkForUpdates: true,
}

export const settings: Settings = $state(structuredClone(defaultSettings))
