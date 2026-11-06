<script lang="ts">
  import { untrack } from 'svelte'
  import { settings } from '$lib/settings.svelte'
  import { badgePosition, type Point, type Rect, type Size } from '../geometry'
  import { Board, exportShapes, loadKonva, type AnnotationExport } from './board'
  import { clear, empty, redo, remove, run, undo, update } from './history'
  import { toolbarPosition } from './placement'
  import { DEFAULT_STYLE, READY, clampTo, draw, toolForKey, type Shape, type Tool } from './shapes'
  import Toolbar, { type ToolbarActions } from './Toolbar.svelte'

  /**
   * Annotations on the selection, and the toolbar next to it. `badge` is the size badge's size,
   * so the toolbar keeps clear of it. Konva loads once there is a selection.
   */
  let {
    selection,
    display,
    badge,
    actions: given = {},
  }: {
    selection: Rect | null
    display: Size
    badge: Size
    actions?: Partial<ToolbarActions>
  } = $props()

  const actions: ToolbarActions = $derived({
    copy: () => {},
    save: () => {},
    share: () => {},
    // Kept for this run only; saving settings comes in #20.
    option: (key, on) => (settings[key] = on),
    ...given,
  })

  let container: HTMLDivElement = $state()!
  let board = $state.raw<Board | null>(null)
  let loading = false
  let history = $state.raw(empty)
  let tool = $state<Tool | null>(null)
  let style = $state(DEFAULT_STYLE)
  let selected = $state<string | null>(null)
  let draft = $state.raw<Shape | null>(null)
  let start: Point | null = null
  let ids = 0
  let bar = $state<Size>({ width: 0, height: 0 })

  // Lazy: Konva is fetched and the stage built on the first selection, not when the overlay
  // shows, so the frozen frame paints as fast as before.
  $effect(() => {
    if (!selection || board || loading) return
    loading = true
    loadKonva()
      .then((K) => {
        board = new Board(
          K,
          container,
          untrack(() => display),
          {
            onchange: (shape) => (history = update(history, shape)),
            onselect: (id) => (selected = id),
          },
        )
      })
      .catch((e) => console.error('could not load the annotation layer', e))
  })
  $effect(() => () => board?.destroy())

  // A new selection (or session) starts with no annotations.
  $effect(() => {
    if (selection) return
    untrack(() => {
      history = empty
      selected = null
      draft = null
      tool = null
      start = null
    })
  })

  $effect(() => board?.resize(display))
  $effect(() => {
    if (selection) board?.place(selection)
  })
  $effect(() => board?.render(history.shapes, draft, selected))

  const toolbarAt = $derived.by(() => {
    if (!selection) return null
    const b = badgePosition(selection, badge, display)
    return toolbarPosition(selection, bar, display, { ...b, ...badge })
  })

  /** The pointer in the selection's own space, clamped to it. */
  const local = (e: PointerEvent): Point =>
    clampTo({ x: e.clientX - selection!.x, y: e.clientY - selection!.y }, selection!)

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0 || !selection || !board) return
    // On a shape or its handles: Konva moves or resizes it.
    if (board.hits({ x: e.clientX, y: e.clientY })) return e.stopPropagation()
    selected = null
    // With no drawing tool, the press goes on to the overlay, which moves the selection.
    if (!tool || !READY.has(tool)) return
    e.stopPropagation()
    start = local(e)
    container.setPointerCapture?.(e.pointerId)
  }

  function onpointermove(e: PointerEvent) {
    if (start && tool) draft = draw(tool, 'draft', start, local(e), style)
  }

  function onpointerup(e: PointerEvent) {
    if (!start || !tool) return
    const shape = draw(tool, `s${++ids}`, start, local(e), style)
    start = null
    draft = null
    if (shape) history = run(history, { kind: 'add', shape })
  }

  const typing = (t: EventTarget | null) =>
    t instanceof HTMLElement && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))

  function onkeydown(e: KeyboardEvent) {
    if (!selection || typing(e.target)) return
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z') {
      e.preventDefault()
      history = e.shiftKey ? redo(history) : undo(history)
      return
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return
    if ((e.key === 'Backspace' || e.key === 'Delete') && selected) {
      e.preventDefault()
      history = remove(history, selected)
      selected = null
      return
    }
    const next = toolForKey(e.key)
    if (next && READY.has(next)) {
      e.preventDefault()
      tool = next
    }
  }

  /**
   * The annotations alone at the crop's native size, for Rust to composite. `crop` is the
   * physical rect being captured; `scale` is physical px per logical px. See
   * docs/annotations.md.
   */
  export function exportAnnotations(crop: Rect, scale: number): Promise<AnnotationExport> {
    return exportShapes(history.shapes, crop, scale)
  }

  // Leaves the selection's edge free so its resize handles stay reachable.
  const EDGE = 4
  const clip = $derived(
    selection &&
      `inset(${selection.y + EDGE}px ${display.width - selection.x - selection.width + EDGE}px ` +
        `${display.height - selection.y - selection.height + EDGE}px ${selection.x + EDGE}px)`,
  )
</script>

<svelte:window {onkeydown} />

<div
  class="layer"
  class:drawing={tool && READY.has(tool)}
  style:clip-path={clip ?? 'inset(50%)'}
  style:pointer-events={selection ? 'auto' : 'none'}
  bind:this={container}
  data-testid="annotations"
  role="presentation"
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
></div>

{#if selection && toolbarAt}
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
      {tool}
      ontool={(t) => (tool = t)}
      bind:style
      canUndo={history.done.length > 0}
      canRedo={history.undone.length > 0}
      canClear={history.shapes.length > 0}
      onundo={() => (history = undo(history))}
      onredo={() => (history = redo(history))}
      onclear={() => (history = clear(history))}
      options={{ rememberRegion: settings.rememberRegion, magnifier: settings.magnifier }}
      {actions}
    />
  </div>
{/if}

<style>
  .layer {
    position: fixed;
    inset: 0;
    cursor: move;
  }

  .layer.drawing {
    cursor: crosshair;
  }

  .toolbar {
    position: fixed;
    cursor: default;
  }
</style>
