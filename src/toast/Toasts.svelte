<script lang="ts">
  import CheckIcon from '@lucide/svelte/icons/check'
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { isMac } from '$lib/keys'
  import { onMount } from 'svelte'
  import {
    TITLES,
    clickable,
    dismiss,
    hover,
    leave,
    look,
    next,
    prune,
    push,
    reveal,
    up,
    type Toast,
    type ToastEvent,
  } from './stack'

  /** The confirmation toasts, bottom centre of their window; Rust places the window. */
  let stack = $state<Toast[]>([])
  let now = $state(Date.now())
  /** The newest toast received; Rust hides the window once that one has faded. */
  let newest = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  /** Whether Rust was told to make the window take clicks. */
  let taking = false

  /**
   * Fades and drops the toasts due, then waits for the next change, or tells Rust it is done.
   * The window takes clicks only while a Saved toast is up.
   */
  function tick() {
    clearTimeout(timer)
    now = Date.now()
    stack = prune(stack, now)
    const on = clickable(stack, now)
    if (on !== taking) {
      taking = on
      invoke('toast_clickable', { on }).catch((e: unknown) => console.error(e))
    }
    const at = next(stack, now)
    if (at !== null) timer = setTimeout(tick, at - now)
    else if (!stack.length)
      invoke('toast_idle', { seq: newest }).catch((e: unknown) => console.error(e))
  }

  /** Changes the stack now (the mouse or a button), then catches up. */
  function change(f: (stack: Toast[], now: number) => Toast[]) {
    stack = f(stack, Date.now())
    tick()
  }

  /** A Saved toast's button: Rust opens the file that toast is about, and the toast goes. */
  function act(toast: Toast, action: 'open' | 'reveal') {
    invoke('toast_action', { seq: toast.seq, action }).catch((e: unknown) => console.error(e))
    change((s, at) => dismiss(s, toast.id, at))
  }

  /** Keeps a click from focusing the button: the toasts never take keyboard focus. */
  const noFocus = (e: MouseEvent) => e.preventDefault()

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

  /** A capture started: Rust hid the window already, and made it ignore the mouse. */
  function clear() {
    clearTimeout(timer)
    stack = []
    taking = false
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
    {@const saved = toast.kind === 'saved'}
    <div
      role="status"
      class={[
        'toast flex max-w-full items-center gap-2.5 rounded-full border border-zinc-800 bg-zinc-900/94 text-[13px] whitespace-nowrap text-zinc-400 shadow-[0_12px_32px_rgb(0_0_0/0.35)]',
        saved ? 'py-[5px] pr-[5px] pl-2.5' : 'py-2 pr-3.5 pl-2.5',
      ]}
      style:opacity={up(toast, now) ? opacity : 0}
      style:transform="scale({scale})"
      onmouseenter={saved ? () => change((s, at) => hover(s, toast.id, at)) : undefined}
      onmouseleave={saved ? () => change((s, at) => leave(s, toast.id, at)) : undefined}
    >
      <span class="flex size-5 shrink-0 items-center justify-center rounded-full bg-zinc-50">
        <CheckIcon class="size-3 text-zinc-950" strokeWidth={3} aria-hidden="true" />
      </span>
      <b class="font-semibold text-zinc-50">{TITLES[toast.kind]}</b>
      {#if toast.detail}
        <span class="truncate font-mono text-xs">{toast.detail}</span>
      {/if}
      {#if saved}
        <span class="ml-1 flex shrink-0 gap-1">
          <button
            type="button"
            tabindex="-1"
            class="h-[26px] rounded-full bg-zinc-50 px-2.5 text-xs font-semibold text-zinc-950"
            onmousedown={noFocus}
            onclick={() => act(toast, 'open')}
          >
            Open
          </button>
          <button
            type="button"
            tabindex="-1"
            class="h-[26px] rounded-full bg-zinc-700 px-2.5 text-xs font-medium text-zinc-50"
            onmousedown={noFocus}
            onclick={() => act(toast, 'reveal')}
          >
            {reveal(isMac)}
          </button>
        </span>
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
