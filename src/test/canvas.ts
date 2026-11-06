import { vi } from 'vitest'

type Draw = (...args: unknown[]) => void

/**
 * A fake 2D context for jsdom (which has no canvas), installed on every canvas.
 * `getImageData` returns `rgba` for any area.
 */
export function fakeContext(rgba: number[] = [7, 7, 7, 255]) {
  const context = {
    imageSmoothingEnabled: true,
    putImageData: vi.fn<(image: ImageData, x: number, y: number) => void>(),
    drawImage: vi.fn<Draw>(),
    fillRect: vi.fn<Draw>(),
    strokeRect: vi.fn<Draw>(),
    getImageData: vi.fn<Draw>(() => ({ data: new Uint8ClampedArray(rgba) })),
  }
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
    context as unknown as CanvasRenderingContext2D,
  )
  return context
}

/** An element's left, top, width and height in px, from its inline style. */
export function box(element: HTMLElement): number[] {
  const s = element.style
  return [s.left, s.top, s.width, s.height].map((v) => parseFloat(v))
}
