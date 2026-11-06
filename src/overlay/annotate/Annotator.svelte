<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import { contains, nudge, type Point, type Rect, type Scale, type Size } from '../geometry'
  import { shortcut } from '../keys'
  import { Board, exportShapes, loadKonva, type AnnotationExport } from './board'
  import { commit, empty, redo, replace, undo, without, type History, type Shapes } from './history'
  import { toolbarPosition } from './placement'
  import {
    DEFAULT_STYLE,
    READY,
    bounds,
    clampTo,
    draw,
    keepInside,
    paintedBounds,
    toolForKey,
    translate,
    type Tool,
  } from './shapes'
  import type ToolbarComponent from './Toolbar.svelte'
  import type { Option, ToolbarActions } from './Toolbar.svelte'

  /**
   * Annotations on the selection, and the toolbar next to it. Konva and the toolbar load when
   * the page is idle; the stage is built on the first selection. The overlay owns the keyboard
   * and calls `key` and `escape`; it calls `reset` when the annotations should go.
   */
  let {
    selection,
    display,
    scale,
    options,
    actions: given = {},
  }: {
    selection: Rect | null
    display: Size
    /** Physical px per logical px, so arrow keys nudge by whole pixels. */
    scale: Scale
    options: Record<Option, boolean>
    actions?: Partial<ToolbarActions>
  } = $props()

  const noop = () => {}
  const actions: ToolbarActions = $derived({ copy: noop, save: noop, option: noop, ...given })

  let container: HTMLDivElement = $state()!
  let Toolbar = $state<typeof ToolbarComponent | null>(null)
  let toolbar = $state<ReturnType<typeof ToolbarComponent>>()
  let board = $state.raw<Board | null>(null)
  let loading = false
  /** Konva could not load: the tools show as unavailable until a later selection retries. */
  let failed = $state(false)
  let history = $state.raw<History>(empty)
  let tool = $state<Tool | null>(null)
  let style = $state(DEFAULT_STYLE)
  let selected = $state<string | null>(null)
  let start: Point | null = null
  let ids = 0
  let bar = $state<Size>({ width: 0, height: 0 })
  let cursor = $state('')

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

  // The stage is built on the first selection. A failed load retries on the next change.
  $effect(() => {
    if (!selection || board || loading) return
    loading = true
    untrack(loadToolbar)
    loadKonva()
      .then((K) => {
        failed = false
        board = new Board(K, container, {
          onchange: (shape) => set(replace(history.present, keepInside(shape, selection!))),
          onselect: (id) => (selected = id),
        })
      })
      .catch((e: unknown) => {
        failed = true
        console.error('could not load the annotation layer', e)
      })
      .finally(() => (loading = false))
  })

  $effect(() => board?.place(selection, display))
  $effect(() => board?.render(history.present, selected))

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

  /** Whether a toolbar menu is open: it owns the keyboard meanwhile. */
  export const menuOpen = () => toolbar?.menuOpen() ?? false

  /** Drops every annotation, the tool and the shape being drawn. Not undoable. */
  export function reset() {
    history = empty
    selected = null
    tool = null
    cancel()
  }

  function cancel() {
    start = null
    board?.preview(null)
  }

  const at = (e: PointerEvent): Point => ({ x: e.clientX, y: e.clientY })

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0 || !selection || !board) return
    // On a shape or its handles: Konva moves or resizes it.
    if (board.hits(at(e))) return e.stopPropagation()
    selected = null
    // Without a drawing tool, or outside the selection, the press goes on to the overlay.
    if (!drawing || !contains(selection, at(e))) return
    e.stopPropagation()
    start = at(e)
    container.setPointerCapture?.(e.pointerId)
  }

  function onpointermove(e: PointerEvent) {
    if (!selection) return
    if (start && tool) {
      board?.preview(draw(tool, 'draft', start, clampTo(at(e), selection), style))
    }
    // Over the selection: crosshair to draw, else move it. Elsewhere, the overlay's cursor.
    cursor = !contains(selection, at(e)) ? '' : drawing ? 'crosshair' : 'move'
  }

  function onpointerup(e: PointerEvent) {
    if (!start || !tool || !selection) return
    const shape = draw(tool, `s${++ids}`, start, clampTo(at(e), selection), style)
    cancel()
    if (shape) set([...history.present, shape])
  }

  const typing = (t: EventTarget | null) =>
    t instanceof HTMLElement && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))

  /** Handles an annotation key. Returns whether it did, so the overlay skips it. */
  export function key(e: KeyboardEvent): boolean {
    if (!selection || menuOpen() || typing(e.target)) return false
    const redoing = shortcut(e, 'z', true)
    if (redoing || shortcut(e, 'z')) {
      e.preventDefault()
      move(redoing ? redo(history) : undo(history))
      return true
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return false
    const shape = history.present.find((s) => s.id === selected)
    if (shape && (e.key === 'Backspace' || e.key === 'Delete')) {
      e.preventDefault()
      set(without(history.present, shape.id))
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
    const next = toolForKey(e.key)
    if (!next || !READY.has(next) || failed) return false
    e.preventDefault()
    tool = next
    return true
  }

  /**
   * Esc, one step at a time: the shape being drawn, an open menu, the selected shape, then the
   * tool. Returns whether it undid anything; if not, the overlay handles Esc.
   */
  export function escape(): boolean {
    if (start) cancel()
    else if (toolbar?.closeMenus()) return true
    else if (selected) selected = null
    else if (tool) tool = null
    else return false
    return true
  }

  /**
   * The annotations alone at native size, for Rust to composite. `crop` is the physical rect
   * being captured; `ratio` is physical px per logical px on each axis. See
   * docs/annotations.md.
   */
  export function exportAnnotations(crop: Rect, ratio: Scale): Promise<AnnotationExport> {
    return exportShapes(history.present, crop, ratio)
  }

  const toolbarAt = $derived(selection && toolbarPosition(selection, bar, display))
</script>

<div
  class="layer"
  style:width="{selection ? display.width : 0}px"
  style:height="{selection ? display.height : 0}px"
  style:cursor
  bind:this={container}
  data-testid="annotations"
  role="presentation"
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={cancel}
></div>

{#if Toolbar && toolbarAt}
  <div
    class="toolbar"
    style:left="{toolbarAt.x}px"
    style:top="{toolbarAt.y}px"
    bind:offsetWidth={bar.width}
    bind:offsetHeight={bar.height}
    onpointerdown={(e) => e.stopPropagation()}
    role="presentation"
  >
    <Toolbar
      bind:this={toolbar}
      {tool}
      ontool={(t) => (tool = t)}
      bind:style
      drawable={!failed}
      canUndo={history.past.length > 0}
      canRedo={history.future.length > 0}
      canClear={history.present.length > 0}
      onundo={() => move(undo(history))}
      onredo={() => move(redo(history))}
      onclear={() => set([])}
      {options}
      {actions}
    />
  </div>
{/if}

<style>
  /* The whole display while there is a selection; presses it does not use go on to the overlay. */
  .layer {
    position: fixed;
    left: 0;
    top: 0;
  }

  /* Above the selection outline and the size badge. */
  .toolbar {
    position: fixed;
    z-index: 1;
    cursor: default;
  }
</style>
