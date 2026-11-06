<script lang="ts">
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { shortcutKeys } from '$lib/keys'

  let { accelerator }: { accelerator: string } = $props()
</script>

<KbdGroup>
  {#each shortcutKeys(accelerator) as key, k (k)}<Kbd>{key}</Kbd>{/each}
</KbdGroup>
