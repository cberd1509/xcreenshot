<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { emit, listen } from '@tauri-apps/api/event'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { onMount } from 'svelte'
  import { failed } from '$lib/log'
  import { setCaptureOption, settings } from '$lib/settings.svelte'
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
    resizeLocked,
    growToRatio,
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
  import { control, fieldShortcut, keyLabels, modifier, shortcut, typing } from './keys'
  import { TOAST } from './panel'
  import Magnifier from './Magnifier.svelte'
  import QuickBar from './QuickBar.svelte'
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
    /** The remembered region on this display (logical px), selected from the start. */
    region: Rect | null
    /** Quick copy is on: the drag copies at once. */
    quickCopy: boolean
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
  /** Broadcast to every overlay page: this capture goes on without quick copy. */
  const ANNOTATE = 'overlay:annotate'
  /** How far (logical px) a press may move before it draws: less is a click. */
  const CLICK_SLOP = 4

  type Drag =
    /** `paused`: Space was pressed before anything was drawn; the start follows the pointer. */
    | { kind: 'draw'; start: Point; paused?: boolean }
    /** `resume`: the draw or resize Space paused to move the selection, taken up on release. */
    | { kind: 'move'; start: Point; origin: Rect; resume?: Resumable }
    | Resize
  /** `keep`: annotations the resize may not cut, so none ends up outside the selection. */
  type Resize = { kind: 'resize'; start: Point; origin: Rect; handle: Handle; keep: Rect | null }
  type Resumable = { kind: 'draw' } | Resize

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
  let drag = $state.raw<Drag | null>(null)
  let escapeShortcut = true
  let editing = $state(false)
  let pointer = $state.raw<Point>({ x: 0, y: 0 })
  /** The pointer moved over this capture, so `pointer` is where it is. */
  let tracked = $state(false)
  /** The size badge's aspect lock: it holds for typed sizes and handle drags. */
  let lock = $state(false)
  let badge = $state<Size>({ width: 0, height: 0 })
  let loupe = $state<Size>({ width: 0, height: 0 })
  /** A copy, save or share is running: keys wait for it. */
  let busy = false
  /** A short error message, shown for a few seconds. */
  let notice = $state<string | null>(null)
  let noticeTimer: ReturnType<typeof setTimeout> | undefined
  let annotator = $state<ReturnType<typeof Annotator>>()
  /** The newest selection event seen, its `stamp`. */
  let seen = 0
  /** The selection is the remembered region, untouched: Esc closes rather than clears it. */
  let restored = $state(false)
  /** Quick copy, for this capture: the drag copies at once and there is no toolbar. */
  let quick = $state(false)
  /** The last press started a draw on the frame, so a double-click there picks a window. */
  let pressedFrame = false
  /** Counts presses, so a window found for a double-click is dropped if another gesture began. */
  let gesture = 0

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
  /** No frame drawn: the page renders nothing and ignores the pointer. */
  const idle = $derived(frame.width === 0)
  // A captured drag can go past this display; the loupe hides until the pointer is back.
  const onDisplay = $derived(
    tracked &&
      pointer.x >= 0 &&
      pointer.y >= 0 &&
      pointer.x < display.width &&
      pointer.y < display.height,
  )
  // Before a selection, and while drawing or resizing one: never while moving it.
  const loupeShown = $derived(
    loupeOn && onDisplay && (drag ? drag.kind !== 'move' : !selection) && !idle,
  )
  /** The keys the hint at the bottom shows, and what each does now. */
  const hints = $derived.by(() => {
    if (!selection)
      return [
        ['Drag', 'select area'],
        ['Double-click', 'window'],
        [`${keyLabels().key} double-click`, 'full screen'],
        ['Esc', 'cancel'],
      ]
    const now = annotator?.keys() ?? { enter: null, esc: null }
    // As Esc goes (see `undoOne`): the size edit, then the annotations, then the selection.
    const esc = editing ? 'cancel' : (now.esc ?? (restored ? 'cancel' : 'clear'))
    const enter = editing ? 'apply' : now.enter === null ? 'copy' : now.enter
    const typingText = annotator?.typingText()
    return [
      ...(enter ? [['Enter', enter]] : []),
      ...(typingText ? [] : [[`${keyLabels().mod}S`, 'save']]),
      ['Space', 'move'],
      ['Esc', esc],
    ]
  })

  function reset() {
    restored = false
    selection = null
    drag = null
    tracked = false
    editing = false
    annotator?.reset(true)
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
    quick = info.quickCopy
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
      // Remember region: the same state as just after a drag. Rust clamped it to the display.
      if (info.region) {
        select(info.region)
        restored = true
      }
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
    restored = false
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
    else if (restored) return false
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

  const FAILED = {
    overlay_copy: "Couldn't copy",
    overlay_save: "Couldn't save",
    overlay_share: "Couldn't share",
  }

  /**
   * Copies (closing the overlay), saves (Rust hides the windows while the Save dialog is open,
   * then closes, or shows them again on cancel) or shares (the share sheet opens under
   * `anchor`; picking a service closes the overlay), annotations included. On failure the
   * overlay stays open and says so.
   */
  async function finish(command: keyof typeof FAILED, rect: Rect, anchor?: Rect) {
    if (busy) return
    busy = true
    try {
      const session = current
      const marks = await annotator?.exportAnnotations(rect, scale)
      // Nothing in a quick copy capture is remembered (Ctrl+S works there too).
      const remember = !quick
      const { body, headers } = outputRequest(session, rect, marks, { anchor, remember })
      await invoke(command, body, { headers })
    } catch (e) {
      console.error(`${command} failed`, e)
      show(FAILED[command])
    } finally {
      busy = false
    }
  }

  // The overlay closed: free the frame. With no frame the page renders nothing (`idle`), so
  // WebKit also drops the canvas layer's full-screen backing store. The next frame is drawn
  // before the window shows again, so nothing stale or empty is ever seen. Hiding for the Save
  // dialog keeps it.
  function free() {
    generation++
    canvas.width = 0
    canvas.height = 0
    frame = { width: 0, height: 0 }
    reset()
    clearTimeout(noticeTimer)
    notice = null
  }

  const at = (e: PointerEvent): Point => ({ x: e.clientX, y: e.clientY })

  // Keys go to the display pressed, whoever handles the press (the annotations stop theirs).
  function onpointerdowncapture(e: PointerEvent) {
    pressedFrame = false
    gesture++
    if (e.button !== 0 || idle) return
    view.setFocus().catch((err) => console.error('could not focus the overlay', err))
  }

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0 || idle) return
    // A quick copy is on its way: one drag, one copy, so a second drag can't race it.
    if (quick && busy) return
    const start = at(e)
    pointer = start
    const handle = (e.target as Element).closest<HTMLElement>('[data-handle]')?.dataset.handle
    if (selection && handle) {
      drag = {
        kind: 'resize',
        start,
        origin: selection,
        handle: handle as Handle,
        keep: kept(selection),
      }
    } else if (selection && contains(selection, start)) {
      drag = { kind: 'move', start, origin: selection }
    } else if (annotationsShown()) {
      // A new selection would drop the annotations: ignore the press instead.
      return
    } else {
      select(null)
      drag = { kind: 'draw', start }
      pressedFrame = true
    }
    // A selection stays on the display where the drag started: with the pointer captured,
    // moves keep coming here past the window's edges, and every rect is clamped to this
    // display. While the button is held, other displays' windows get no pointerdown.
    ;(e.currentTarget as Element).setPointerCapture?.(e.pointerId)
  }

  /** The annotations shown in `r`, which a resize of it may not cut. */
  function kept(r: Rect): Rect | null {
    const painted = annotator?.painted()
    return painted ? intersect(painted, r) : null
  }

  function onpointermove(e: PointerEvent) {
    pointer = at(e)
    tracked = true
    if (!drag) return
    if (drag.kind === 'draw' && drag.paused) {
      drag = { ...drag, start: pointer }
      return
    }
    const dx = pointer.x - drag.start.x
    const dy = pointer.y - drag.start.y
    if (drag.kind === 'move') return select(move(drag.origin, dx, dy, display))
    if (drag.kind === 'draw') {
      // A click (or the first of a double-click) that wobbles a little selects nothing.
      if (!selection && Math.hypot(dx, dy) < CLICK_SLOP) return
      // A drag, even as the second press of a double-click: it picks no window.
      pressedFrame = false
      return select(fromPoints(drag.start, pointer, display))
    }
    const rect = (lock ? resizeLocked : resize)(drag.origin, drag.handle, dx, dy, display)
    if (!drag.keep) return select(rect)
    // It cannot shrink past the annotations shown in it; locked, it grows to keep its ratio.
    const holding = union(rect, drag.keep)
    select(lock ? growToRatio(holding, drag.origin.width / drag.origin.height, display) : holding)
  }

  function onpointerup() {
    // A draw moved with Space held is still a draw.
    const drew = drag?.kind === 'draw' || (drag?.kind === 'move' && drag.resume?.kind === 'draw')
    drag = null
    // Quick copy: letting go of the drag copies, as Copy does. A click selects nothing.
    if (quick && drew && physical) void finish('overlay_copy', physical)
  }

  /**
   * Double-click on the frame: selects the window under the pointer, clipped to this display,
   * or the whole display on the desktop, the menu bar, the Dock or the taskbar. With ⌘ (Ctrl
   * off the Mac), always the whole display. Rust listed the windows at the hotkey, so they
   * match the frame. With quick copy on, it copies at once, as a drag does.
   */
  async function ondblclick(e: MouseEvent) {
    if (!pressedFrame || idle || (quick && busy)) return
    const session = current
    const token = gesture
    const whole: Rect = { x: 0, y: 0, ...display }
    let rect = whole
    if (!modifier(e)) {
      const query = { session, x: e.clientX, y: e.clientY, ...display }
      const picked = await invoke<Rect | null>('overlay_window_at', query).catch(
        failed('overlay_window_at'),
      )
      rect = picked ?? whole
    }
    // Closed, or a new drag, click or double-click began while Rust answered.
    if (session !== current || idle || token !== gesture) return
    select(rect)
    if (quick && physical) void finish('overlay_copy', physical)
  }

  /** Leaves quick copy for the rest of this capture, on every display. */
  function annotate() {
    void emit(ANNOTATE, { session: current })
  }

  function turnOff() {
    setCaptureOption('quickCopy', false)
    annotate()
  }

  /** Space held during a draw or resize: the selection moves, as a move drag, until let go. */
  function pause(d: Drag) {
    if (d.kind === 'move') return
    // Nothing drawn yet: the press point moves instead.
    if (!selection) {
      if (d.kind === 'draw') drag = { ...d, paused: true }
      return
    }
    const resume: Resumable = d.kind === 'draw' ? { kind: 'draw' } : d
    drag = { kind: 'move', start: pointer, origin: selection, resume }
  }

  /** Space let go: the draw or resize goes on from the moved selection. */
  function onkeyup(e: KeyboardEvent) {
    if (e.key !== ' ') return
    if (drag?.kind === 'draw' && drag.paused) {
      drag = { kind: 'draw', start: drag.start }
      return
    }
    if (drag?.kind !== 'move' || !drag.resume || !selection) return
    const r = selection
    if (drag.resume.kind === 'resize') {
      drag = { ...drag.resume, start: pointer, origin: r, keep: kept(r) }
      return
    }
    // A draw starts again from the corner across from the pointer.
    const x = pointer.x >= r.x + r.width / 2 ? r.x : r.x + r.width
    const y = pointer.y >= r.y + r.height / 2 ? r.y : r.y + r.height
    drag = { kind: 'draw', start: { x, y } }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // Rust owns Esc and forwards it, so one press is one action. Only if its shortcut
      // could not be registered does the page's own key go to Rust.
      if (!escapeShortcut) void invoke('overlay_escape')
      return
    }
    // Space during a drag moves the selection being drawn or resized.
    if (e.key === ' ' && drag) {
      e.preventDefault()
      pause(drag)
      return
    }
    const output = shortcut(e, 'c') ? 'overlay_copy' : shortcut(e, 's') ? 'overlay_save' : null
    // Keys typed in a field (the size badge, the hex color) are its own: Enter applies there.
    // Copy and Save there are the capture's, taking what was typed first (see `fieldShortcut`).
    const own = output ? fieldShortcut(e.target, output === 'overlay_copy') : typing(e.target)
    const inBadge = e.target instanceof Element && !!e.target.closest('[data-size-badge]')
    if (own || (inBadge && !output)) return
    // Enter and Space on a focused control (Save, a tool, a menu item) press that control only.
    if ((e.key === 'Enter' || e.key === ' ') && control(e.target)) return
    // Copy and Save always work, closing an open toolbar menu first, unless text is being
    // typed: then they are the text box's.
    if (output && !annotator?.typingText()) {
      annotator?.closeMenus()
      if (!selection || !physical) return
      e.preventDefault()
      void finish(output, physical)
      return
    }
    // An open toolbar menu owns the other keys.
    if (annotator?.menuOpen() || annotator?.key(e)) return
    if (!selection || !physical) return
    if (e.key === 'Enter') {
      e.preventDefault()
      void finish('overlay_copy', physical)
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
      listen<{ session: number }>(ANNOTATE, ({ payload }) => {
        if (payload.session === current) quick = false
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

<svelte:window {onkeydown} {onkeyup} />

<div
  class="root"
  style:display={idle ? 'none' : null}
  class:dragging={cursor}
  style:cursor
  data-testid="overlay"
  role="presentation"
  {onpointerdowncapture}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  {ondblclick}
  onpointercancel={() => (drag = null)}
>
  <canvas bind:this={canvas} width="0" height="0"></canvas>
  <!-- Quick copy has no toolbar or annotations: the drag ends the capture. -->
  <Annotator
    bind:this={annotator}
    selection={quick ? null : selection}
    {display}
    {scale}
    source={canvas}
    options={{ rememberRegion: settings.rememberRegion, magnifier: settings.magnifier }}
    actions={{
      copy: () => physical && void finish('overlay_copy', physical),
      save: () => physical && void finish('overlay_save', physical),
      share: (anchor) => physical && void finish('overlay_share', physical, anchor),
      cancel: () => close(current),
      option: setCaptureOption,
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
      <SizeBadge size={physical} {onresize} bind:editing bind:lock />
    </div>
  {:else}
    <div class="dim"></div>
    {#if onDisplay}
      <div class="guide down" style:transform="translateX({pointer.x}px)" data-testid="guide"></div>
      <div
        class="guide across"
        style:transform="translateY({pointer.y}px)"
        data-testid="guide"
      ></div>
    {/if}
  {/if}
  {#if loupeShown}
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
    <div class={TOAST} role="alert">{notice}</div>
  {/if}
  {#if !idle}
    {#if quick}
      <QuickBar {annotate} {turnOff} />
    {:else}
      <div class="hint" data-testid="hint">
        {#each hints as [key, action] (key)}
          <span><b>{key}</b> {action}</span>
        {/each}
      </div>
    {/if}
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

  /* The selection color, for the outline, its handles and focused fields. */
  :global(:root) {
    --selection: #3b82f6;
  }

  /* Before a selection, the whole frame is dimmed. */
  .dim {
    background: rgb(9 9 11 / 0.45);
    pointer-events: none;
  }

  /* Full-width and full-height lines through the pointer. */
  .guide {
    position: fixed;
    pointer-events: none;
    background: rgb(255 255 255 / 0.35);
  }

  .down {
    top: 0;
    left: 0;
    width: 1px;
    height: 100%;
  }

  .across {
    top: 0;
    left: 0;
    width: 100%;
    height: 1px;
  }

  /* The keys for the current state, at the bottom. */
  .hint {
    position: fixed;
    left: 50%;
    transform: translateX(-50%);
    pointer-events: none;
    font-size: 12px;
    white-space: nowrap;
    bottom: 28px;
    display: flex;
    gap: 16px;
    padding: 8px 14px;
    border-radius: 999px;
    background: rgb(24 24 27 / 0.92);
    border: 1px solid #27272a;
    color: #a1a1aa;
  }

  .hint b {
    color: #e4e4e7;
    font-weight: 600;
  }

  .floating {
    position: fixed;
    cursor: default;
  }
</style>
