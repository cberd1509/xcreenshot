<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    title,
    description,
    for: htmlFor,
    children,
  }: { title: string; description?: string; for?: string; children: Snippet } = $props()
</script>

<div class="flex items-center gap-4">
  <div class="grow">
    {#if htmlFor}
      <label for={htmlFor} class="text-sm font-medium">{title}</label>
    {:else}
      <div class="text-sm font-medium">{title}</div>
    {/if}
    {#if description}
      <div class="text-[13px] text-muted-foreground">{description}</div>
    {/if}
  </div>
  {@render children()}
</div>
