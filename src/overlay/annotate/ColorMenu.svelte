<script lang="ts">
  import { ToggleGroup } from 'bits-ui'
  import * as Popover from '$lib/components/ui/popover'
  import { PANEL } from '../panel'
  import { COLORS, SIZES, parseHex, type Style } from './shapes'
  import Tip from './Tip.svelte'

  /** The rail's color button and its menu: swatches, a hex field and the stroke size. */
  let {
    style = $bindable(),
    open = $bindable(false),
    keepFocus = false,
  }: {
    style: Style
    open?: boolean
    /** Presses leave focus in the text being typed (see `Toolbar`). */
    keepFocus?: boolean
  } = $props()

  const SIZE_LABELS = ['Thin', 'Medium', 'Thick']
  /** Each size's bar in the menu, in px: thin, medium, thick. */
  const BARS = [2, 4, 7]

  const keep = (e: Event) => keepFocus && e.preventDefault()

  /** What the hex field shows while it is typed in; otherwise the current color. */
  let draft = $state<string | null>(null)
  /** What had focus when the hex field took it while `keepFocus`, to give it back. */
  let refocus: HTMLElement | null = null

  function onfocus(e: FocusEvent) {
    const from = e.relatedTarget
    refocus = keepFocus && from instanceof HTMLElement ? from : null
  }

  function oninput(e: Event & { currentTarget: HTMLInputElement }) {
    draft = e.currentTarget.value
    const color = parseHex(draft)
    if (color) style = { ...style, color }
  }

  /** Enter keeps the color and gives focus back to the text being typed, if any. */
  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'Enter') return
    e.preventDefault()
    draft = null
    refocus?.focus()
    refocus = null
  }
</script>

{#snippet button(props: Record<string, unknown>)}
  <Popover.Trigger {...props}>
    {#snippet child({ props: inner })}
      <button
        {...inner}
        type="button"
        class="flex size-9 items-center justify-center rounded-lg hover:bg-zinc-800 aria-expanded:bg-zinc-800"
        aria-label="Color and size"
      >
        <span
          class="size-[18px] rounded-full shadow-[0_0_0_2px_#18181b,0_0_0_3px_#71717a]"
          style:background={style.color}
          data-testid="current-color"
        ></span>
      </button>
    {/snippet}
  </Popover.Trigger>
{/snippet}

<Popover.Root bind:open>
  <Tip label="Color and size" trigger={button} />
  <Popover.Content
    side="right"
    align="start"
    sideOffset={17}
    alignOffset={-2}
    class="w-[216px] gap-3 p-3.5 {PANEL}"
    escapeKeydownBehavior="ignore"
    onmousedown={keep}
    onOpenAutoFocus={keep}
    onCloseAutoFocus={keep}
    trapFocus={!keepFocus}
  >
    <div class="text-xs font-medium text-zinc-400">Color</div>
    <ToggleGroup.Root
      type="single"
      aria-label="Color"
      class="grid grid-cols-4 gap-2"
      value={style.color}
      onValueChange={(color) => color && (style = { ...style, color })}
    >
      {#each COLORS as { color, name } (color)}
        <ToggleGroup.Item
          value={color}
          aria-label={name}
          class="h-8 rounded-lg data-[state=on]:shadow-[0_0_0_2px_#18181b,0_0_0_4px_#fafafa] {name ===
            'White' || name === 'Black'
            ? 'border border-zinc-700'
            : ''}"
          style="background: {color}"
        />
      {/each}
    </ToggleGroup.Root>
    <!-- In the page, not the system color panel: that would open under the overlay. -->
    <!-- Not `keep`: its press must go through. `data-keeps-text` keeps text open. -->
    <div class="flex items-center gap-2">
      <label for="hex-color" class="text-xs text-zinc-400">Hex</label>
      <input
        id="hex-color"
        class="h-8 min-w-0 grow rounded-lg border border-zinc-700 bg-zinc-950 px-2.5 font-mono text-xs text-zinc-50 uppercase outline-none focus:border-(--selection)"
        spellcheck="false"
        autocomplete="off"
        maxlength="7"
        data-keeps-text
        value={draft ?? style.color}
        onmousedown={(e) => e.stopPropagation()}
        {onfocus}
        {oninput}
        {onkeydown}
        onblur={() => (draft = null)}
      />
    </div>
    <div class="text-xs font-medium text-zinc-400">Size</div>
    <ToggleGroup.Root
      type="single"
      aria-label="Size"
      class="grid grid-cols-3 gap-1.5 rounded-[9px] bg-zinc-950 p-[3px]"
      value={String(style.size)}
      onValueChange={(size) => size && (style = { ...style, size: Number(size) })}
    >
      {#each SIZES as size, i (size)}
        <ToggleGroup.Item
          value={String(size)}
          aria-label={SIZE_LABELS[i]}
          class="group flex h-[30px] items-center justify-center rounded-md data-[state=on]:bg-zinc-700"
        >
          <span
            class="w-[22px] rounded-full bg-zinc-400 group-data-[state=on]:bg-zinc-50"
            style:height="{BARS[i]}px"
          ></span>
        </ToggleGroup.Item>
      {/each}
    </ToggleGroup.Root>
  </Popover.Content>
</Popover.Root>
