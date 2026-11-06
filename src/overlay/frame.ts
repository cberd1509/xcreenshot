/** Bytes before the pixels: width and height, u32 little-endian (see `pack_frame` in Rust). */
export const HEADER_LEN = 8

/**
 * Wraps a packed frame from `overlay_frame` in an `ImageData` without copying the pixels.
 * Returns `null` when there is no frame (empty or malformed response).
 */
export function readFrame(buffer: ArrayBuffer): ImageData | null {
  if (buffer.byteLength < HEADER_LEN) return null
  const header = new DataView(buffer, 0, HEADER_LEN)
  const width = header.getUint32(0, true)
  const height = header.getUint32(4, true)
  const length = width * height * 4
  if (width === 0 || height === 0 || buffer.byteLength !== HEADER_LEN + length) return null
  return new ImageData(new Uint8ClampedArray(buffer, HEADER_LEN, length), width, height)
}
