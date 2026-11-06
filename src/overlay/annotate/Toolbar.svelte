<script lang="ts" module>
  import type { ToggleKey } from '$lib/settings.svelte'

  /** Options in the ⋯ menu. */
  export type Option = Extract<ToggleKey, 'rememberRegion' | 'magnifier'>

  /** What the toolbar's output buttons and menu do. Wired by the overlay. */
  export type ToolbarActions = {
    copy: () => void
    save: () => void
    share: () => void
    option: (key: Option, on: boolean) => void
  }
</script>

<script lang="ts">
  import ArrowIcon from '@lucide/svelte/icons/arrow-up-right'
  import EllipseIcon from '@lucide/svelte/icons/circle'
  import CopyIcon from '@lucide/svelte/icons/copy'
  import SaveIcon from '@lucide/svelte/icons/download'
  import MoreIcon from '@lucide/svelte/icons/ellipsis'
  import BlurIcon from '@lucide/svelte/icons/eye-off'
  import HighlighterIcon from '@lucide/svelte/icons/highlighter'
  import PenIcon from '@lucide/svelte/icons/pencil'
  import RedoIcon from '@lucide/svelte/icons/redo-2'
  import ShareIcon from '@lucide/svelte/icons/share'
  import LineIcon from '@lucide/svelte/icons/slash'
  import RectIcon from '@lucide/svelte/icons/square'
  import ClearIcon from '@lucide/svelte/icons/trash-2'
  import TextIcon from '@lucide/svelte/icons/type'
  import UndoIcon from '@lucide/svelte/icons/undo-2'
  import type { Component, Snippet } from 'svelte'
  import { Button } from '$lib/components/ui/button'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { Kbd } from '$lib/components/ui/kbd'
  import * as Popover from '$lib/components/ui/popover'
  import { Separator } from '$lib/components/ui/separator'
  import { Toggle } from '$lib/components/ui/toggle'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { isMac } from '$lib/settings.svelte'
  import { COLORS, READY, SIZES, TOOLS, type Style, type Tool } from './shapes'

  let {
    tool,
    ontool,
    style = $bindable(),
    canUndo,
    canRedo,
    canClear,
    onundo,
    onredo,
    onclear,
    options,
    actions,
    mac = isMac,
  }: {
    tool: Tool | null
    ontool: (tool: Tool | null) => void
    style: Style
    canUndo: boolean
    canRedo: boolean
    canClear: boolean
    onundo: () => void
    onredo: () => void
    onclear: () => void
    options: Record<Option, boolean>
    actions: ToolbarActions
    mac?: boolean
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
  const SIZE_LABELS = ['Thin', 'Medium', 'Thick']
  const mod = $derived(mac ? '⌘' : 'Ctrl+')
  const shift = $derived(mac ? '⇧' : 'Shift+')

  // Keys that would also act on the overlay (Enter copies, arrows nudge) stay in the toolbar.
  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ' || e.key.startsWith('Arrow')) e.stopPropagation()
  }
</script>

