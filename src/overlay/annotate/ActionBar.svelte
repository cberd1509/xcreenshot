<script lang="ts">
  import CaretIcon from '@lucide/svelte/icons/chevron-down'
  import CopyIcon from '@lucide/svelte/icons/copy'
  import SaveIcon from '@lucide/svelte/icons/download'
  import MoreIcon from '@lucide/svelte/icons/ellipsis'
  import ShareIcon from '@lucide/svelte/icons/share'
  import CancelIcon from '@lucide/svelte/icons/x'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import type { Size } from '../geometry'
  import { KEY, PANEL } from '../panel'
  import Tip from './Tip.svelte'
  import type { Option, ToolbarActions } from './Toolbar.svelte'
  import { sizeOptions, type Factor } from './upscale'

  /**
   * The bar under the selection: options, Share, Save, Copy and Cancel. Save and Copy are split
   * buttons (artboard `Main.dc.html`, Tweak `menu`): the main part works at 1×, the caret opens
   * the larger sizes, and picking one saves or copies at that size right away.
   */
  let {
    actions,
    options,
    size = null,
    moreOpen = $bindable(false),
    copyOpen = $bindable(false),
    saveOpen = $bindable(false),
    keepFocus,
    mod,
    canShare,
  }: {
    actions: ToolbarActions
    options: Record<Option, boolean>
    /** The selection in physical px: each larger size shows what it gives. */
    size?: Size | null
    moreOpen?: boolean
    copyOpen?: boolean
    saveOpen?: boolean
    keepFocus: boolean
    /** How the platform writes ⌘ in a shortcut. */
    mod: string
    /** Whether the platform has a share sheet (macOS and Windows). */
    canShare: boolean
  } = $props()

  const keep = (e: Event) => keepFocus && e.preventDefault()

  /** The sheet opens under the button: its rect in the page. */
  function share(e: MouseEvent) {
    const { x, y, width, height } = (e.currentTarget as Element).getBoundingClientRect()
    actions.share({ x, y, width, height })
  }

  const sizes = $derived(size ? sizeOptions(size) : [])

  /** The split groups: their menus open under the whole group, not just the caret. */
  let saveGroup = $state<HTMLElement | null>(null)
  let copyGroup = $state<HTMLElement | null>(null)

  const ICON = 'flex size-9 items-center justify-center rounded-lg [&_svg]:size-[18px]'
  const LABELED =
    'flex h-9 items-center gap-1.5 rounded-lg px-3 text-[13px] font-medium [&_svg]:size-4'
  type Verb = 'Save' | 'Copy'
  /** The caret half of a split button, and its colours per button. */
  const CARET =
    'flex h-9 w-[26px] items-center justify-center rounded-l-none rounded-r-lg border-l [&_svg]:size-3.5'
  const CARET_COLORS: Record<Verb, string> = {
    Save: 'border-zinc-700 bg-zinc-800 text-zinc-400 hover:bg-zinc-700 aria-expanded:bg-zinc-700',
    Copy: 'border-zinc-300 bg-zinc-50 text-zinc-600 hover:bg-zinc-200 aria-expanded:bg-zinc-200',
  }
  /** The main part's right corners are the caret's, when there is one. */
  const split = $derived(sizes.length > 0 ? 'rounded-r-none' : '')
  const OPTIONS: { key: Option; label: string }[] = [
    { key: 'rememberRegion', label: 'Remember region' },
    { key: 'magnifier', label: 'Magnifier' },
  ]
</script>

