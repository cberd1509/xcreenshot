<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { emit, listen } from '@tauri-apps/api/event'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { onMount } from 'svelte'
  import { isMac, setSetting, settings } from '$lib/settings.svelte'
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
  import Annotator from './annotate/Annotator.svelte'
  import Magnifier from './Magnifier.svelte'
  import Selection from './Selection.svelte'
  import SizeBadge from './SizeBadge.svelte'

  /** Sent with `overlay:show` (see `FrameInfo` in Rust). */
  type FrameInfo = {
    session: number
    width: number
    height: number
    logicalWidth: number
    logicalHeight: number
    /** Whether Rust owns Esc. If not, the page forwards its own Esc key to Rust. */
    escapeShortcut: boolean
  }
  /** Sent with `overlay:escape`; answered with the same token. */
  type EscapeEvent = { session: number; token: number }

  /** Broadcast to every overlay page, so only one display has a selection. */
  const SELECTED = 'overlay:selected'
  /** `fresh`: the user started a new selection there, so this display drops its annotations. */
  type Selected = { label: string; active: boolean; fresh: boolean }

  type Drag =
    | { kind: 'draw'; start: Point }
    | { kind: 'move'; start: Point; origin: Rect }
    | { kind: 'resize'; start: Point; origin: Rect; handle: Handle }

  /** Show the pixel loupe while dragging. Defaults to the magnifier setting. */
  let { magnifier }: { magnifier?: boolean } = $props()
  const loupeOn = $derived(magnifier ?? settings.magnifier)

  const view = getCurrentWebviewWindow()
  let canvas: HTMLCanvasElement = $state()!
  // The latest session asked to draw, so a show event and the load check never draw twice
  // and an older fetch never paints over a newer one.
  let current = -1
  // Bumped when the overlay closes, so a draw still fetching its frame never paints after.
  let generation = 0
  /** The drawn frame: its size in physical px and its display's size in logical px. */
  let frame = $state<Size>({ width: 0, height: 0 })
  let display = $state<Size>({ width: 0, height: 0 })
  // Same scale as Rust: frame pixels per logical px.
  const scale = $derived({ x: frame.width / display.width, y: frame.height / display.height })

  /** The selection in logical px, relative to this display. */
  let selection = $state<Rect | null>(null)
  let drag = $state<Drag | null>(null)
  let escapeShortcut = true
  let editing = $state(false)
  let pointer = $state<Point>({ x: 0, y: 0 })
  let badge = $state<Size>({ width: 0, height: 0 })
  let loupe = $state<Size>({ width: 0, height: 0 })
  /** A copy or save is running: keys wait for it. */
  let busy = false
  /** A short error message, shown for a few seconds. */
  let notice = $state<string | null>(null)
  let noticeTimer: ReturnType<typeof setTimeout> | undefined
  let annotator = $state<ReturnType<typeof Annotator>>()
  let annotated = $state(false)

  const physical = $derived(selection && toPhysical(selection, scale, frame))
  const cursor = $derived(
    drag?.kind === 'move' ? CURSORS.move : drag?.kind === 'resize' ? CURSORS[drag.handle] : null,
  )
  const loupeAt = $derived({
    x: Math.floor(pointer.x * scale.x),
    y: Math.floor(pointer.y * scale.y),
  })
  // A captured drag can go past this display; the loupe hides until the pointer is back.
  const onDisplay = $derived(
    pointer.x >= 0 && pointer.y >= 0 && pointer.x < display.width && pointer.y < display.height,
  )

  function reset() {
    selection = null
    drag = null
    editing = false
    annotator?.reset()
  }

  /** Gives up on `session` (never a newer one); Rust closes it. */
  function close(session: number) {
    void invoke('close_overlay', { session })
  }

  // Draws this display's frozen frame, then tells Rust, which shows the (hidden) window.
  async function draw(info: FrameInfo) {
    // Session ids only grow: an older (or the same) session never redraws.
    if (info.session <= current) return
    current = info.session
    const drawing = generation
    escapeShortcut = info.escapeShortcut
    reset()
    try {
      const data = await invoke<ArrayBuffer | Uint8Array<ArrayBuffer>>('overlay_frame', {
        session: info.session,
      })
      if (current !== info.session || generation !== drawing) return
      // Raw RGBA. Tauri returns an ArrayBuffer; accept a view too.
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
      // Not kept afterwards: the canvas has the pixels and Rust keeps the original.
      context.putImageData(new ImageData(pixels, info.width, info.height), 0, 0)
      frame = { width: info.width, height: info.height }
      display = { width: info.logicalWidth, height: info.logicalHeight }
      await invoke('overlay_painted', { session: info.session })
    } catch (e) {
      // Never leave the user on a dead overlay.
      console.error('overlay could not draw its frame', e)
      close(info.session)
    }
  }

  /**
   * Sets the selection and tells the other displays when this one gains or loses it. A rect
   * under one physical pixel on either side is no selection.
   */
  function select(next: Rect | null) {
    const rect = next && !isEmpty(toPhysical(next, scale, frame)) ? next : null
    // Resizing an annotated selection to nothing keeps the last size, so nothing is lost.
    if (!rect && annotated && drag?.kind === 'resize') return
    const active = rect !== null
    if (active !== (selection !== null)) {
      const fresh = drag?.kind === 'draw'
      void emit(SELECTED, { label: view.label, active, fresh } satisfies Selected)
    }
    selection = rect
  }

  /** Drops the selection and any drag, so the next pointer move does not bring it back. */
  function clear() {
    drag = null
    select(null)
    annotator?.reset()
  }

  /**
   * Esc from Rust: undo one thing here (the size edit, else the selection or drag) and say
   * whether anything was undone. Rust closes the overlay when no page undid anything.
   */
  function escape({ session, token }: EscapeEvent) {
    let handled = true
    if (annotator?.escape()) {
      // The annotations undid one step: the shape drawn, a menu, the shape selection, a tool.
    } else if (editing) editing = false
    else if (selection || drag) clear()
    else handled = false
    void invoke('overlay_escaped', { session, token, handled })
  }

  function show(message: string) {
    clearTimeout(noticeTimer)
    notice = message
    noticeTimer = setTimeout(() => (notice = null), 3000)
  }

  /**
   * Copies (closing the overlay) or saves (Rust hides the windows while the Save dialog is
   * open, then closes, or shows them again on cancel). On failure the overlay stays open and
   * says so.
   */
  async function finish(command: 'overlay_copy' | 'overlay_save', rect: Rect) {
    if (busy) return
    busy = true
    try {
      await invoke(command, { session: current, rect })
    } catch (e) {
      console.error(`${command} failed`, e)
      show(command === 'overlay_copy' ? "Couldn't copy" : "Couldn't save")
    } finally {
      busy = false
    }
  }

  // The overlay closed: free the frame. The next frame is drawn before the window shows
  // again, so nothing stale or empty is ever seen. Hiding for the Save dialog keeps it.
  function free() {
    generation++
    canvas.width = 0
    canvas.height = 0
    reset()
    clearTimeout(noticeTimer)
    notice = null
  }

  /**
   * ⌘ on macOS, Ctrl elsewhere, plus `key` (a Latin letter), with no other modifier. On a
   * non-Latin layout the physical key counts, as in other apps.
   */
  function shortcut(e: KeyboardEvent, key: string) {
    const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
    const latin = /^[a-z]$/i.test(e.key)
    const pressed = latin ? e.key.toLowerCase() === key : e.code === `Key${key.toUpperCase()}`
    return mod && !e.altKey && !e.shiftKey && pressed
  }

  const at = (e: PointerEvent): Point => ({ x: e.clientX, y: e.clientY })

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0 || frame.width === 0) return
    // Keys go to the display with the selection.
    view.setFocus().catch((err) => console.error('could not focus the overlay', err))
    const start = at(e)
    pointer = start
    const handle = (e.target as Element).closest<HTMLElement>('[data-handle]')?.dataset.handle
    if (selection && handle) {
      drag = { kind: 'resize', start, origin: selection, handle: handle as Handle }
    } else if (selection && contains(selection, start)) {
      drag = { kind: 'move', start, origin: selection }
    } else if (annotated) {
      // A new selection would drop the annotations: ignore the press instead.
      return
    } else {
      select(null)
      drag = { kind: 'draw', start }
    }
    // A selection stays on the display where the drag started: with the pointer captured,
    // moves keep coming here past the window's edges, and every rect is clamped to this
    // display. While the button is held, other displays' windows get no pointerdown.
    ;(e.currentTarget as Element).setPointerCapture?.(e.pointerId)
  }

  function onpointermove(e: PointerEvent) {
    pointer = at(e)
    if (!drag) return
    const dx = pointer.x - drag.start.x
    const dy = pointer.y - drag.start.y
    if (drag.kind === 'move') return select(move(drag.origin, dx, dy, display))
    const rect =
      drag.kind === 'draw'
        ? fromPoints(drag.start, pointer, display)
        : resize(drag.origin, drag.handle, dx, dy, display)
    select(rect)
  }

  function onpointerup() {
    drag = null
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // Rust owns Esc and forwards it, so one press is one action. Only if its shortcut
      // could not be registered does the page's own key go to Rust.
      if (!escapeShortcut) void invoke('overlay_escape')
      return
    }
    // Typing in the size badge: Enter applies the size there.
    if (e.target instanceof Element && e.target.closest('[data-size-badge]')) return
    if (annotator?.key(e)) return
    if (!selection || !physical) return
    if (e.key === 'Enter' || shortcut(e, 'c')) {
      e.preventDefault()
      void finish('overlay_copy', physical)
      return
    }
    if (shortcut(e, 's')) {
      e.preventDefault()
      void finish('overlay_save', physical)
      return
    }
    const moved = nudge(selection, e.key, e.shiftKey ? 10 : 1, scale, display)
    if (moved) {
      e.preventDefault()
      select(moved)
    }
  }

  function onresize(size: Size, keepRatio: boolean) {
    if (selection) select(resizeTo(selection, size, scale, frame, keepRatio))
  }

  onMount(() => {
    const listeners = Promise.all([
      view.listen<FrameInfo>('overlay:show', (e) => void draw(e.payload)),
      view.listen<EscapeEvent>('overlay:escape', (e) => escape(e.payload)),
      view.listen('overlay:closed', free),
      // A selection started on another display clears this one's, annotations included.
      listen<Selected>(SELECTED, ({ payload }) => {
        if (payload.label === view.label || !payload.active || !payload.fresh) return
        drag = null
        selection = null
        annotator?.reset()
      }),
    ])
    // Asked only once the listeners are registered, so a frame sent while this window was
    // loading is never missed.
    void listeners
      .then(() => invoke<FrameInfo | null>('overlay_ready'))
      .then((info) => info && draw(info))
    return () => void listeners.then((stops) => stops.forEach((stop) => stop()))
  })
