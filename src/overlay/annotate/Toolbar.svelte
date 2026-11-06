<script lang="ts" module>
  import type { ToggleKey } from '$lib/settings.svelte'

  /** Options in the ⋯ menu. */
  export type Option = Extract<ToggleKey, 'rememberRegion' | 'magnifier'>

  /** What the toolbar's output buttons and menu do. Wired by the overlay. */
  export type ToolbarActions = {
    copy: () => void
    save: () => void
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
  import * as ToggleGroup from '$lib/components/ui/toggle-group'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { isMac } from '$lib/settings.svelte'
  import { COLORS, READY, SIZES, TOOLS, type Style, type Tool } from './shapes'

  let {
    tool,
    ontool,
    style = $bindable(),
    drawable = true,
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

  let colorsOpen = $state(false)
  let moreOpen = $state(false)

  /** Closes the color or ⋯ menu. Returns whether one was open (for Esc). */
  export function closeMenus(): boolean {
    const open = colorsOpen || moreOpen
    colorsOpen = false
    moreOpen = false
    return open
  }

  /** Whether the color or ⋯ menu is open: it owns the keyboard meanwhile. */
  export function menuOpen(): boolean {
    return colorsOpen || moreOpen
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
  >
    <ToggleGroup.Root
      type="single"
      size="sm"
      spacing={1}
      aria-label="Tools"
      value={tool ?? ''}
      onValueChange={(v) => ontool((v || null) as Tool | null)}
    >
      {#each TOOLS as t (t.tool)}
        {@const Icon = ICONS[t.tool]}
        {#snippet item(props: Record<string, unknown>)}
          <ToggleGroup.Item
            {...props}
            value={t.tool}
            aria-label={t.label}
            disabled={!drawable || !READY.has(t.tool)}
          >
            <Icon />
          </ToggleGroup.Item>
        {/snippet}
        {@render tip(item, t.label, t.key)}
      {/each}
    </ToggleGroup.Root>

    <Popover.Root bind:open={colorsOpen}>
      {@render tip(colorButton, 'Color and size')}
      <Popover.Content class="w-auto p-2" side="bottom" escapeKeydownBehavior="ignore">
        <ToggleGroup.Root
          type="single"
          spacing={1}
          aria-label="Color"
          value={style.color}
          onValueChange={(color) => color && (style = { ...style, color })}
        >
          {#each COLORS as color (color)}
            <ToggleGroup.Item value={color} aria-label="Color {color}" class="size-7 min-w-7 p-0">
              <span class="size-5 rounded-full border border-black/20" style:background={color}
              ></span>
            </ToggleGroup.Item>
          {/each}
        </ToggleGroup.Root>
        <ToggleGroup.Root
          type="single"
          size="sm"
          spacing={1}
          class="mt-2"
          aria-label="Size"
          value={String(style.size)}
          onValueChange={(size) => size && (style = { ...style, size: Number(size) })}
        >
          {#each SIZES as size, i (size)}
            <ToggleGroup.Item value={String(size)} aria-label={SIZE_LABELS[i]}>
              <span
                class="rounded-full bg-current"
                style:width="{size + 2}px"
                style:height="{size + 2}px"
              ></span>
            </ToggleGroup.Item>
          {/each}
        </ToggleGroup.Root>
      </Popover.Content>
    </Popover.Root>

    <Separator orientation="vertical" class="mx-1 h-5" />

    {@render action('Undo', `${mod}Z`, UndoIcon, onundo, !canUndo)}
    {@render action('Redo', `${shift}${mod}Z`, RedoIcon, onredo, !canRedo)}
    {@render action('Clear all', undefined, ClearIcon, onclear, !canClear)}

    <DropdownMenu.Root bind:open={moreOpen}>
      {@render tip(moreButton, 'More')}
      <DropdownMenu.Content side="bottom" align="end" escapeKeydownBehavior="ignore">
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
    <!-- Share comes in #19. -->
    <Button variant="ghost" size="sm" disabled>
      <ShareIcon />
      Share
    </Button>
  </div>
</Tooltip.Provider>
