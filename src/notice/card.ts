// What a notice window says, from what Rust sends (`notice.rs`). Copy from the design's
// "Update ready" and "Shortcut changed (Windows)" artboards.

/** Why the capture shortcut moved off PrintScreen. */
export type Reason = 'snippingTool' | 'inUse'

export type Notice =
  | { kind: 'updateReady'; version: string }
  | { kind: 'shortcutChanged'; reason: Reason; accelerator: string }

export type Card = {
  title: string
  text: string
  /** The shortcut to show as key caps, if any. */
  keys?: string
  /** The left (outline) button. */
  secondary: string
  /** The right button, focused so Enter clicks it. */
  primary: string
  /** Whether Esc answers with the primary button. */
  escapeIsPrimary: boolean
}

const reasons: Record<Reason, string> = {
  snippingTool:
    'PrintScreen opens Snipping Tool, so xcreenshot uses the shortcut below. You can change it in Settings.',
  inUse:
    'Another app is using PrintScreen, so xcreenshot uses the shortcut below. You can change it in Settings.',
}

export function card(notice: Notice): Card {
  if (notice.kind === 'updateReady') {
    return {
      title: 'Update ready',
      text: `xcreenshot ${notice.version} is downloaded. Restart to finish updating.`,
      secondary: 'Later',
      primary: 'Restart now',
      escapeIsPrimary: false,
    }
  }
  return {
    title: 'Shortcut changed',
    text: reasons[notice.reason],
    keys: notice.accelerator,
    secondary: 'Open Settings',
    primary: 'OK',
    escapeIsPrimary: true,
  }
}
