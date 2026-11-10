<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import { contains, nudge, type Point, type Rect, type Scale, type Size } from '../geometry'
  import { shortcut, typing } from '../keys'
  import { TOAST } from '../panel'
  import { Board, exportShapes, loadKonva, type AnnotationExport } from './board'
  import { commit, empty, redo, replace, undo, without, type History, type Shapes } from './history'
  import { recall, remember } from './memory'
  import {
    FREEHAND,
    MAX_BLURS,
    READY,
    SELECT_KEY,
    bounds,
    constrain,
    draw,
    Freehand,
    keepInside,
    paintedBounds,
    toolForKey,
    translate,
    type Style,
    type TextShape,
    type Tool,
  } from './shapes'
  import {
    DEFAULT_TEXT,
    MAX_TEXT,
    TEXT_SIZES,
    within,
    boxAt,
    sizeOf,
    type ShownTextOptions,
    type TextOptions,
  } from './text'
  import TextEditor from './TextEditor.svelte'
  import type { Factor } from './upscale'
  import type ToolbarComponent from './Toolbar.svelte'
  import type { Option, ToolbarActions } from './Toolbar.svelte'

  /**
   * Annotations on the selection, with the tool rail and action bar (the toolbar) next to it.
   * Konva and the toolbar load when the page is idle; the stage is built on the first selection.
   * The overlay owns the keyboard and calls `key` and `escape`; it calls `reset` when the
   * annotations should go.
   */
  let {
    selection,
    display,
    scale,
    options,
    actions: given = {},
    source = null,
    size = null,
  }: {
    selection: Rect | null
    display: Size
    /** Physical px per logical px, so arrow keys nudge by whole pixels. */
    scale: Scale
    options: Record<Option, boolean>
    actions?: Partial<ToolbarActions>
    /** The frozen frame, for the blur preview. */
    source?: HTMLCanvasElement | null
    /** The selection in physical px: Copy and Save show their larger sizes from it. */
    size?: Size | null
  } = $props()

  const noop = () => {}
  const actions: ToolbarActions = $derived({
    copy: noop,
    save: noop,
    share: noop,
    cancel: noop,
    option: noop,
    ...given,
  })

  let container: HTMLDivElement = $state()!
  let Toolbar = $state<typeof ToolbarComponent | null>(null)
  let toolbar = $state<ReturnType<typeof ToolbarComponent>>()
  let board = $state.raw<Board | null>(null)
  /** Konva was asked for during this selection; a failed load waits for the next one. */
  let tried = false
  /** Konva could not load: the tools show as unavailable until a later selection retries. */
  let failed = $state(false)
  let history = $state.raw<History>(empty)
  // Each capture starts in Select, with the last color and size.
  let tool = $state<Tool | null>(null)
  let style = $state(recall())

  let selected = $state<string | null>(null)
  /**
   * A drag drawing a shape: where it started, where the pointer is, where the shape ends, and
   * a freehand stroke.
   */
  type Stroke = { tool: Tool; start: Point; pointer: Point; end: Point; path: Freehand }
  let stroke: Stroke | null = null
  let ids = 0
  let cursor = $state('')
  let textOptions = $state<TextOptions>(DEFAULT_TEXT)

  /** The text being typed: a new one, or one taken off the board to edit again. */
  type Editing = Pick<TextShape, 'id' | 'text' | 'fontSize' | 'style'> & {
    at: Point
    existing: boolean
  }
  let editing = $state<Editing | null>(null)
  /** The text hidden from the board while it is edited. */
  const hidden = $derived(editing?.id)

  const drawing = $derived(tool !== null && READY.has(tool) && !failed)

  function loadToolbar() {
    if (Toolbar) return
    import('./Toolbar.svelte').then(
      (m) => (Toolbar = m.default),
      (e: unknown) => console.error('could not load the toolbar', e),
    )
  }

  // Fetched when the page is idle (long before any hotkey), so the first selection and the
  // first click on the toolbar never wait. The overlay's own bundle stays small.
  onMount(() => {
    const idle = window.requestIdleCallback ?? ((f: () => void) => setTimeout(f, 200))
    idle(() => {
      loadToolbar()
      loadKonva().catch(() => {})
    })
    return () => board?.destroy()
  })

  // The stage is built on the first selection. A failed load is tried again once per new
  // selection, never in a loop.
  $effect(() => {
    if (!selection) {
      tried = false
      return
    }
    if (board || tried) return
    tried = true
    untrack(loadToolbar)
    loadKonva()
      .then((K) => {
        failed = false
        board = new Board(
          K,
          container,
          {
            onchange: (shape) => set(replace(history.present, keepInside(shape, selection!))),
            onselect: (id) => (selected = id),
            onedit: (id) => {
              const shape = history.present.find((s) => s.id === id)
              if (shape?.kind === 'text') edit(shape)
            },
          },
          () => source,
        )
      })
      .catch((e: unknown) => {
        failed = true
        console.error('could not load the annotation layer', e)
      })
  })

  $effect(() => board?.place(selection, display))
  $effect(() =>
    board?.render(hidden ? without(history.present, hidden) : history.present, selected),
  )

  // Each is kept only when it changes, so this window never writes back another's newer choice.
  const lastColor = $derived(style.color)
  const lastSize = $derived(style.size)
  $effect(() => remember('color', lastColor))
  $effect(() => remember('size', lastSize))

  /** Moves history, dropping the shape selection if that shape is gone. */
  function move(next: History) {
    history = next
    if (selected && !next.present.some((s) => s.id === selected)) selected = null
  }
  const set = (shapes: Shapes) => move(commit(history, shapes))

  /** Whether there are any shapes. */
  export const annotated = () => history.present.length > 0

  /** The box all the shapes paint, or `null` without shapes. */
  export const painted = () => paintedBounds(history.present)

  /** Whether a toolbar menu is open: it owns the plain keys meanwhile. */
  export const menuOpen = () => toolbar?.menuOpen() ?? false

  /** Closes an open toolbar menu, as a click outside it would. */
  export const closeMenus = () => void toolbar?.closeMenus()

  /** Whether text is being typed: every key is the text box's meanwhile, ⌘C and ⌘S included. */
  export const typingText = () => editing !== null

  /**
   * Drops every annotation and the shape being drawn, and goes back to Select with the last
   * color and size used (in any overlay window). Not undoable. `frame` says the frame changed too.
   */
  export function reset(frame = false) {
    history = empty
    selected = null
    editing = null
    tool = null
    style = recall()
    cancel()
    if (frame) board?.newFrame()
  }

  /** Picks a tool (`null`: Select), committing any text being typed. */
  function pick(next: Tool | null) {
    finish()
    tool = next
  }

  /** Opens the editor for a new text with its caret at `p`. */
  function place(p: Point) {
    const fontSize = TEXT_SIZES[textOptions.size]
    editing = {
      id: `s${++ids}`,
      at: boxAt(p, fontSize),
      text: '',
      fontSize,
      style,
      existing: false,
    }
  }

  /** Opens the editor on a text already on the board. Its background becomes the tool's. */
  function edit(shape: TextShape) {
    const { id, x, y, text, fontSize, background } = shape
    selected = null
    editing = { id, at: { x, y }, text, fontSize, style: shape.style, existing: true }
    textOptions = { ...textOptions, background }
  }

  /**
   * Commits the text being edited, as typed and where the editor was, as one undo step (none if
   * nothing changed). Text that is all whitespace is dropped (an edited text emptied is deleted).
   */
  function finish() {
    const e = editing
    if (!e || !board) return
    editing = null
    const text = e.text.slice(0, MAX_TEXT)
    if (!text.trim()) {
      if (e.existing) set(without(history.present, e.id))
      return
    }
    const { id, fontSize, style: textStyle } = e
    const { background } = textOptions
    const before = history.present.find((s) => s.id === id)
    const same =
      before?.kind === 'text' &&
      before.text === text &&
      before.fontSize === fontSize &&
      before.background === background &&
      before.style.color === textStyle.color
    if (same) return
    const shape: TextShape = {
      id,
      kind: 'text',
      style: textStyle,
      ...e.at,
      ...board.measure(text, fontSize),
      text,
      fontSize,
      background,
    }
    set(e.existing ? replace(history.present, shape) : [...history.present, shape])
  }

  const selectedShape = $derived(history.present.find((s) => s.id === selected))
  /** The selected shape, if it is a text: the toolbar's options apply to it, not the tool's. */
  const selectedText = $derived(selectedShape?.kind === 'text' ? selectedShape : undefined)

  /** The selected shape the color and size apply to: any but a blur, which has neither. */
  const styled = $derived(selectedShape?.kind === 'blur' ? undefined : selectedShape)

  /**
   * The color and size the toolbar shows: the edited text's, the selected shape's (only the
   * color, for text), or the tool's.
   */
  const shownStyle = $derived.by((): Style => {
    if (editing) return editing.style
    if (styled?.kind === 'text') return { ...style, color: styled.style.color }
    return styled ? styled.style : style
  })

  /** The text options the toolbar shows: the edited or selected text's, or the tool's. */
  const shownText = $derived.by((): ShownTextOptions | null => {
    if (editing) return { size: sizeOf(editing.fontSize), background: textOptions.background }
    if (selectedText)
      return { size: sizeOf(selectedText.fontSize), background: selectedText.background }
    return tool === 'text' ? textOptions : null
  })

  /**
   * A changed text option. A selected text takes it, as one undo step; otherwise the tool does,
   * and the text being typed follows it.
   */
  function setText(change: Partial<TextOptions>) {
    const fontSize = change.size && TEXT_SIZES[change.size]
    if (selectedText && !editing) {
      const { fontSize: was, background, text } = selectedText
      const next = { fontSize: fontSize || was, background: change.background ?? background }
      const box = { ...selectedText, ...next, ...board!.measure(text, next.fontSize) }
      // A text that grows stays wholly in the selection, if it fits.
      const kept = selection ? { ...box, ...within(box, selection) } : box
      return set(replace(history.present, kept))
    }
    textOptions = { ...textOptions, ...change }
    if (editing && fontSize) editing.fontSize = fontSize
  }

  /**
   * A new color or size. The selected shape takes it as one undo step (text only its color),
   * or the text being typed does; the next shapes take it too, and it is kept for the next capture.
   */
  function setStyle(next: Style) {
    if (editing) editing.style = next
    else if (styled) {
      // Only the color shows on text, so a size change leaves no undo step.
      const restyled = styled.kind === 'text' ? { ...styled.style, color: next.color } : next
      set(replace(history.present, { ...styled, style: restyled }))
    }
    style = next
  }

  /** The press that committed the text being edited: it does nothing more. */
  let committing: Event | null = null

  /** A press while text is edited commits it and does nothing more; Konva never sees it. */
  function onpointerdowncapture(e: PointerEvent) {
    if (!editing || e.button !== 0) return
    e.stopPropagation()
    committing = e
    finish()
  }

  function cancel() {
    stroke = null
    board?.preview(null)
  }

  /** Takes the drag on to `e`. Returns whether what it draws changed. */
  function follow(s: Stroke, e: PointerEvent, sel: Rect): boolean {
    s.pointer = at(e)
    const p = constrain(s.tool, s.start, s.pointer, sel, e.shiftKey)
    if (FREEHAND.has(s.tool)) return s.path.add(p)
    s.end = p
    return true
  }

  /** What the drag draws, or `null`. The preview shares a stroke's points. */
  function drawn(s: Stroke, id: string, preview = false) {
    return FREEHAND.has(s.tool)
      ? s.path.shape(s.tool, id, style, preview)
      : draw(s.tool, id, s.start, s.end, style)
  }

  const at = (e: PointerEvent): Point => ({ x: e.clientX, y: e.clientY })

  function onpointerdown(e: PointerEvent) {
    // On the layer itself, the press that committed text reaches this handler too.
    if (e === committing) return e.stopPropagation()
    // A press on the board dismisses an open menu, as a click outside it would: drawing,
    // moving a shape and pressing the overlay all stop it reaching the menu's own listener.
    closeMenus()
    if (e.button !== 0 || !selection || !board) return
    // On a shape or its handles: Konva moves or resizes it.
    if (board.hits(at(e))) return e.stopPropagation()
    selected = null
    // Without a drawing tool, or outside the selection, the press goes on to the overlay.
    if (!drawing || !contains(selection, at(e))) return
    e.stopPropagation()
    const p = at(e)
    const path = new Freehand()
    path.add(p)
    stroke = { tool: tool!, start: p, pointer: p, end: p, path }
    container.setPointerCapture?.(e.pointerId)
  }

  function onpointermove(e: PointerEvent) {
    if (!selection) return
    // Only the top layer redraws, and a freehand stroke only once the pointer moved enough.
    if (stroke && follow(stroke, e, selection)) board?.preview(drawn(stroke, 'draft', true))
    // Over the selection: crosshair to draw, else move it. Elsewhere, the overlay's cursor.
    cursor = !contains(selection, at(e))
      ? ''
      : !drawing
        ? 'move'
        : tool === 'text'
          ? 'text'
          : 'crosshair'
  }

  function onpointerup(e: PointerEvent) {
    if (!stroke || !selection) return
    if (stroke.tool === 'text') {
      const p = stroke.start
      cancel()
      return place(p)
    }
    follow(stroke, e, selection)
    const shape = drawn(stroke, `s${++ids}`)
    cancel()
    if (shape?.kind === 'blur' && blurs() >= MAX_BLURS) return say(`${MAX_BLURS} blurs at most`)
    if (shape) set([...history.present, shape])
  }

  const blurs = () => history.present.filter((s) => s.kind === 'blur').length

  /** A short hint, shown for a few seconds. */
  let hint = $state<string | null>(null)
  let hintTimer: ReturnType<typeof setTimeout> | undefined
  function say(message: string) {
    clearTimeout(hintTimer)
    hint = message
    hintTimer = setTimeout(() => (hint = null), 3000)
  }

  /** Shift pressed or let go mid-drag: snaps or unsnaps the shape without a pointer move. */
  function onshift(e: KeyboardEvent) {
    if (e.key !== 'Shift' || !stroke || !selection || FREEHAND.has(stroke.tool)) return
    stroke.end = constrain(stroke.tool, stroke.start, stroke.pointer, selection, e.shiftKey)
    board?.preview(drawn(stroke, 'draft'))
  }

  /** Handles an annotation key. Returns whether it did, so the overlay skips it. */
  export function key(e: KeyboardEvent): boolean {
    // While text is edited, every key is the textarea's: no shortcuts, copy or nudges.
    if (editing) return true
    if (!selection || menuOpen() || typing(e.target)) return false
    const redoing = shortcut(e, 'z', true)
    if (redoing || shortcut(e, 'z')) {
      e.preventDefault()
      move(redoing ? redo(history) : undo(history))
      return true
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return false
    const shape = selectedShape
    if (shape && (e.key === 'Backspace' || e.key === 'Delete')) {
      e.preventDefault()
      set(without(history.present, shape.id))
      return true
    }
    if (shape?.kind === 'text' && e.key === 'Enter') {
      e.preventDefault()
      edit(shape)
      return true
    }
    const b = shape && bounds(shape)
    const moved = b && nudge(b, e.key, e.shiftKey ? 10 : 1, scale, display)
    if (shape && b && moved) {
      e.preventDefault()
      const next = keepInside(translate(shape, moved.x - b.x, moved.y - b.y), selection)
      set(replace(history.present, next))
      return true
    }
    if (e.key.toUpperCase() === SELECT_KEY) {
      e.preventDefault()
      pick(null)
      return true
    }
    const next = toolForKey(e.key)
    if (!next || !READY.has(next) || failed) return false
    e.preventDefault()
    tool = next
    return true
  }

  /**
   * Esc, one step at a time: the shape being drawn, an open menu, the text being edited (it is
   * kept), the selected shape, then the tool. Returns whether it undid anything; if not, the
   * overlay handles Esc.
   */
  export function escape(): boolean {
    if (stroke) cancel()
    else if (toolbar?.closeMenus()) return true
    else if (editing) finish()
    else if (selected) selected = null
    // Without a selection the tool does nothing, so Esc goes on to close the overlay.
    else if (tool && selection) tool = null
    else return false
    return true
  }

  /**
   * What Enter and Esc do now, for the key hint; `null` leaves them to the overlay. Enter is
   * `false` when it does nothing here (a menu or the text box has it).
   */
  export function keys(): { enter: string | false | null; esc: string | null } {
    const menu = menuOpen()
    const enter = editing || menu ? false : selectedText ? 'edit' : null
    const esc = menu
      ? 'close menu'
      : editing
        ? 'finish text'
        : selected
          ? 'deselect'
          : tool && selection
            ? 'drop tool'
            : null
    return { enter, esc }
  }

  /**
   * The annotations alone at native size times `factor`, for Rust to composite. `crop` is the
   * physical rect being captured; `ratio` is physical px per logical px on each axis. See
   * docs/annotations.md.
   */
  export function exportAnnotations(
    crop: Rect,
    ratio: Scale,
    factor: Factor = 1,
  ): Promise<AnnotationExport> {
    finish()
    return exportShapes(history.present, crop, ratio, factor)
  }
</script>

<svelte:window onkeydown={onshift} onkeyup={onshift} />

<div
  class="layer"
  style:width="{selection ? display.width : 0}px"
  style:height="{selection ? display.height : 0}px"
  style:cursor
  bind:this={container}
  data-testid="annotations"
  role="presentation"
  {onpointerdowncapture}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={cancel}
></div>

{#if hint}
  <div class={TOAST} role="status">
    {hint}
  </div>
{/if}

{#if editing && board}
  <TextEditor
    bind:text={editing.text}
    at={editing.at}
    fontSize={editing.fontSize}
    background={textOptions.background}
    color={editing.style.color}
    measure={board.measure}
    oncommit={finish}
  />
{/if}

{#if Toolbar && selection}
  <Toolbar
    bind:this={toolbar}
    {selection}
    {display}
    {size}
    {tool}
    ontool={pick}
    bind:style={() => shownStyle, setStyle}
    text={shownText}
    ontext={setText}
    keepFocus={editing !== null}
    drawable={!failed}
    canUndo={history.past.length > 0 || editing !== null}
    canRedo={history.future.length > 0 && !editing}
    canClear={history.present.length > 0}
    onundo={() => (editing ? (editing = null) : move(undo(history)))}
    onredo={() => move(redo(history))}
    onclear={() => {
      finish()
      set([])
    }}
    {options}
    {actions}
  />
{/if}

<style>
  /* The whole display while there is a selection; presses it does not use go on to the overlay. */
  .layer {
    position: fixed;
    left: 0;
    top: 0;
  }
</style>
