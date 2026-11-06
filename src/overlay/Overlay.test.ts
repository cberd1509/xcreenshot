import { render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { invoke } from '@tauri-apps/api/core'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Overlay from './Overlay.svelte'
import { FakeImageData, packFrame } from '../test/overlay'

type Handler = () => void
const handlers = new Map<string, Handler>()
const unlisten = vi.fn<() => void>()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}))
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({
    listen: (event: string, handler: Handler) => {
      handlers.set(event, handler)
      return Promise.resolve(unlisten)
    },
  }),
}))

const putImageData = vi.fn<(image: ImageData, x: number, y: number) => void>()
let frame: ArrayBuffer

function commands(name: string) {
  return vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === name)
}

describe('Overlay', () => {
  beforeEach(() => {
    handlers.clear()
    frame = new ArrayBuffer(0)
    vi.mocked(invoke).mockImplementation((cmd) =>
      Promise.resolve(cmd === 'overlay_frame' ? frame : undefined),
    )
    vi.stubGlobal('ImageData', FakeImageData)
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
      putImageData,
    } as unknown as CanvasRenderingContext2D)
  })

  afterEach(() => {
    vi.clearAllMocks()
    vi.unstubAllGlobals()
  })

  it('draws the frame on show, dims it and reports the paint', async () => {
    const { container } = render(Overlay)
    // Nothing captured yet: the window stays transparent.
    await waitFor(() => expect(commands('overlay_frame')).toHaveLength(1))
    expect(screen.queryByTestId('dim')).not.toBeInTheDocument()

    frame = packFrame(4, 2)
    handlers.get('overlay:show')!()
    expect(await screen.findByTestId('dim')).toBeInTheDocument()
    const canvas = container.querySelector('canvas')!
    expect([canvas.width, canvas.height]).toEqual([4, 2])
    const [image, x, y] = putImageData.mock.calls[0]
    expect([image.width, image.height, x, y]).toEqual([4, 2, 0, 0])
    await waitFor(() => expect(commands('overlay_painted')).toHaveLength(1))
    expect(commands('overlay_painted')[0][1]).toEqual({
      fetchMs: expect.any(Number),
      drawMs: expect.any(Number),
    })
  })

  it('draws on load when the frame is already there', async () => {
    frame = packFrame(2, 2)
    render(Overlay)
    expect(await screen.findByTestId('dim')).toBeInTheDocument()
    expect(putImageData).toHaveBeenCalledOnce()
  })

  it('clears the frame on hide', async () => {
    frame = packFrame(2, 2)
    const { container } = render(Overlay)
    await screen.findByTestId('dim')
    handlers.get('overlay:hide')!()
    await waitFor(() => expect(screen.queryByTestId('dim')).not.toBeInTheDocument())
    expect(container.querySelector('canvas')!.width).toBe(0)
  })

  it('does not draw a frame that arrives after hide', async () => {
    render(Overlay)
    await waitFor(() => expect(commands('overlay_frame')).toHaveLength(1))
    let respond!: (frame: ArrayBuffer) => void
    vi.mocked(invoke).mockImplementationOnce(() => new Promise((r) => (respond = r)))
    handlers.get('overlay:show')!()
    handlers.get('overlay:hide')!()
    respond(packFrame(2, 2))
    await Promise.resolve()
    await Promise.resolve()
    expect(putImageData).not.toHaveBeenCalled()
    expect(screen.queryByTestId('dim')).not.toBeInTheDocument()
  })

  it('closes on Esc only', async () => {
    render(Overlay)
    await userEvent.keyboard('a')
    expect(commands('close_overlay')).toHaveLength(0)
    await userEvent.keyboard('{Escape}')
    expect(commands('close_overlay')).toHaveLength(1)
  })

  it('stops listening when unmounted', async () => {
    const { unmount } = render(Overlay)
    unmount()
    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(2))
  })
})
