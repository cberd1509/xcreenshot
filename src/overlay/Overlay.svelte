<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { onMount } from 'svelte'
  import { readFrame } from './frame'

  let canvas: HTMLCanvasElement
  let painted = $state(false)
  // Bumped on hide, so a fetch that finishes after Esc does not draw the old frame.
  let generation = 0

  // Fetches this display's frozen frame as raw RGBA and draws it at native resolution.
  async function draw() {
    const start = performance.now()
    const drawing = generation
    const frame = readFrame(await invoke<ArrayBuffer>('overlay_frame'))
    const fetched = performance.now()
    if (!frame || drawing !== generation) return
    canvas.width = frame.width
    canvas.height = frame.height
    // Opaque: the frame covers the canvas, so skip alpha blending.
    canvas.getContext('2d', { alpha: false })?.putImageData(frame, 0, 0)
    painted = true
    requestAnimationFrame(() => {
      void invoke('overlay_painted', {
        fetchMs: fetched - start,
        drawMs: performance.now() - fetched,
      })
    })
  }

  // Frees the frame, so the next show starts transparent instead of flashing the old one.
  function clear() {
    generation++
    painted = false
    canvas.width = 0
    canvas.height = 0
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') void invoke('close_overlay')
  }

  onMount(() => {
    const win = getCurrentWebviewWindow()
    const unlisten = [win.listen('overlay:show', draw), win.listen('overlay:hide', clear)]
    // A window created during a capture loads after its frame is ready.
    void draw()
    return () => unlisten.forEach((u) => void u.then((stop) => stop()))
  })
</script>

<svelte:window {onkeydown} />

<!-- Empty until the first frame: an opaque canvas would show black. -->
<canvas bind:this={canvas} width="0" height="0"></canvas>
{#if painted}<div class="dim" data-testid="dim"></div>{/if}

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    background: transparent;
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
