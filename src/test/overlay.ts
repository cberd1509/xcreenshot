// Overlay test helpers. jsdom has no canvas, so no ImageData either.
import { HEADER_LEN } from '../overlay/frame'

/** A packed frame as `overlay_frame` returns it: u32 LE width and height, then RGBA. */
export function packFrame(width: number, height: number): ArrayBuffer {
  const buffer = new ArrayBuffer(HEADER_LEN + width * height * 4)
  const view = new DataView(buffer)
  view.setUint32(0, width, true)
  view.setUint32(4, height, true)
  new Uint8Array(buffer, HEADER_LEN).forEach((_, i, a) => (a[i] = i % 256))
  return buffer
}

export class FakeImageData {
  constructor(
    readonly data: Uint8ClampedArray,
    readonly width: number,
    readonly height: number,
  ) {}
}
