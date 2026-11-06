<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { onMount } from 'svelte'
  import { settings } from '$lib/settings.svelte'
  import {
    CURSORS,
    badgePosition,
    contains,
    fromPoints,
    isEmpty,
    loupePosition,
    move,
    nudge,
    resize,
    resizeTo,
    toPhysical,
    type Handle,
    type Point,
    type Rect,
    type Size,
  } from './geometry'
  import Magnifier from './Magnifier.svelte'
  import Selection from './Selection.svelte'
  import SizeBadge from './SizeBadge.svelte'

  /** Sent with `overlay:show` (see `FrameInfo` in Rust). */
  type FrameInfo = { session: number; width: number; height: number }

  type Drag =
    | { kind: 'draw'; start: Point }
    | { kind: 'move'; start: Point; origin: Rect }
    | { kind: 'resize'; start: Point; origin: Rect; handle: Handle }

  /** Show the pixel loupe while dragging. Defaults to the magnifier setting. */
  let { magnifier }: { magnifier?: boolean } = $props()
  const loupeOn = $derived(magnifier ?? settings.magnifier)

  let canvas: HTMLCanvasElement = $state()!
  // The latest session asked to draw, so a show event and the load check never draw twice
  // and an older fetch never paints over a newer one.
  let current = -1
  // This display's frame: its size in physical px and its RGBA pixels (for the loupe color).
  let frame = $state<Size>({ width: 0, height: 0 })
  let pixels = $state.raw<Uint8ClampedArray | null>(null)
  let innerWidth = $state(0)
  let innerHeight = $state(0)
  const display = $derived({ width: innerWidth, height: innerHeight })
  const scale = $derived({ x: frame.width / innerWidth, y: frame.height / innerHeight })

  /** The selection in logical px, relative to this display. */
  let selection = $state<Rect | null>(null)
  let drag = $state<Drag | null>(null)
  let pointer = $state<Point>({ x: 0, y: 0 })
  let badge = $state<Size>({ width: 0, height: 0 })
  const loupeSize = { width: 121, height: 145 }

  const physical = $derived(selection && toPhysical(selection, scale, frame))
  const cursor = $derived(
    drag?.kind === 'move' ? CURSORS.move : drag?.kind === 'resize' ? CURSORS[drag.handle] : null,
  )
  const loupeAt = $derived({
    x: Math.min(Math.floor(pointer.x * scale.x), frame.width - 1),
    y: Math.min(Math.floor(pointer.y * scale.y), frame.height - 1),
  })

  // Draws this display's frozen frame, then tells Rust, which shows the (hidden) window.
  async function draw(info: FrameInfo) {
    if (info.session === current) return
    current = info.session
    selection = null
    try {
      const data = await invoke<ArrayBuffer | Uint8Array<ArrayBuffer>>('overlay_frame', {
        session: info.session,
      })
      if (current !== info.session) return
      // Raw RGBA. Tauri returns an ArrayBuffer; accept a view too.
      const bytes =
        data instanceof ArrayBuffer
          ? new Uint8ClampedArray(data)
          : new Uint8ClampedArray(data.buffer, data.byteOffset, data.byteLength)
      // Empty: the session closed while this window was loading.
      if (bytes.length === 0) return
      canvas.width = info.width
      canvas.height = info.height
      // Opaque: the frame covers the canvas, so skip alpha blending.
      const context = canvas.getContext('2d', { alpha: false })!
      context.putImageData(new ImageData(bytes, info.width, info.height), 0, 0)
      frame = { width: info.width, height: info.height }
      pixels = bytes
      await invoke('overlay_painted', { session: info.session })
    } catch (e) {
      // Never leave the user on a dead overlay.
      console.error('overlay could not draw its frame', e)
      void invoke('close_overlay')
    }
  }

  /** Sets the selection and tells Rust when this display gains or loses one (for Esc). */
  function select(rect: Rect | null) {
    const active = rect !== null
    if (active !== (selection !== null)) {
      void invoke('overlay_selection', { session: current, active })
    }
    selection = rect
  }

  function confirm(rect: Rect) {
    invoke('overlay_confirm', { session: current, rect }).catch((e) =>
      console.error('could not confirm the selection', e),
    )
  }

  // Frees the frame once the window is hidden after a capture. The next frame is drawn
  // before the window shows again, so nothing stale or empty is ever seen.
  function onvisibilitychange() {
    if (!document.hidden) return
    canvas.width = 0
    canvas.height = 0
    pixels = null
    selection = null
    drag = null
  }

  const at = (e: PointerEvent): Point => ({ x: e.clientX, y: e.clientY })

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return
    const start = at(e)
    pointer = start
    const handle = (e.target as Element).closest<HTMLElement>('[data-handle]')?.dataset.handle
    if (selection && handle) {
      drag = { kind: 'resize', start, origin: selection, handle: handle as Handle }
    } else if (selection && contains(selection, start)) {
      drag = { kind: 'move', start, origin: selection }
    } else {
      drag = { kind: 'draw', start }
      select(null)
    }
    ;(e.currentTarget as Element).setPointerCapture?.(e.pointerId)
  }

  function onpointermove(e: PointerEvent) {
    pointer = at(e)
    if (!drag) return
    const dx = pointer.x - drag.start.x
    const dy = pointer.y - drag.start.y
    if (drag.kind === 'draw') {
      const rect = fromPoints(drag.start, pointer, display)
      select(isEmpty(rect) ? null : rect)
    } else if (drag.kind === 'move') {
      select(move(drag.origin, dx, dy, display))
    } else {
      const rect = resize(drag.origin, drag.handle, dx, dy, display)
      select(isEmpty(rect) ? null : rect)
    }
  }

  function onpointerup() {
    drag = null
  }

  function onkeydown(e: KeyboardEvent) {
    // Typing in the size badge.
    if (e.target instanceof HTMLInputElement) return
    if (e.key === 'Escape') {
      // Rust owns Esc while the overlay is open (`overlay:clear`); this is the fallback.
      if (selection) select(null)
      else void invoke('close_overlay')
    } else if (e.key === 'Enter' && selection) {
      e.preventDefault()
      confirm(selection)
    } else if (selection) {
      const moved = nudge(selection, e.key, e.shiftKey ? 10 : 1, display)
      if (moved) {
        e.preventDefault()
        select(moved)
      }
    }
  }

  function onresize(size: Size, keepRatio: boolean) {
    if (selection) select(resizeTo(selection, size, scale, frame, keepRatio))
  }

  onMount(() => {
    const window = getCurrentWebviewWindow()
    const listeners = Promise.all([
      window.listen<FrameInfo>('overlay:show', (e) => void draw(e.payload)),
      // Esc with a selection, or a selection started on another display.
      window.listen('overlay:clear', () => (selection = null)),
    ])
    // Asked only once the listeners are registered, so a frame sent while this window was
    // loading is never missed.
    void listeners
      .then(() => invoke<FrameInfo | null>('overlay_ready'))
      .then((info) => info && draw(info))
    return () => void listeners.then((stops) => stops.forEach((stop) => stop()))
  })
