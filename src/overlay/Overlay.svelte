<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { emit, listen } from '@tauri-apps/api/event'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { onMount } from 'svelte'
  import { setSetting, settings } from '$lib/settings.svelte'
  import {
    CURSORS,
    badgePosition,
    contains,
    fromPoints,
    intersect,
    isEmpty,
    loupePosition,
    move,
    nudge,
    resize,
    resizeTo,
    toPhysical,
    union,
    type Handle,
    type Point,
    type Rect,
    type Size,
  } from './geometry'
  import Annotator from './annotate/Annotator.svelte'
  import { outputRequest } from './annotate/output'
  import { shortcut } from './keys'
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
  /**
   * `stamp` orders the events (ms since the epoch, the same clock on every display), so a late
   * one never undoes a newer selection; `session` keeps one capture's events out of the next.
   */
  type Selected = { label: string; active: boolean; session: number; stamp: number }

  type Drag =
    | { kind: 'draw'; start: Point }
    | { kind: 'move'; start: Point; origin: Rect }
    /** `keep`: annotations the resize may not cut, so none ends up outside the selection. */
    | { kind: 'resize'; start: Point; origin: Rect; handle: Handle; keep: Rect | null }

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
  /** The newest selection event seen, its `stamp`. */
  let seen = 0

  /**
   * Whether annotations show in the selection. Only then does a press outside it keep them
   * rather than start a new selection, so hidden ones never strand the user.
   */
  function annotationsShown() {
    const painted = annotator?.painted()
    return !!(painted && selection && intersect(painted, selection))
  }

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
    seen = 0
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
    const active = rect !== null
    if (active !== (selection !== null)) {
      seen = Math.max(Date.now(), seen + 1)
      const event: Selected = { label: view.label, active, session: current, stamp: seen }
      void emit(SELECTED, event)
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
   * Undoes one thing for Esc: the size edit, else one annotation step (the shape being drawn,
   * a menu, the selected shape, the tool), else the selection or drag. Returns whether it did.
   */
  function undoOne(): boolean {
    if (editing) editing = false
    else if (annotator?.escape()) return true
    else if (selection || drag) clear()
    else return false
    return true
  }

  /** Esc from Rust. Rust closes the overlay when no page undid anything. */
  function escape({ session, token }: EscapeEvent) {
    void invoke('overlay_escaped', { session, token, handled: undoOne() })
  }

  function show(message: string) {
    clearTimeout(noticeTimer)
    notice = message
    noticeTimer = setTimeout(() => (notice = null), 3000)
  }

  /**
   * Copies (closing the overlay) or saves (Rust hides the windows while the Save dialog is
   * open, then closes, or shows them again on cancel), annotations included. On failure the
   * overlay stays open and says so.
   */
  async function finish(command: 'overlay_copy' | 'overlay_save', rect: Rect) {
    if (busy) return
    busy = true
    try {
      const session = current
      const marks = await annotator?.exportAnnotations(rect, scale)
      const { body, headers } = outputRequest(session, rect, marks)
      await invoke(command, body, { headers })
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

  const at = (e: PointerEvent): Point => ({ x: e.clientX, y: e.clientY })

  // Keys go to the display pressed, whoever handles the press (the annotations stop theirs).
  function onpointerdowncapture(e: PointerEvent) {
    if (e.button !== 0 || frame.width === 0) return
    view.setFocus().catch((err) => console.error('could not focus the overlay', err))
  }

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0 || frame.width === 0) return
    const start = at(e)
    pointer = start
    const handle = (e.target as Element).closest<HTMLElement>('[data-handle]')?.dataset.handle
    if (selection && handle) {
      const painted = annotator?.painted()
      const keep = painted ? intersect(painted, selection) : null
      drag = { kind: 'resize', start, origin: selection, handle: handle as Handle, keep }
    } else if (selection && contains(selection, start)) {
      drag = { kind: 'move', start, origin: selection }
    } else if (annotationsShown()) {
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
    if (drag.kind === 'draw') return select(fromPoints(drag.start, pointer, display))
    const rect = resize(drag.origin, drag.handle, dx, dy, display)
    // It cannot shrink past the annotations shown in it.
    select(drag.keep ? union(rect, drag.keep) : rect)
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
    // An open toolbar menu owns the keyboard.
    if (annotator?.menuOpen() || annotator?.key(e)) return
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
        // Only the newest event of this capture counts.
        if (payload.session !== current || payload.stamp <= seen) return
        seen = payload.stamp
        if (payload.label !== view.label && payload.active) reset()
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
  {onpointerdowncapture}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
>
  <canvas bind:this={canvas} width="0" height="0"></canvas>
  <Annotator
    bind:this={annotator}
    {selection}
    {display}
    {scale}
    options={{ rememberRegion: settings.rememberRegion, magnifier: settings.magnifier }}
    actions={{
      copy: () => physical && void finish('overlay_copy', physical),
      save: () => physical && void finish('overlay_save', physical),
      option: setSetting,
    }}
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
