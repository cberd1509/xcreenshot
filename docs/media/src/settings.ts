// The Settings window with Tauri mocked: what Rust would send on a Mac with the defaults.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks'

const prefs = {
  shortcut: 'Control+Shift+Super+S',
  rememberRegion: true,
  magnifier: false,
  openAtLogin: true,
  checkForUpdates: true,
  version: 1,
}

const replies: Record<string, unknown> = {
  'plugin:app|version': '0.2.0',
  get_prefs: prefs,
  login_item_needs_approval: false,
  capture_shortcut: { accelerator: prefs.shortcut, registered: true },
}

// Settings load only inside Tauri (`isTauri`).
Object.assign(window, { isTauri: true })
mockWindows('settings')
mockIPC((cmd) => replies[cmd] ?? null, { shouldMockEvents: true })
await import('../../../src/main')
