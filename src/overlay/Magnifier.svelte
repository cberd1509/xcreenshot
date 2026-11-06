<script lang="ts">
  import { hex, type Point, type Size } from './geometry'

  const SIDE = 120 // the loupe's diameter, in screen px
  const ZOOM = 12 // screen px per frame pixel
  const SPAN = 11 // frame pixels across; the one under the cursor is in the middle
  const MIDDLE = Math.floor(SPAN / 2)
  // The middle pixel is centred, so the grid starts half a pixel off the edge.
  const ORIGIN = SIDE / 2 - MIDDLE * ZOOM - ZOOM / 2

  /** A round pixel loupe: zoomed frame around physical pixel `at`, with its coords and color. */
  let { source, frame, at }: { source: HTMLCanvasElement; frame: Size; at: Point } = $props()

  let loupe: HTMLCanvasElement
  let color = $state<string | null>(null)
  // Deriveds, so the loupe redraws only when the pixel under the cursor (or the frame) changes.
  const x = $derived(at.x)
  const y = $derived(at.y)
  const image = $derived(source)
  const inside = $derived(x >= 0 && y >= 0 && x < frame.width && y < frame.height)

  $effect(() => {
    const context = loupe.getContext('2d', { willReadFrequently: true })
    if (!context) return
    context.imageSmoothingEnabled = false
    context.fillStyle = 'black'
    context.fillRect(0, 0, SIDE, SIDE)
    const drawn = SPAN * ZOOM
    context.drawImage(image, x - MIDDLE, y - MIDDLE, SPAN, SPAN, ORIGIN, ORIGIN, drawn, drawn)
    // The color of the middle pixel, read back before the grid is drawn over it.
    color = inside ? hex(context.getImageData(SIDE / 2, SIDE / 2, 1, 1).data) : null
    // A faint line between pixels.
    context.fillStyle = 'rgb(0 0 0 / 0.08)'
    for (let i = 1; i < SPAN; i++) {
      const edge = ORIGIN + i * ZOOM - 1
      context.fillRect(edge, 0, 1, SIDE)
      context.fillRect(0, edge, SIDE, 1)
    }
  })
</script>

<div class="pointer-events-none flex flex-col items-center gap-1.5" data-testid="magnifier">
  <div class="loupe" style:width="{SIDE}px" style:height="{SIDE}px">
    <canvas bind:this={loupe} width={SIDE} height={SIDE} class="block"></canvas>
    <div
      class="middle"
      style:left="{SIDE / 2 - ZOOM / 2}px"
      style:top="{SIDE / 2 - ZOOM / 2}px"
      style:width="{ZOOM}px"
      style:height="{ZOOM}px"
    ></div>
  </div>
  <div
    class="rounded-md border border-zinc-800 bg-zinc-900 px-2 py-[3px] font-mono text-[11px] text-zinc-200 tabular-nums"
  >
    {x}, {y}{color ? ` · ${color}` : ''}
  </div>
</div>

<style>
  .loupe {
    position: relative;
    overflow: hidden;
    border: 2px solid white;
    border-radius: 50%;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.4);
    box-sizing: content-box;
  }

  /* The pixel under the cursor, outlined in the selection color. */
  .middle {
    position: absolute;
    box-sizing: border-box;
    border: 2px solid var(--selection);
  }
</style>
