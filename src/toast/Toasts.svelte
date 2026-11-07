<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check'
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { onMount } from 'svelte'
  import { TITLES, look, next, prune, push, up, type Toast, type ToastEvent } from './stack'

  /** The confirmation toasts, bottom centre of their window; Rust places the window. */
  let stack = $state<Toast[]>([])
  let now = $state(Date.now())
  /** The newest toast received; Rust hides the window once that one has faded. */
  let newest = 0
  let timer: ReturnType<typeof setTimeout> | undefined

  /** Fades and drops the toasts due, then waits for the next change, or tells Rust it is done. */
  function tick() {
    clearTimeout(timer)
    now = Date.now()
    stack = prune(stack, now)
    const at = next(stack, now)
    if (at !== null) timer = setTimeout(tick, at - now)
    else invoke('toast_idle', { seq: newest }).catch((e: unknown) => console.error(e))
  }

  /** The toasts received, by number: the load check and an event may both have one. */
  const got = new Set<number>()

  function add(events: ToastEvent[]) {
    // Skips a toast already shown. The load check's answer can come after a newer event, so
    // its toasts are not dropped for being older.
    const fresh = events.filter((e) => !got.has(e.seq))
    if (!fresh.length) return
    for (const event of fresh) {
      got.add(event.seq)
      newest = Math.max(newest, event.seq)
      stack = push(stack, event, Date.now())
    }
    tick()
  }

  /** A capture started: Rust hid the window already. */
  function clear() {
    clearTimeout(timer)
    stack = []
  }

  onMount(() => {
    const view = getCurrentWebviewWindow()
    const listeners = Promise.all([
      view.listen<ToastEvent>('toast:show', (e) => add([e.payload])),
      view.listen('toast:clear', clear),
    ])
    // Asked once listening, so a toast sent while this page loaded is never missed.
    void listeners
      .then(() => invoke<ToastEvent[]>('toast_ready'))
      .then(add)
      .catch((e: unknown) => console.error('could not get the toasts', e))
    return () => {
      clearTimeout(timer)
      void listeners.then((stops) => stops.forEach((stop) => stop()))
    }
  })
</script>

<div class="stack">
  {#each stack as toast, i (toast.id)}
    {@const { scale, opacity } = look(stack.length - 1 - i)}
    <div
      role="status"
      class="toast flex max-w-full items-center gap-2.5 rounded-full border border-zinc-800 bg-zinc-900/94 py-2 pr-3.5 pl-2.5 text-[13px] whitespace-nowrap text-zinc-400 shadow-[0_12px_32px_rgb(0_0_0/0.35)]"
      style:opacity={up(toast, now) ? opacity : 0}
      style:transform="scale({scale})"
    >
      <span class="flex size-5 shrink-0 items-center justify-center rounded-full bg-zinc-50">
        <CheckIcon class="size-3 text-zinc-950" strokeWidth={3} aria-hidden="true" />
      </span>
      <b class="font-semibold text-zinc-50">{TITLES[toast.kind]}</b>
      {#if toast.detail}
        <span class="truncate font-mono text-xs">{toast.detail}</span>
      {/if}
      {#if toast.count > 1}
        <span class="rounded-full bg-zinc-700 px-[7px] py-px text-xs font-semibold text-zinc-50">
          ×{toast.count}
        </span>
      {/if}
    </div>
  {/each}
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    background: transparent;
    user-select: none;
  }

  .stack {
    position: fixed;
    left: 16px;
    right: 16px;
    bottom: 28px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .toast {
    transition:
      opacity 200ms,
      transform 200ms;
    animation: enter 150ms ease-out;
  }

  @keyframes enter {
    from {
      opacity: 0;
      translate: 0 6px;
    }
  }
</style>
