<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { onMount } from 'svelte'

  /** Sent with `overlay:show` (see `FrameInfo` in Rust). */
  type FrameInfo = { session: number; width: number; height: number }

  let canvas: HTMLCanvasElement
  // The latest session asked to draw, so a show event and the load check never draw twice
  // and an older fetch never paints over a newer one.
  let current = -1

  // Draws this display's frozen frame, then tells Rust, which shows the (hidden) window.
  async function draw(info: FrameInfo) {
    if (info.session === current) return
    current = info.session
    try {
      const data = await invoke<ArrayBuffer | Uint8Array<ArrayBuffer>>('overlay_frame', {
        session: info.session,
      })
      if (current !== info.session) return
      // Raw RGBA, no copy. Tauri returns an ArrayBuffer; accept a view too.
      const pixels =
        data instanceof ArrayBuffer
          ? new Uint8ClampedArray(data)
          : new Uint8ClampedArray(data.buffer, data.byteOffset, data.byteLength)
      // Empty: the session closed while this window was loading.
      if (pixels.length === 0) return
      canvas.width = info.width
      canvas.height = info.height
      // Opaque: the frame covers the canvas, so skip alpha blending.
      const context = canvas.getContext('2d', { alpha: false })!
      context.putImageData(new ImageData(pixels, info.width, info.height), 0, 0)
      await invoke('overlay_painted', { session: info.session })
    } catch (e) {
      // Never leave the user on a dead overlay.
      console.error('overlay could not draw its frame', e)
      void invoke('close_overlay')
    }
  }

  // Frees the frame's bitmap once the window is hidden after a capture. The next frame is
  // drawn before the window shows again, so nothing stale or empty is ever seen.
  function onvisibilitychange() {
    if (!document.hidden) return
    canvas.width = 0
    canvas.height = 0
  }

  function onkeydown(e: KeyboardEvent) {
    // Rust owns Esc while the overlay is open; this is the fallback.
    if (e.key === 'Escape') void invoke('close_overlay')
  }

  onMount(() => {
    const unlisten = getCurrentWebviewWindow().listen<FrameInfo>('overlay:show', (e) => {
      void draw(e.payload)
    })
    // Asked only once the listener is registered, so a frame sent while this window was
    // loading is never missed.
    void unlisten
      .then(() => invoke<FrameInfo | null>('overlay_ready'))
      .then((info) => info && draw(info))
    return () => void unlisten.then((stop) => stop())
  })
</script>

<svelte:window {onkeydown} />
<svelte:document {onvisibilitychange} />

<canvas bind:this={canvas} width="0" height="0"></canvas>
<div class="dim"></div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    background: black;
    cursor: crosshair;
    user-select: none;
  }

  canvas,
  .dim {
    position: fixed;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .dim {
    background: rgb(0 0 0 / 0.35);
    pointer-events: none;
  }
</style>
