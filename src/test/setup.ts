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
