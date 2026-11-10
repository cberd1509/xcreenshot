<script lang="ts" module>
  import type { ToggleKey } from '$lib/settings.svelte'
  import type { Rect } from '../geometry'
  import type { Factor } from './upscale'

  /** Options in the ⋯ menu. */
  export type Option = Extract<ToggleKey, 'rememberRegion' | 'magnifier'>

  /** What the action bar's buttons and menu do. Wired by the overlay. */
  export type ToolbarActions = {
    /** Copies at 1×, or at a larger size picked under the Copy caret. */
    copy: (factor?: Factor) => void
    /** Saves at 1×, or at a larger size picked under the Save caret. */
    save: (factor?: Factor) => void
    /** Opens the share sheet under `anchor`, the Share button in logical px in the page. */
    share: (anchor: Rect) => void
    /** Closes the overlay, as Esc does once nothing is left to undo. */
    cancel: () => void
    option: (key: Option, on: boolean) => void
  }
</script>

<script lang="ts">
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { isMac } from '$lib/settings.svelte'
  import type { Size } from '../geometry'
  import { keyLabels } from '../keys'
  import ActionBar from './ActionBar.svelte'
  import { railPosition, toolbarPosition } from './placement'
  import Rail from './Rail.svelte'
  import type { Style, Tool } from './shapes'
  import type { ShownTextOptions, TextOptions } from './text'

  /**
   * The tool rail right of the selection and the action bar below it, each kept on the display.
   * Loaded lazily with Konva, so the overlay's own bundle stays small.
   */
  let {
    selection,
    display,
    size = null,
    tool,
    ontool,
    style = $bindable(),
    text = null,
    ontext = () => {},
    drawable = true,
    keepFocus = false,
    canUndo,
    canRedo,
    canClear,
    onundo,
    onredo,
    onclear,
    options,
    actions,
    mac = isMac,
    canShare = isMac,
  }: {
    /** The selection, in logical px on the display. */
    selection: Rect
    display: Size
    /** The selection in physical px, for the larger sizes under Copy and Save. */
    size?: Size | null
    tool: Tool | null
    ontool: (tool: Tool | null) => void
    style: Style
    /** The text tool's options, shown while it is in use; `null` hides them. */
    text?: ShownTextOptions | null
    ontext?: (change: Partial<TextOptions>) => void
    /**
     * While text is typed, presses on the bars and their menus leave focus in the text's
     * editor, so it stays the active edit and follows the change.
     */
    keepFocus?: boolean
    /** False when the drawing layer could not load: the tools are shown as unavailable. */
    drawable?: boolean
    canUndo: boolean
    canRedo: boolean
    canClear: boolean
    onundo: () => void
    onredo: () => void
    onclear: () => void
    options: Record<Option, boolean>
    actions: ToolbarActions
    /** macOS: keys show as ⌘ and ⇧. */
    mac?: boolean
    /** Whether there is a share sheet: macOS only until Windows gets one (#30). */
    canShare?: boolean
  } = $props()

  const { mod, shift } = $derived(keyLabels(mac))

  let colorsOpen = $state(false)
  let moreOpen = $state(false)
  let copyOpen = $state(false)
  let saveOpen = $state(false)
  let rail = $state<Size>({ width: 0, height: 0 })
  let bar = $state<Size>({ width: 0, height: 0 })
  const railAt = $derived(railPosition(selection, rail, display))
  const barAt = $derived(toolbarPosition(selection, bar, display))

  /** Closes the color, ⋯, Copy or Save menu. Returns whether one was open (for Esc). */
  export function closeMenus(): boolean {
    const open = menuOpen()
    colorsOpen = false
    moreOpen = false
    copyOpen = false
    saveOpen = false
    return open
  }

  /** Whether the color, ⋯, Copy or Save menu is open: it owns the keyboard meanwhile. */
  export function menuOpen(): boolean {
    return colorsOpen || moreOpen || copyOpen || saveOpen
  }

  const stop = (e: PointerEvent) => e.stopPropagation()
</script>

<Tooltip.Provider delayDuration={400}>
  <div
    class="bar"
    style:left="{railAt.x}px"
    style:top="{railAt.y}px"
    bind:offsetWidth={rail.width}
    bind:offsetHeight={rail.height}
    onpointerdown={stop}
    role="presentation"
  >
    <Rail
      {tool}
      {ontool}
      bind:style
      bind:colorsOpen
      {text}
      {ontext}
      {drawable}
      {keepFocus}
      {canUndo}
      {canRedo}
      {canClear}
      {onundo}
      {onredo}
      {onclear}
      {mod}
      {shift}
    />
  </div>
  <div
    class="bar"
    style:left="{barAt.x}px"
    style:top="{barAt.y}px"
    bind:offsetWidth={bar.width}
    bind:offsetHeight={bar.height}
    onpointerdown={stop}
    role="presentation"
  >
    <ActionBar
      {actions}
      {options}
      {size}
      bind:moreOpen
      bind:copyOpen
      bind:saveOpen
      {keepFocus}
      {mod}
      {canShare}
    />
  </div>
</Tooltip.Provider>

<style>
  /* Above the selection outline and the size badge. */
  .bar {
    position: fixed;
    z-index: 1;
    cursor: default;
  }
</style>
