<script lang="ts">
  import ZapIcon from '@lucide/svelte/icons/zap'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { BAR } from './panel'

  /** Shown instead of the key hint while quick copy is on: the drag copies at once. */
  let {
    annotate,
    turnOff,
  }: {
    /** This capture only: the toolbar after the drag, as usual. */
    annotate: () => void
    /** Quick copy off for good; this capture goes on as usual. */
    turnOff: () => void
  } = $props()

  const BUTTON =
    'h-[30px] rounded-md bg-zinc-800 text-[13px] font-medium text-zinc-50 hover:bg-zinc-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-(--selection)'
</script>

<div
  class={BAR}
  role="toolbar"
  aria-label="Quick copy"
  tabindex="-1"
  onpointerdown={(e) => e.stopPropagation()}
>
  <ZapIcon class="size-4 text-zinc-50" aria-hidden="true" />
  <span
    ><b class="font-semibold text-zinc-50">Quick copy</b> · drag or double-click a window to copy</span
  >
  <div class="h-5 w-px bg-zinc-700"></div>
  <button type="button" class="{BUTTON} px-3" onclick={annotate}>Annotate this time</button>
  <Tooltip.Provider delayDuration={300}>
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <button {...props} type="button" class="{BUTTON} px-2.5" onclick={turnOff}>
            Turn off
          </button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content
        side="top"
        sideOffset={10}
        arrowClasses="hidden"
        class="rounded-md bg-zinc-50 px-2.5 py-1.5 text-xs font-medium text-zinc-950 shadow-[0_8px_20px_rgb(0_0_0/0.3)]"
      >
        You can turn it back on in Settings.
      </Tooltip.Content>
    </Tooltip.Root>
  </Tooltip.Provider>
  <div class="h-5 w-px bg-zinc-700"></div>
  <span class="pr-2"><b class="font-semibold text-zinc-200">Esc</b> cancel</span>
</div>
