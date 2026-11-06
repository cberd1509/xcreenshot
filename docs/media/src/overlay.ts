// The capture overlay with Tauri mocked, driven by scripts/readme-media.ts through `window.demo`.
// The frozen frame is /frame.png (the rendered desktop.html), served by the script. A headless
// browser draws no pointer and shows no keys, so this page draws both.
import { emit } from '@tauri-apps/api/event'
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { shortcutKeys } from '$lib/keys'

const FRAME = '/frame.png'
// One display, the size of the page, at its scale.
const info = {
  session: 1,
  width: innerWidth * devicePixelRatio,
  height: innerHeight * devicePixelRatio,
  logicalWidth: innerWidth,
  logicalHeight: innerHeight,
  escapeShortcut: true,
  region: null,
}

// Decoded once: shown behind the overlay, and sent as its frame.
const image = new Image()
image.src = FRAME
const decoded = image.decode()

/** The frame as raw RGBA, as Rust sends it. */
async function pixels(): Promise<ArrayBuffer> {
  await decoded
  const canvas = new OffscreenCanvas(info.width, info.height)
  const context = canvas.getContext('2d')!
  context.drawImage(image, 0, 0, info.width, info.height)
  return context.getImageData(0, 0, info.width, info.height).data.buffer
}

let open: (value: typeof info) => void
const opened = new Promise<typeof info>((resolve) => (open = resolve))
let copy: () => void
const copied = new Promise<void>((resolve) => (copy = resolve))

mockWindows('overlay')
mockIPC(
  (cmd) => {
    if (cmd === 'overlay_ready') return opened
    if (cmd === 'overlay_frame') return pixels()
    if (cmd === 'overlay_copy') {
      copy()
      // Rust closes the overlay once the image is on the clipboard.
      setTimeout(() => void emit('overlay:closed'), 150)
    }
    return null
  },
  { shouldMockEvents: true },
)

// Behind the overlay: the desktop, seen before it opens and after it closes.
const style = document.createElement('style')
style.textContent = `
  html, body { background: url(${FRAME}) 0 0 / 100vw 100vh no-repeat !important; }
  #demo-cursor { position: fixed; left: 0; top: 0; z-index: 2147483647; pointer-events: none; }
  #demo-keys {
    position: fixed; left: 50%; top: 50%; z-index: 2147483646; pointer-events: none;
    transform: translate(-50%, -50%); display: flex; gap: 8px; padding: 14px 18px;
    border-radius: 16px; background: rgb(24 24 27 / 0.85); transition: opacity 0.2s;
    font: 600 26px system-ui, sans-serif; color: #fafafa;
  }
  #demo-keys kbd {
    min-width: 44px; padding: 6px 10px; border-radius: 8px; text-align: center;
    background: #3f3f46; box-shadow: inset 0 -2px 0 #18181b; font: inherit;
  }
`
document.head.append(style)

// Cursor images with their hot spots, by the CSS cursor under the pointer.
const ARROW = {
  x: 4,
  y: 2,
  svg: `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="28" viewBox="0 0 24 28"><path d="M4 2v19l4.6-4.4 3 6.9 3-1.3-3-6.8H18z" fill="#000" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/></svg>`,
}
const CROSS = {
  x: 12,
  y: 12,
  svg: `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M12 1v22M1 12h22" stroke="#fff" stroke-width="3.5"/><path d="M12 1v22M1 12h22" stroke="#000" stroke-width="1.5"/></svg>`,
}
const cursor = document.createElement('img')
cursor.id = 'demo-cursor'
cursor.alt = ''
document.body.append(cursor)
let shape: typeof ARROW | null = null

function pointAt(x: number, y: number) {
  const under = document.elementFromPoint(x, y)
  const next = under && getComputedStyle(under).cursor === 'crosshair' ? CROSS : ARROW
  if (next !== shape) {
    shape = next
    cursor.src = `data:image/svg+xml,${encodeURIComponent(next.svg)}`
  }
  cursor.style.transform = `translate(${x - next.x}px, ${y - next.y}px)`
}
window.addEventListener('pointermove', (e) => pointAt(e.clientX, e.clientY), true)
pointAt(innerWidth / 2, innerHeight / 2)

const keys = document.createElement('div')
keys.id = 'demo-keys'
keys.style.opacity = '0'
document.body.append(keys)

Object.assign(window, {
  demo: {
    /** Opens the overlay, as the shortcut would. */
    open: () => open(info),
    /** Resolves once a copy is asked for. */
    copied: () => copied,
    /** Shows the pointer or hides it. */
    cursor: (shown: boolean) => (cursor.style.display = shown ? '' : 'none'),
    /** Shows an accelerator being pressed, e.g. "Super+C" (none: hides it). */
    keys: (accelerator = '') => {
      const labels = shortcutKeys(accelerator, true)
      if (labels.length)
        keys.replaceChildren(
          ...labels.map((label) => Object.assign(document.createElement('kbd'), { textContent: label })),
        )
      keys.style.opacity = labels.length ? '1' : '0'
    },
  },
})

await decoded
await import('../../../src/overlay/main')
