/**
 * Holds `requestAnimationFrame` callbacks until `next()` runs them, as the browser does once
 * per frame. `restore()` puts the real one back.
 */
export function fakeFrames() {
  const real = globalThis.requestAnimationFrame
  let waiting: FrameRequestCallback[] = []
  globalThis.requestAnimationFrame = (callback) => waiting.push(callback)
  return {
    /** How many callbacks wait for the next frame. */
    pending: () => waiting.length,
    next() {
      const now = waiting
      waiting = []
      now.forEach((callback) => callback(0))
    },
    restore() {
      globalThis.requestAnimationFrame = real
    },
  }
}
