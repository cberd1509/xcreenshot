<script lang="ts">
  import ArrowIcon from '@lucide/svelte/icons/arrow-up-right'
  import BlurIcon from '@lucide/svelte/icons/eye-off'
  import EllipseIcon from '@lucide/svelte/icons/ellipse'
  import HighlighterIcon from '@lucide/svelte/icons/highlighter'
  import SelectIcon from '@lucide/svelte/icons/mouse-pointer-2'
  import PenIcon from '@lucide/svelte/icons/pen-line'
  import RectIcon from '@lucide/svelte/icons/rectangle-horizontal'
  import RedoIcon from '@lucide/svelte/icons/redo-2'
  import LineIcon from '@lucide/svelte/icons/slash'
  import ClearIcon from '@lucide/svelte/icons/trash-2'
  import TextIcon from '@lucide/svelte/icons/type'
  import UndoIcon from '@lucide/svelte/icons/undo-2'
  import { ToggleGroup } from 'bits-ui'
  import type { Component } from 'svelte'
  import { BUTTON, DIVIDER, PANEL } from '../panel'
  import ColorMenu from './ColorMenu.svelte'
  import { READY, SELECT_KEY, TOOLS, type Style, type Tool } from './shapes'
  import type { ShownTextOptions, TextOptions } from './text'
  import TextOptionsBar from './TextOptions.svelte'
  import Tip from './Tip.svelte'

  /** The vertical tool rail: Select and the drawing tools, the color menu and history. */
  let {
    tool,
    ontool,
    style = $bindable(),
    colorsOpen = $bindable(false),
    text = null,
    ontext,
    drawable,
    keepFocus,
    canUndo,
    canRedo,
    canClear,
    onundo,
    onredo,
    onclear,
    mod,
    shift,
  }: {
    tool: Tool | null
    ontool: (tool: Tool | null) => void
    style: Style
    colorsOpen?: boolean
    text?: ShownTextOptions | null
    ontext: (change: Partial<TextOptions>) => void
    drawable: boolean
    keepFocus: boolean
    canUndo: boolean
    canRedo: boolean
    canClear: boolean
    onundo: () => void
    onredo: () => void
    onclear: () => void
    /** How the platform writes ⌘ and ⇧ in a shortcut. */
    mod: string
    shift: string
  } = $props()

  const ICONS: Record<Tool, Component> = {
    pen: PenIcon,
    line: LineIcon,
    arrow: ArrowIcon,
    rect: RectIcon,
    ellipse: EllipseIcon,
    highlighter: HighlighterIcon,
    text: TextIcon,
    blur: BlurIcon,
  }
  /** The group's value for no drawing tool. */
  const SELECT = 'select'
</script>

{#snippet action(
  label: string,
  keys: string | undefined,
  Icon: Component,
  onclick: () => void,
  disabled: boolean,
)}
  {#snippet button(props: Record<string, unknown>)}
    <button {...props} type="button" class={BUTTON} aria-label={label} {disabled} {onclick}>
      <Icon />
    </button>
  {/snippet}
  <Tip {label} {keys} trigger={button} />
{/snippet}

{#snippet select(props: Record<string, unknown>)}
  <ToggleGroup.Item {...props} value={SELECT} aria-label="Select" class={BUTTON}>
    <SelectIcon />
  </ToggleGroup.Item>
{/snippet}

<div
  class="flex w-12 flex-col gap-0.5 p-1.5 {PANEL}"
  role="toolbar"
  aria-label="Annotation tools"
  aria-orientation="vertical"
  tabindex="-1"
  onmousedown={(e) => keepFocus && e.preventDefault()}
>
  <ToggleGroup.Root
    type="single"
    orientation="vertical"
    aria-label="Tools"
    class="flex flex-col gap-0.5"
    value={tool ?? SELECT}
    onValueChange={(v) => ontool(v && v !== SELECT ? (v as Tool) : null)}
  >
    <Tip label="Select and move" keys={SELECT_KEY} trigger={select} />
    {#each TOOLS as t (t.tool)}
      {@const Icon = ICONS[t.tool]}
      {#snippet item(props: Record<string, unknown>)}
        <ToggleGroup.Item
          {...props}
          value={t.tool}
          aria-label={t.label}
          class={BUTTON}
          disabled={!drawable || !READY.has(t.tool)}
        >
          <Icon />
        </ToggleGroup.Item>
      {/snippet}
      <Tip label={t.tip ?? t.label} keys={t.key} trigger={item} />
    {/each}
  </ToggleGroup.Root>

  {#if text}
    <TextOptionsBar options={text} onchange={ontext} />
  {/if}

  <div class={DIVIDER}></div>

  <ColorMenu bind:style bind:open={colorsOpen} {keepFocus} />
  {@render action('Undo', `${mod}Z`, UndoIcon, onundo, !canUndo)}
  {@render action('Redo', `${shift}${mod}Z`, RedoIcon, onredo, !canRedo)}
  {@render action('Clear all', undefined, ClearIcon, onclear, !canClear)}
</div>