{#snippet more(props: Record<string, unknown>)}
  <DropdownMenu.Trigger {...props}>
    {#snippet child({ props: inner })}
      <button
        {...inner}
        type="button"
        class="{ICON} text-zinc-400 hover:bg-zinc-800 hover:text-zinc-50 aria-expanded:bg-zinc-800 aria-expanded:text-zinc-50"
        aria-label="More"
      >
        <MoreIcon />
      </button>
    {/snippet}
  </DropdownMenu.Trigger>
{/snippet}

{#snippet shareButton(props: Record<string, unknown>)}
  <button
    {...props}
    type="button"
    class="{LABELED} bg-zinc-800 text-zinc-50 hover:bg-zinc-700"
    onclick={share}
  >
    <ShareIcon />Share
  </button>
{/snippet}

{#snippet save(props: Record<string, unknown>)}
  <button
    {...props}
    type="button"
    class="{LABELED} {split} bg-zinc-800 text-zinc-50 hover:bg-zinc-700"
    onclick={() => actions.save()}
  >
    <SaveIcon />Save
  </button>
{/snippet}

<!-- The caret half of a split button, coloured like its main part. -->
{#snippet caret(props: Record<string, unknown>, verb: Verb)}
  <DropdownMenu.Trigger {...props}>
    {#snippet child({ props: inner })}
      <button
        {...inner}
        type="button"
        class="{CARET} {CARET_COLORS[verb]}"
        aria-label="{verb} at a larger size"
      >
        <CaretIcon />
      </button>
    {/snippet}
  </DropdownMenu.Trigger>
{/snippet}

{#snippet saveCaret(props: Record<string, unknown>)}
  {@render caret(props, 'Save')}
{/snippet}

{#snippet copyCaret(props: Record<string, unknown>)}
  {@render caret(props, 'Copy')}
{/snippet}

<!-- The larger sizes under a caret: whole factors, each with the size it gives; one past the
     cap is disabled. -->
{#snippet larger(verb: Verb, anchor: HTMLElement | null, pick: (factor: Factor) => void)}
  <DropdownMenu.Content
    customAnchor={anchor}
    side="bottom"
    align="start"
    sideOffset={13}
    aria-label="{verb} larger"
    class="flex w-[230px] flex-col gap-0.5 p-1.5 {PANEL}"
    escapeKeydownBehavior="ignore"
    onmousedown={keep}
    onOpenAutoFocus={keep}
    onCloseAutoFocus={keep}
  >
    {#each sizes as { factor, width, height, enabled } (factor)}
      <DropdownMenu.Item
        disabled={!enabled}
        onSelect={() => pick(factor)}
        class="h-9 gap-2.5 rounded-lg px-2.5 text-[13px] font-medium text-zinc-200 focus:bg-zinc-800 focus:text-zinc-50"
      >
        <span class="grow">{verb} at {factor}×</span>
        <span class="font-mono text-[11px] text-zinc-500 tabular-nums">{width} × {height}</span>
      </DropdownMenu.Item>
    {/each}
  </DropdownMenu.Content>
{/snippet}

{#snippet cancel(props: Record<string, unknown>)}
  <button
    {...props}
    type="button"
    class="{ICON} text-zinc-400 hover:bg-zinc-800 hover:text-zinc-50"
    aria-label="Cancel"
    onclick={actions.cancel}
  >
    <CancelIcon />
  </button>
{/snippet}

<div
  class="flex h-12 items-center gap-1 p-1.5 {PANEL}"
  role="toolbar"
  aria-label="Capture actions"
  tabindex="-1"
  onmousedown={keep}
>
  <DropdownMenu.Root bind:open={moreOpen}>
    <Tip label="More" side="top" sideOffset={10} trigger={more} />
    <DropdownMenu.Content
      side="bottom"
      align="start"
      sideOffset={13}
      alignOffset={-7}
      class="flex w-[220px] flex-col gap-0.5 p-1.5 {PANEL}"
      escapeKeydownBehavior="ignore"
      onmousedown={keep}
      onOpenAutoFocus={keep}
      onCloseAutoFocus={keep}
    >
      {#each OPTIONS as { key, label } (key)}
        <DropdownMenu.CheckboxItem
          checked={options[key]}
          onCheckedChange={(on) => actions.option(key, on)}
          class="group h-9 gap-2.5 rounded-lg px-2.5 text-[13px] font-medium text-zinc-200 focus:bg-zinc-800 focus:text-zinc-50 [&>[data-slot=dropdown-menu-checkbox-item-indicator]]:hidden"
        >
          <span class="grow">{label}</span>
          <span
            class="flex h-[18px] w-8 rounded-full p-0.5 {options[key]
              ? 'justify-end bg-(--selection)'
              : 'justify-start bg-zinc-700'}"
            aria-hidden="true"
          >
            <span class="size-3.5 rounded-full {options[key] ? 'bg-white' : 'bg-zinc-400'}"></span>
          </span>
        </DropdownMenu.CheckboxItem>
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Root>

  <div class="mx-1 h-5 w-px bg-zinc-800"></div>

  {#if canShare}
    <Tip label="Share" side="top" sideOffset={10} trigger={shareButton} />
  {/if}
  <div class="flex" bind:this={saveGroup}>
    <Tip label="Save" keys="{mod}S" side="top" sideOffset={10} trigger={save} />
    {#if split}
      <DropdownMenu.Root bind:open={saveOpen}>
        <Tip label="Save larger" side="top" sideOffset={10} trigger={saveCaret} />
        {@render larger('Save', saveGroup, (factor) => actions.save(factor))}
      </DropdownMenu.Root>
    {/if}
  </div>
  <div class="flex" bind:this={copyGroup}>
    <button
      type="button"
      class="{LABELED} gap-2 {split} bg-zinc-50 font-semibold text-zinc-950 hover:bg-white"
      aria-label="Copy"
      onclick={() => actions.copy()}
    >
      <CopyIcon />Copy
      <kbd class="{KEY} py-px">{mod}C</kbd>
    </button>
    {#if split}
      <DropdownMenu.Root bind:open={copyOpen}>
        <Tip label="Copy larger" side="top" sideOffset={10} trigger={copyCaret} />
        {@render larger('Copy', copyGroup, (factor) => actions.copy(factor))}
      </DropdownMenu.Root>
    {/if}
  </div>
  <Tip label="Cancel" keys="Esc" side="top" sideOffset={10} trigger={cancel} />
</div>
