/** `ArrayBuffer.prototype.transfer` (ES2024), missing from the ES2023 types. */
type Movable = ArrayBuffer & { transfer?: (length: number) => ArrayBuffer }

/**
 * Frees a buffer's bytes now instead of at the next garbage collection, which can be long after
 * the overlay closed: a frame is about 34 MB. The buffer is empty afterwards. Best effort: it
 * does nothing in a WebView without `transfer`, or if the buffer can't be detached.
 */
export function release(buffer: ArrayBuffer) {
  const movable = buffer as Movable
  try {
    movable.transfer?.(0)
  } catch {
    // The garbage collector frees it later.
  }
}