</script>

<svelte:window {onkeydown} />

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
  <Annotator
    bind:this={annotator}
    bind:annotated
    {selection}
    {display}
    {scale}
    options={{ rememberRegion: settings.rememberRegion, magnifier: settings.magnifier }}
    actions={{ option: setSetting }}
  />
  {#if physical}
    <Selection rect={selection!} />
    {@const p = badgePosition(selection!, badge, display)}
    <div
      class="floating"
      style:left="{p.x}px"
      style:top="{p.y}px"
      bind:offsetWidth={badge.width}
      bind:offsetHeight={badge.height}
      onpointerdown={(e) => e.stopPropagation()}
      role="presentation"
    >
      <SizeBadge size={physical} {onresize} bind:editing />
    </div>
  {:else}
    <div class="dim"></div>
  {/if}
  {#if loupeOn && drag && drag.kind !== 'move' && onDisplay}
    {@const p = loupePosition(pointer, loupe, display)}
    <div
      class="floating"
      style:left="{p.x}px"
      style:top="{p.y}px"
      bind:offsetWidth={loupe.width}
      bind:offsetHeight={loupe.height}
    >
      <Magnifier source={canvas} {frame} at={loupeAt} />
    </div>
  {/if}
  {#if notice}
    <div
      class="pointer-events-none fixed bottom-8 left-1/2 -translate-x-1/2 rounded-md bg-popover/90 px-3 py-2 text-sm text-popover-foreground shadow-md"
      role="alert"
    >
      {notice}
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
