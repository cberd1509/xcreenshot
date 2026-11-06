<script lang="ts">
  import { CURSORS, HANDLES, type Handle, type Rect } from './geometry'

  /** The selection in logical px. Handles carry `data-handle`; the overlay handles the drag. */
  let { rect }: { rect: Rect } = $props()

  /** A handle's offset along one axis: at the start edge, the end edge or the middle. */
  const offset = (h: Handle, start: string, end: string) =>
    h.includes(start) ? '0' : h.includes(end) ? '100%' : '50%'
</script>

<div
  class="selection"
  data-testid="selection"
  style:left="{rect.x}px"
  style:top="{rect.y}px"
  style:width="{rect.width}px"
  style:height="{rect.height}px"
>
  {#each HANDLES as handle (handle)}
    <div
      class="handle"
      data-handle={handle}
      style:left={offset(handle, 'w', 'e')}
      style:top={offset(handle, 'n', 's')}
      style:cursor={CURSORS[handle]}
    ></div>
  {/each}
</div>

<style>
  /* Under it, the annotation layer takes presses (and moves the selection); only the handles
     are hit here. */
  .selection {
    position: fixed;
    pointer-events: none;
    /* Inside the edge, as designed. */
    outline: 1px solid var(--selection);
    outline-offset: -1px;
    /* Dims everything outside; the frame inside stays at full brightness. */
    box-shadow: 0 0 0 100vmax rgb(9 9 11 / 0.55);
  }

  .handle {
    position: absolute;
    pointer-events: auto;
    box-sizing: border-box;
    width: 8px;
    height: 8px;
    margin: -4px;
    background: white;
    border: 1.5px solid var(--selection);
    border-radius: 2px;
  }
</style>
