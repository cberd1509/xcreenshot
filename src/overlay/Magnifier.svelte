<script lang="ts">
  import { hexAt, type Point, type Size } from './geometry'

  const RADIUS = 5 // pixels on each side of the one under the cursor
  const ZOOM = 11
  const SIDE = (RADIUS * 2 + 1) * ZOOM

  /** A pixel loupe: zoomed frame around physical pixel `at`, with its coords and color. */
  let {
    source,
    pixels,
    frame,
    at,
  }: { source: HTMLCanvasElement; pixels: Uint8ClampedArray; frame: Size; at: Point } = $props()

  let loupe: HTMLCanvasElement
  const color = $derived(hexAt(pixels, frame, at.x, at.y))

  $effect(() => {
    const context = loupe.getContext('2d')
    if (!context) return
    context.imageSmoothingEnabled = false
    context.fillStyle = 'black'
    context.fillRect(0, 0, SIDE, SIDE)
    const side = RADIUS * 2 + 1
    context.drawImage(source, at.x - RADIUS, at.y - RADIUS, side, side, 0, 0, SIDE, SIDE)
    context.strokeStyle = 'white'
    context.strokeRect(RADIUS * ZOOM + 0.5, RADIUS * ZOOM + 0.5, ZOOM - 1, ZOOM - 1)
  })
</script>

<div
  class="pointer-events-none overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-md"
  data-testid="magnifier"
>
  <canvas bind:this={loupe} width={SIDE} height={SIDE} class="block"></canvas>
  <div class="flex justify-between gap-2 px-2 py-1 font-mono text-xs tabular-nums">
    <span>{at.x}, {at.y}</span>
    <span>{color ?? ''}</span>
  </div>
</div>
