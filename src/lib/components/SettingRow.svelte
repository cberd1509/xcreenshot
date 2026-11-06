<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    title,
    description,
    for: htmlFor,
    children,
    below,
  }: {
    title: string
    description?: string
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
        <div class="text-[13px] text-muted-foreground">{description}</div>
      {/if}
    </div>
    {@render children()}
  </div>
  {@render below?.()}
</div>