</script>

<svelte:window {onkeydown} bind:innerWidth bind:innerHeight />
<svelte:document {onvisibilitychange} />

<div
  class="root"
  class:dragging={cursor}
  style:cursor
  data-testid="overlay"
  role="presentation"
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
>
  <canvas bind:this={canvas} width="0" height="0"></canvas>
  {#if selection && physical}
    <Selection rect={selection} />
    {@const p = badgePosition(selection, badge, display)}
    <div
      class="floating"
      style:left="{p.x}px"
      style:top="{p.y}px"
      bind:offsetWidth={badge.width}
      bind:offsetHeight={badge.height}
      onpointerdown={(e) => e.stopPropagation()}
      role="presentation"
    >
      <SizeBadge size={physical} {onresize} />
    </div>
  {:else}
    <div class="dim"></div>
  {/if}
  {#if loupeOn && drag && drag.kind !== 'move' && pixels}
    {@const p = loupePosition(pointer, loupeSize, display)}
    <div class="floating" style:left="{p.x}px" style:top="{p.y}px">
      <Magnifier source={canvas} {pixels} {frame} at={loupeAt} />
    </div>
  {/if}
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    background: black;
    user-select: none;
  }

  .root,
  canvas,
  .dim {
    position: fixed;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .root {
    cursor: crosshair;
  }

  /* While dragging, the drag's cursor wins over whatever is under the pointer. */
  .dragging :global(*) {
    cursor: inherit !important;
  }

  .dim {
    background: rgb(0 0 0 / 0.35);
    pointer-events: none;
  }

  .floating {
    position: fixed;
    cursor: default;
  }
</style>
