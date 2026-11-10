/**
 * How long to wait for the frame callbacks before showing the window anyway. A webview that
 * renders nothing while its window is hidden never calls them, and the overlay must still open.
 * Rust stages the window so they come; on a busy machine they can take over 100 ms, and showing
 * before them brings the flash back.
 */
export const PAINT_TIMEOUT_MS = 200

/**
 * The wait for a page's first draw since it loaded (the first capture since launch): a cold
 * webview can take this long to render its first frame. A late overlay beats a black flash.
 * Rust's own reveal waits longer still (`FIRST_REVEAL_AFTER`).
 */
export const FIRST_PAINT_TIMEOUT_MS = 600

/** How long `afterPaint` waits: longer for the page's first draw. */
export function paintTimeout(firstDraw: boolean): number {
  return firstDraw ? FIRST_PAINT_TIMEOUT_MS : PAINT_TIMEOUT_MS
}

/**
 * Resolves `true` once the DOM as it is now has been rendered: the second animation frame only
 * starts after the first one's rendering is done. Resolves `false` instead if that takes longer
 * than `timeoutMs`.
 */
export function afterPaint(timeoutMs = PAINT_TIMEOUT_MS): Promise<boolean> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), timeoutMs)
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        clearTimeout(timer)
        resolve(true)
      }),
    )
  })
}
