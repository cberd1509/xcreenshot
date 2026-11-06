<script lang="ts">
  import { shortcutKeys } from '$lib/keys'

  let { accelerator }: { accelerator: string } = $props()
</script>

<span class="inline-flex items-center gap-1 align-middle">
  {#each shortcutKeys(accelerator) as key, k (k)}
    <kbd
      class="rounded-[5px] border bg-muted px-[7px] py-0.5 font-kbd text-[13px] font-medium text-foreground"
      >{key}</kbd
    >
  {/each}
</span>
