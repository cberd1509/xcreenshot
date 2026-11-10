<script lang="ts">
  import ZapIcon from '@lucide/svelte/icons/zap'
  import { BAR } from './panel'

  /**
   * Suggests quick copy after a few plain copies in a row (artboard `QuickCopyTip.dc.html`).
   * Shown instead of the key hint until the drag, in the Quick copy bar's shape.
   */
  let {
    turnOn,
    notNow,
  }: {
    /** Quick copy on, the Settings switch; this capture goes on in quick copy. */
    turnOn: () => void
    /** Hides the tip for good. */
    notNow: () => void
  } = $props()

  const BUTTON =
    'h-[30px] rounded-md text-[13px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-(--selection)'
</script>

<div class={BAR} role="status" onpointerdown={(e) => e.stopPropagation()}>
  <ZapIcon class="size-4 text-zinc-50" aria-hidden="true" />
  <span
    ><b class="font-semibold text-zinc-50">Just copying?</b> Quick copy copies as soon as you drag.</span
  >
  <button
    type="button"
    class="{BUTTON} bg-zinc-50 px-3 font-semibold text-zinc-950 hover:bg-zinc-200"
    onclick={turnOn}>Turn on</button
  >
  <button
    type="button"
    class="{BUTTON} px-2.5 font-medium text-zinc-300 hover:bg-zinc-800 hover:text-zinc-50"
    onclick={notNow}>Not now</button
  >
</div>
