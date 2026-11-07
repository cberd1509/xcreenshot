<script lang="ts">
  import type { Snippet } from 'svelte'
  import { cn } from '$lib/utils'

  let {
    title,
    description,
    for: htmlFor,
    descriptionClass,
    children,
    below,
  }: {
    title: string
    description?: string
    /** Extra classes for the description, such as its line height. */
    descriptionClass?: string
    for?: string
    children: Snippet
    /** Lines under the row, such as an error. */
    below?: Snippet
  } = $props()
</script>

<div class="flex flex-col gap-1.5">
  <div class="flex items-center gap-4">
    <div class="grow">
      {#if htmlFor}
        <label for={htmlFor} class="block text-sm/[normal] font-medium">{title}</label>
      {:else}
        <div class="text-sm/[normal] font-medium">{title}</div>
      {/if}
      {#if description}
        <div class={cn('text-[13px] text-muted-foreground', descriptionClass)}>{description}</div>
      {/if}
    </div>
    {@render children()}
  </div>
  {@render below?.()}
</div>
