<script lang="ts">
  import LockIcon from '@lucide/svelte/icons/lock'
  import { tick } from 'svelte'
  import { Input } from '$lib/components/ui/input'
  import { Toggle } from '$lib/components/ui/toggle'
  import { lockedSize, type Size } from './geometry'

  /**
   * The selection size in physical px. Click to type an exact size. `editing` is bindable so
   * Esc can cancel the edit.
   */
  let {
    size,
    onresize,
    editing = $bindable(false),
  }: {
    size: Size
    onresize: (size: Size, keepRatio: boolean) => void
    editing?: boolean
  } = $props()

  let width = $state(0)
  let height = $state(0)
  let lock = $state(false)
  let ratio = 1
  let form = $state<HTMLFormElement>()

  async function edit() {
    width = size.width
    height = size.height
    ratio = size.width / size.height
    editing = true
    await tick()
    const input = form?.querySelector('input')
    input?.focus()
    input?.select()
  }

  function typed(side: keyof Size) {
    if (!lock) return
    const value = side === 'width' ? width : height
    if (!(value >= 1)) return
    ;({ width, height } = lockedSize(side, value, ratio))
  }

  function apply(e: SubmitEvent) {
    e.preventDefault()
    editing = false
    if (width >= 1 && height >= 1) onresize({ width, height }, lock)
  }

  // Leaving the form without Enter cancels the edit.
  function onfocusout(e: FocusEvent) {
    if (!form?.contains(e.relatedTarget as Node | null)) editing = false
  }
</script>

<div
  class="rounded-md bg-popover/90 text-xs text-popover-foreground tabular-nums shadow-md"
  data-testid="size-badge"
  data-size-badge
>
  {#if editing}
    <form bind:this={form} class="flex items-center gap-1 p-1" onsubmit={apply} {onfocusout}>
      <Input
        type="number"
        min="1"
        aria-label="Width"
        class="h-7 w-20 text-xs"
        bind:value={width}
        oninput={() => typed('width')}
      />
      <span aria-hidden="true">×</span>
      <Input
        type="number"
        min="1"
        aria-label="Height"
        class="h-7 w-20 text-xs"
        bind:value={height}
        oninput={() => typed('height')}
      />
      <Toggle size="sm" aria-label="Lock ratio" bind:pressed={lock}>
        <LockIcon />
      </Toggle>
      <button type="submit" hidden aria-hidden="true"></button>
    </form>
  {:else}
    <button type="button" class="cursor-text px-2 py-1" onclick={edit} title="Set size">
      {size.width} × {size.height}
    </button>
  {/if}
</div>
