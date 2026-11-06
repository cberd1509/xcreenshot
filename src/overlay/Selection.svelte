<script lang="ts">
  import { CURSORS, HANDLES, type Rect } from './geometry'

  /** The selection in logical px. Handles carry `data-handle`; the overlay handles the drag. */
  let { rect }: { rect: Rect } = $props()
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
    <div class="handle {handle}" data-handle={handle} style:cursor={CURSORS[handle]}></div>
  {/each}
</div>

<style>
  .selection {
    position: fixed;
    cursor: move;
    outline: 1px solid white;
    /* Dims everything outside; the frame inside stays at full brightness. */
    box-shadow: 0 0 0 100vmax rgb(0 0 0 / 0.35);
  }

  .handle {
    position: absolute;
    width: 8px;
    height: 8px;
    margin: -4px;
    background: white;
    border: 1px solid rgb(0 0 0 / 0.5);
    border-radius: 2px;
  }

  .nw,
  .n,
  .ne {
    top: 0;
  }
  .sw,
  .s,
  .se {
    top: 100%;
  }
  .e,
  .w {
    top: 50%;
  }
  .nw,
  .w,
  .sw {
    left: 0;
  }
  .ne,
  .e,
  .se {
    left: 100%;
  }
  .n,
  .s {
    left: 50%;
  }
</style>
