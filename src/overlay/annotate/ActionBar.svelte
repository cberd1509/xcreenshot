<script lang="ts">
  import CopyIcon from '@lucide/svelte/icons/copy'
  import SaveIcon from '@lucide/svelte/icons/download'
  import MoreIcon from '@lucide/svelte/icons/ellipsis'
  import ShareIcon from '@lucide/svelte/icons/share'
  import CancelIcon from '@lucide/svelte/icons/x'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { PANEL } from '../panel'
  import Tip from './Tip.svelte'
  import type { Option, ToolbarActions } from './Toolbar.svelte'

  /** The bar under the selection: options, Share, Save, Copy and Cancel. */
  let {
    actions,
    options,
    moreOpen = $bindable(false),
    keepFocus,
    mod,
  }: {
    actions: ToolbarActions
    options: Record<Option, boolean>
    moreOpen?: boolean
    keepFocus: boolean
    /** How the platform writes ⌘ in a shortcut. */
    mod: string
  } = $props()

  const keep = (e: Event) => keepFocus && e.preventDefault()

  const ICON = 'flex size-9 items-center justify-center rounded-lg [&_svg]:size-[18px]'
  const LABELED =
    'flex h-9 items-center gap-1.5 rounded-lg px-3 text-[13px] font-medium [&_svg]:size-4'
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

{#snippet share(props: Record<string, unknown>)}
  <!-- Share from the overlay comes in #68: shown, but not yet usable. -->
  <button
    {...props}
    type="button"
    class="{LABELED} cursor-default bg-zinc-800 text-zinc-50 opacity-50"
    aria-disabled="true"
  >
    <ShareIcon />Share
  </button>
{/snippet}

{#snippet save(props: Record<string, unknown>)}
  <button
    {...props}
    type="button"
    class="{LABELED} bg-zinc-800 text-zinc-50 hover:bg-zinc-700"
    onclick={actions.save}
  >
    <SaveIcon />Save
  </button>
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

  <Tip label="Coming soon" side="top" sideOffset={10} trigger={share} />
  <Tip label="Save" keys="{mod}S" side="top" sideOffset={10} trigger={save} />
  <button
    type="button"
    class="{LABELED} gap-2 bg-zinc-50 font-semibold text-zinc-950 hover:bg-white"
    aria-label="Copy"
    onclick={actions.copy}
  >
    <CopyIcon />Copy
    <kbd class="rounded bg-zinc-200 px-[5px] py-px font-sans text-[11px] font-normal text-zinc-600"
      >{mod}C</kbd
    >
  </button>
  <Tip label="Cancel" keys="Esc" side="top" sideOffset={10} trigger={cancel} />
</div>
