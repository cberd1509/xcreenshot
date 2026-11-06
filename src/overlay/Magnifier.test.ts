import { render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeContext } from '../test/canvas'
import Magnifier from './Magnifier.svelte'

const frame = { width: 2, height: 1 }

describe('Magnifier', () => {
  afterEach(() => vi.restoreAllMocks())

  it('zooms the pixels around the cursor and shows its coords and color', () => {
    const context = fakeContext([255, 128, 0, 255])
    const source = document.createElement('canvas')
    render(Magnifier, { props: { source, frame, at: { x: 0, y: 0 } } })
    expect(screen.getByTestId('magnifier')).toHaveTextContent('0, 0 · #FF8000')
    expect(context.imageSmoothingEnabled).toBe(false)
    // An 11 x 11 px area centred on the cursor, 12x bigger, its middle pixel in the middle.
    expect(context.drawImage).toHaveBeenCalledWith(source, -5, -5, 11, 11, -6, -6, 132, 132)
    // The color is the middle of the zoomed middle pixel.
    expect(context.getImageData).toHaveBeenCalledWith(60, 60, 1, 1)
    // A grid between the pixels: 10 lines each way, after the background.
    expect(context.fillRect).toHaveBeenCalledTimes(21)
    expect(context.fillRect).toHaveBeenCalledWith(5, 0, 1, 120)
  })

  it('redraws only when the pixel changes', async () => {
    const context = fakeContext()
    const source = document.createElement('canvas')
    const { rerender } = render(Magnifier, { props: { source, frame, at: { x: 0, y: 0 } } })
    await rerender({ at: { x: 0, y: 0 } })
    expect(context.drawImage).toHaveBeenCalledTimes(1)
    await rerender({ at: { x: 1, y: 0 } })
    expect(context.drawImage).toHaveBeenCalledTimes(2)
  })

  it('shows no color off the frame and survives a missing context', () => {
    const source = document.createElement('canvas')
    fakeContext()
    render(Magnifier, { props: { source, frame, at: { x: 5, y: 0 } } })
    expect(screen.getByTestId('magnifier')).toHaveTextContent(/^5, 0$/)
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
    render(Magnifier, { props: { source, frame, at: { x: 0, y: 0 } } })
  })
})
