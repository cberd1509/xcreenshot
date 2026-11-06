<script lang="ts">
  import { hex, type Point, type Size } from './geometry'

  const SPAN = 11 // frame pixels across; the one under the cursor is in the middle
  const ZOOM = 11 // screen px per frame pixel
  const SIDE = SPAN * ZOOM
  const MIDDLE = Math.floor(SPAN / 2)

  /** A pixel loupe: zoomed frame around physical pixel `at`, with its coords and color. */
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
    context.drawImage(image, x - MIDDLE, y - MIDDLE, SPAN, SPAN, 0, 0, SIDE, SIDE)
    // The color of the middle pixel, read back before the outline is drawn over it.
    const middle = MIDDLE * ZOOM + Math.floor(ZOOM / 2)
    color = inside ? hex(context.getImageData(middle, middle, 1, 1).data) : null
    context.strokeStyle = 'white'
    context.strokeRect(MIDDLE * ZOOM + 0.5, MIDDLE * ZOOM + 0.5, ZOOM - 1, ZOOM - 1)
  })
</script>

<div
  class="pointer-events-none overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-md"
  data-testid="magnifier"
>
  <canvas bind:this={loupe} width={SIDE} height={SIDE} class="block"></canvas>
  <div class="flex justify-between gap-2 px-2 py-1 font-mono text-xs tabular-nums">
    <span>{x}, {y}</span>
    <span>{color ?? ''}</span>
  </div>
</div>
