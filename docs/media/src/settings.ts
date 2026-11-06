// The Settings window with Tauri mocked: what Rust would send on a Mac with the defaults.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { version } from '../../../package.json'

// Settings load only inside Tauri (`isTauri`), checked when the module loads: so it is imported
// after the mocks, and the replies wait for it.
Object.assign(window, { isTauri: true })
mockWindows('settings')
const replies: Promise<Record<string, unknown>> = import('$lib/settings.svelte').then(
  ({ fallbackSettings }) => {
    const shortcut = 'Control+Shift+Super+S'
    return {
      'plugin:app|version': version,
      get_prefs: { ...fallbackSettings(), shortcut, version: 1 },
      login_item_needs_approval: false,
      capture_shortcut: { accelerator: shortcut, registered: true },
    }
  },
)
mockIPC(async (cmd) => (await replies)[cmd] ?? null, { shouldMockEvents: true })
await import('../../../src/main')
