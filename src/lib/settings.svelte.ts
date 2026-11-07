// App settings. Rust owns them (`get_prefs` / `set_pref`) and sends `prefs:changed` to every
// window after a change, so the Settings window and the overlay stay in sync.
import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export { isMac, shortcutKeys } from './keys'

export type Settings = {
  /** Tauri global-shortcut accelerator, e.g. "Control+Shift+Super+S". Empty until loaded. */
  shortcut: string
  rememberRegion: boolean
  magnifier: boolean
  /** Copy as soon as the drag ends, with no toolbar. */
  quickCopy: boolean
  openAtLogin: boolean
  checkForUpdates: boolean
}

export type ToggleKey = {
  [K in keyof Settings]: Settings[K] extends boolean ? K : never
}[keyof Settings]

export const CHANGED = 'prefs:changed'

/** Shown until the real settings load (or if they can't). */
export function fallbackSettings(): Settings {
  return {
    shortcut: '',
    rememberRegion: true,
    magnifier: false,
    quickCopy: false,
    openAtLogin: true,
    checkForUpdates: true,
  }
}

export const settings: Settings = $state(fallbackSettings())

/** Settings as Rust sends them: numbered, higher is newer. */
type Snapshot = Settings & { version: number }

/** The version of what `settings` shows; older snapshots are ignored. */
let shown = -1

/** Shows `snapshot` unless something newer is already shown (responses and events can cross). */
function show({ version, ...values }: Snapshot) {
  if (version < shown) return
  shown = version
  Object.assign(settings, values)
}

/** Back to the fallbacks, as before loading. */
export function resetSettings() {
  shown = -1
  Object.assign(settings, fallbackSettings())
}

/** Loads the settings and follows changes made in any window, for the life of the page. */
export async function syncSettings(): Promise<void> {
  await listen<Snapshot>(CHANGED, (e) => show(e.payload))
  show(await invoke<Snapshot>('get_prefs'))
}

/** The options the overlay may change too (through its own, narrower command): in its ⋯ menu,
 * and Quick copy's Turn off. */
const CAPTURE_OPTIONS = ['rememberRegion', 'magnifier', 'quickCopy'] as const
const captureOptions = new Set<keyof Settings>(CAPTURE_OPTIONS)

/**
 * Changes one setting; `null` resets the shortcut to the default. Rejects with a message to
 * show if it can't be changed (e.g. the shortcut is taken), and then nothing changes.
 */
export async function setSetting<K extends keyof Settings>(
  key: K,
  value: K extends 'shortcut' ? string | null : Settings[K],
): Promise<void> {
  show(
    captureOptions.has(key)
      ? await invoke<Snapshot>('set_capture_option', { key, on: value })
      : await invoke<Snapshot>('set_pref', { key, value }),
  )
}

export type CaptureOption = (typeof CAPTURE_OPTIONS)[number]

/** The overlay's toggles: like `setSetting`, but a failure is only logged (the overlay keeps
 * showing the saved value). */
export function setCaptureOption(key: CaptureOption, on: boolean): void {
  setSetting(key, on).catch((e: unknown) => console.error(`saving ${key} failed`, e))
}

/** Lets go of the capture shortcut while a new one is recorded, so pressing it reaches the
 * page. Rust also takes it back if the page reloads or its window closes. */
export function pauseShortcut(): Promise<void> {
  return invoke('pause_capture_shortcut')
}

export function resumeShortcut(): Promise<void> {
  return invoke('resume_capture_shortcut')
}

// Every window that uses the settings follows them; tests call `syncSettings` themselves.
if (isTauri()) syncSettings().catch((e: unknown) => console.error('loading settings failed', e))
