<script lang="ts">
  import BackgroundIcon from '@lucide/svelte/icons/rectangle-horizontal'
  import { Separator } from '$lib/components/ui/separator'
  import { Toggle } from '$lib/components/ui/toggle'
  import * as ToggleGroup from '$lib/components/ui/toggle-group'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { TEXT_SIZES, type ShownTextOptions, type TextOptions, type TextSize } from './text'

  /** The text tool's font size and background, in the toolbar. */
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

<div class="flex items-center gap-0.5">
  <Separator orientation="vertical" class="mx-1 h-5" />
  <ToggleGroup.Root
    type="single"
    size="sm"
    spacing={1}
    aria-label="Text size"
    value={options.size ?? ''}
    onValueChange={(size) => size && onchange({ size: size as TextSize })}
  >
    {#each Object.keys(TEXT_SIZES) as size (size)}
      <ToggleGroup.Item value={size} aria-label={LABELS[size as TextSize]}>{size}</ToggleGroup.Item>
    {/each}
  </ToggleGroup.Root>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Toggle
          {...props}
          size="sm"
          aria-label="Background"
          pressed={options.background}
          onPressedChange={(background) => onchange({ background })}
        >
          <BackgroundIcon class="fill-current" />
        </Toggle>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content side="bottom">Background</Tooltip.Content>
  </Tooltip.Root>
  <Separator orientation="vertical" class="mx-1 h-5" />
</div>
