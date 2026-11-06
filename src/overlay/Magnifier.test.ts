import { render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import Magnifier from './Magnifier.svelte'

const frame = { width: 2, height: 1 }
const pixels = new Uint8ClampedArray([255, 128, 0, 255, 0, 0, 0, 255])

describe('Magnifier', () => {
  afterEach(() => vi.restoreAllMocks())

  it('zooms the pixels around the cursor and shows its coords and color', () => {
    const context = {
      drawImage: vi.fn<(...args: unknown[]) => void>(),
      fillRect: vi.fn<(...args: unknown[]) => void>(),
      strokeRect: vi.fn<(...args: unknown[]) => void>(),
      imageSmoothingEnabled: true,
    }
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
      context as unknown as CanvasRenderingContext2D,
    )
    const source = document.createElement('canvas')
    render(Magnifier, { props: { source, pixels, frame, at: { x: 0, y: 0 } } })
    expect(screen.getByTestId('magnifier')).toHaveTextContent('0, 0')
    expect(screen.getByTestId('magnifier')).toHaveTextContent('#FF8000')
    expect(context.imageSmoothingEnabled).toBe(false)
    // An 11 x 11 px area centred on the cursor, 11x bigger.
    expect(context.drawImage).toHaveBeenCalledWith(source, -5, -5, 11, 11, 0, 0, 121, 121)
  })

  it('shows no color off the frame and survives a missing context', () => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
    const source = document.createElement('canvas')
    render(Magnifier, { props: { source, pixels, frame, at: { x: 5, y: 0 } } })
    expect(screen.getByTestId('magnifier')).toHaveTextContent(/^5, 0$/)
  })
})
