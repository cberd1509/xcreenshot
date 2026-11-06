<script lang="ts">
  import type { Snippet } from 'svelte'
  import * as Tooltip from '$lib/components/ui/tooltip'

  /** A tooltip on the overlay's bars: a white pill with the label and, if any, its key. */
  let {
    label,
    keys,
    side = 'right',
    sideOffset = 15,
    trigger,
  }: {
    label: string
    keys?: string
    side?: 'right' | 'top' | 'bottom'
    sideOffset?: number
    /** The control, given the trigger's props. */
    trigger: Snippet<[Record<string, unknown>]>
  } = $props()
</script>

<Tooltip.Root>
  <Tooltip.Trigger>
    {#snippet child({ props })}
      {@render trigger(props)}
    {/snippet}
  </Tooltip.Trigger>
  <Tooltip.Content
    {side}
    {sideOffset}
    arrowClasses="hidden"
    class="h-7 gap-2 rounded-md bg-zinc-50 px-2.5 py-0 text-xs font-medium text-zinc-950 shadow-[0_4px_12px_rgb(0_0_0/0.3)] has-data-[slot=kbd]:pr-2.5"
  >
    {label}
    {#if keys}
      <kbd class="rounded bg-zinc-200 px-[5px] font-sans text-[11px] font-normal text-zinc-600"
        >{keys}</kbd
      >
    {/if}
  </Tooltip.Content>
</Tooltip.Root>
