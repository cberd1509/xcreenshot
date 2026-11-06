<script lang="ts">
  import BackgroundIcon from '@lucide/svelte/icons/rectangle-horizontal'
  import { Toggle, ToggleGroup } from 'bits-ui'
  import { TEXT_SIZES, type ShownTextOptions, type TextOptions, type TextSize } from './text'
  import { BUTTON, DIVIDER } from '../panel'
  import Tip from './Tip.svelte'

  /** The text tool's font size and background, in the rail. */
  let {
    options,
    onchange,
  }: {
    /** No size is shown for text resized off S, M and L. */
    options: ShownTextOptions
    /** The option that changed. */
    onchange: (change: Partial<TextOptions>) => void
  } = $props()

  const LABELS: Record<TextSize, string> = { S: 'Small text', M: 'Medium text', L: 'Large text' }
</script>

<div class={DIVIDER}></div>
<ToggleGroup.Root
  type="single"
  orientation="vertical"
  aria-label="Text size"
  class="flex flex-col gap-0.5"
  value={options.size ?? ''}
  onValueChange={(size) => size && onchange({ size: size as TextSize })}
>
  {#each Object.keys(TEXT_SIZES) as size (size)}
    {#snippet item(props: Record<string, unknown>)}
      <ToggleGroup.Item
        {...props}
        value={size}
        aria-label={LABELS[size as TextSize]}
        class="{BUTTON} text-xs font-semibold">{size}</ToggleGroup.Item
      >
    {/snippet}
    <Tip label={LABELS[size as TextSize]} trigger={item} />
  {/each}
</ToggleGroup.Root>
{#snippet background(props: Record<string, unknown>)}
  <Toggle.Root
    {...props}
    aria-label="Background"
    class={BUTTON}
    pressed={options.background}
    onPressedChange={(on) => onchange({ background: on })}
  >
    <BackgroundIcon class="fill-current" />
  </Toggle.Root>
{/snippet}
<Tip label="Background" trigger={background} />
