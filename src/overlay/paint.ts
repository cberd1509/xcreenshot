/**
 * How long to wait for the frame callbacks before showing the window anyway. A webview that
 * renders nothing while its window is hidden never calls them, and the overlay must still open.
 */
export const PAINT_TIMEOUT_MS = 100

/**
 * Resolves `true` once the DOM as it is now has been rendered: the second animation frame only
 * starts after the first one's rendering is done. Resolves `false` instead if that takes longer
 * than `PAINT_TIMEOUT_MS`.
 */
export function afterPaint(): Promise<boolean> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), PAINT_TIMEOUT_MS)
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        clearTimeout(timer)
        resolve(true)
      }),
    )
  })
}