{#snippet tip(trigger: Snippet<[Record<string, unknown>]>, label: string, keys?: string)}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        {@render trigger(props)}
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content side="bottom">
      {label}
      {#if keys}<Kbd>{keys}</Kbd>{/if}
    </Tooltip.Content>
  </Tooltip.Root>
{/snippet}

{#snippet action(
  label: string,
  keys: string | undefined,
  Icon: Component,
  onclick: () => void,
  disabled = false,
)}
  {#snippet button(props: Record<string, unknown>)}
    <Button {...props} variant="ghost" size="icon-sm" aria-label={label} {disabled} {onclick}>
      <Icon />
    </Button>
  {/snippet}
  {@render tip(button, label, keys)}
{/snippet}

{#snippet colorButton(props: Record<string, unknown>)}
  <Popover.Trigger {...props}>
    {#snippet child({ props: inner })}
      <Button {...inner} variant="ghost" size="icon-sm" aria-label="Color and size">
        <span
          class="size-4 rounded-full border border-black/20"
          style:background={style.color}
          data-testid="current-color"
        ></span>
      </Button>
    {/snippet}
  </Popover.Trigger>
{/snippet}

{#snippet moreButton(props: Record<string, unknown>)}
  <DropdownMenu.Trigger {...props}>
    {#snippet child({ props: inner })}
      <Button {...inner} variant="ghost" size="icon-sm" aria-label="More">
        <MoreIcon />
      </Button>
    {/snippet}
  </DropdownMenu.Trigger>
{/snippet}

<Tooltip.Provider delayDuration={400}>
  <div
    class="flex items-center gap-0.5 rounded-lg bg-popover/95 p-1 text-popover-foreground shadow-md"
    role="toolbar"
    aria-label="Annotate"
    tabindex="-1"
    {onkeydown}
  >
    {#each TOOLS as t (t.tool)}
      {@const Icon = ICONS[t.tool]}
      {#snippet toggle(props: Record<string, unknown>)}
        <Toggle
          {...props}
          size="sm"
          aria-label={t.label}
          disabled={!READY.has(t.tool)}
          pressed={tool === t.tool}
          onPressedChange={(on) => ontool(on ? t.tool : null)}
        >
          <Icon />
        </Toggle>
      {/snippet}
      {@render tip(toggle, t.label, t.key)}
    {/each}

    <Popover.Root>
      {@render tip(colorButton, 'Color and size')}
      <Popover.Content class="w-auto p-2" side="bottom" {onkeydown}>
        <div class="flex gap-1">
          {#each COLORS as color (color)}
            <button
              type="button"
              class="size-6 rounded-full border border-black/20 aria-pressed:ring-2 aria-pressed:ring-ring"
              style:background={color}
              aria-label="Color {color}"
              aria-pressed={style.color === color}
              onclick={() => (style = { ...style, color })}
            ></button>
          {/each}
        </div>
        <div class="mt-2 flex gap-1">
          {#each SIZES as size, i (size)}
            <Toggle
              size="sm"
              aria-label={SIZE_LABELS[i]}
              pressed={style.size === size}
              onPressedChange={() => (style = { ...style, size })}
            >
              <span
                class="rounded-full bg-current"
                style:width="{size + 2}px"
                style:height="{size + 2}px"
              ></span>
            </Toggle>
          {/each}
        </div>
      </Popover.Content>
    </Popover.Root>

    <Separator orientation="vertical" class="mx-1 h-5" />

    {@render action('Undo', `${mod}Z`, UndoIcon, onundo, !canUndo)}
    {@render action('Redo', `${shift}${mod}Z`, RedoIcon, onredo, !canRedo)}
    {@render action('Clear all', undefined, ClearIcon, onclear, !canClear)}

    <DropdownMenu.Root>
      {@render tip(moreButton, 'More')}
      <DropdownMenu.Content side="bottom" align="end" {onkeydown}>
        <DropdownMenu.CheckboxItem
          checked={options.rememberRegion}
          onCheckedChange={(on) => actions.option('rememberRegion', on)}
        >
          Remember region
        </DropdownMenu.CheckboxItem>
        <DropdownMenu.CheckboxItem
          checked={options.magnifier}
          onCheckedChange={(on) => actions.option('magnifier', on)}
        >
          Magnifier
        </DropdownMenu.CheckboxItem>
      </DropdownMenu.Content>
    </DropdownMenu.Root>

    <Separator orientation="vertical" class="mx-1 h-5" />

    {@render action('Copy', `${mod}C`, CopyIcon, actions.copy)}
    {@render action('Save', `${mod}S`, SaveIcon, actions.save)}
    <Button variant="ghost" size="sm" onclick={actions.share}>
      <ShareIcon />
      Share
    </Button>
  </div>
</Tooltip.Provider>
