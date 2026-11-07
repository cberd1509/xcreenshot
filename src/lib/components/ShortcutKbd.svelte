<script lang="ts">
  import { shortcutKeys } from '$lib/keys'
  import { cn } from '$lib/utils'

  /** `class` goes on each key cap. */
  let { accelerator, class: className }: { accelerator: string; class?: string } = $props()
</script>

<span class="inline-flex items-center gap-1 align-middle">
  {#each shortcutKeys(accelerator) as key, k (k)}
    <kbd
      class={cn(
        'rounded-[5px] border bg-muted px-[7px] py-0.5 font-kbd text-[13px] font-medium text-foreground',
        className,
      )}>{key}</kbd
    >
  {/each}
</span>
