import { vi } from 'vitest'

type Draw = (...args: unknown[]) => void

/**
 * A fake 2D context for jsdom (which has no canvas), installed on every canvas.
 * `getImageData` returns `rgba` for every pixel of the area asked for.
 */
export function fakeContext(rgba: number[] = [7, 7, 7, 255]) {
  const context = {
    imageSmoothingEnabled: true,
    putImageData: vi.fn<(image: ImageData, x: number, y: number) => void>(),
    drawImage: vi.fn<Draw>(),
    fillRect: vi.fn<Draw>(),
    strokeRect: vi.fn<Draw>(),
    getImageData: vi.fn<
      (x: number, y: number, w?: number, h?: number) => { data: Uint8ClampedArray }
    >((_x, _y, width = 1, height = 1) => {
      const data = new Uint8ClampedArray(width * height * 4)
      for (let i = 0; i < data.length; i += 4) data.set(rgba, i)
      return { data }
    }),
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
