<script lang="ts">
  import BackgroundIcon from '@lucide/svelte/icons/rectangle-horizontal'
  import { Separator } from '$lib/components/ui/separator'
  import { Toggle } from '$lib/components/ui/toggle'
  import * as ToggleGroup from '$lib/components/ui/toggle-group'
  import { TEXT_SIZES, type TextOptions, type TextSize } from './text'

  /**
   * The text tool's font size and background, in the toolbar. Its buttons never take focus,
   * so text being typed stays in its editor and follows the change.
   */
  let { options, onchange }: { options: TextOptions; onchange: (options: TextOptions) => void } =
    $props()

  const LABELS: Record<TextSize, string> = { S: 'Small text', M: 'Medium text', L: 'Large text' }
</script>

<div class="flex items-center gap-0.5" role="presentation" onmousedown={(e) => e.preventDefault()}>
  <Separator orientation="vertical" class="mx-1 h-5" />
  <ToggleGroup.Root
    type="single"
    size="sm"
    spacing={1}
    aria-label="Text size"
    value={options.size}
    onValueChange={(size) => size && onchange({ ...options, size: size as TextSize })}
  >
    {#each Object.keys(TEXT_SIZES) as size (size)}
      <ToggleGroup.Item value={size} aria-label={LABELS[size as TextSize]}>{size}</ToggleGroup.Item>
    {/each}
  </ToggleGroup.Root>
  <Toggle
    size="sm"
    aria-label="Background"
    title="Background"
    pressed={options.background}
    onPressedChange={(background) => onchange({ ...options, background })}
  >
    <BackgroundIcon class="fill-current" />
  </Toggle>
  <Separator orientation="vertical" class="mx-1 h-5" />
</div>
