import '@testing-library/jest-dom/vitest'
import { vi } from 'vitest'

// jsdom has no canvas, so no ImageData either.
class FakeImageData {
  constructor(
    readonly data: Uint8ClampedArray,
    readonly width: number,
    readonly height: number,
  ) {
    if (data.length !== width * height * 4) throw new RangeError('ImageData size mismatch')
  }
}
vi.stubGlobal('ImageData', FakeImageData)

// The Tauri runtime isn't available in tests.
vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn<() => Promise<string>>(() => Promise.resolve('0.1.0')),
}))

// jsdom has no canvas, so Konva gets a stand-in that records what it was asked to draw.
vi.mock('konva', async () => ({ default: (await import('./konva')).FakeKonva }))

// jsdom has no pointer events or ResizeObserver. Node-environment tests (scripts/) have no DOM.
if (typeof MouseEvent === 'function') {
  class FakePointerEvent extends MouseEvent {
    readonly pointerId: number
    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init)
      this.pointerId = init.pointerId ?? 0
    }
  }
  vi.stubGlobal('PointerEvent', FakePointerEvent)
}
vi.stubGlobal(
  'ResizeObserver',
  class {
    observe() {}
    unobserve() {}
    disconnect() {}
  },
)
