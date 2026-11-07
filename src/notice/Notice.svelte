<script lang="ts">
  import { failed } from '$lib/log'
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { onMount } from 'svelte'
  import ShortcutKbd from '$lib/components/ShortcutKbd.svelte'
  import { Button } from '$lib/components/ui/button'
  // Bundled by Vite and served from the app itself, so the CSP allows it on every platform.
  import icon from '../../src-tauri/icons/128x128@2x.png'
  import { card, type Notice } from './card'

  let notice = $state<Notice | null>(null)
  let primary = $state<HTMLElement | null>(null)
  let answered = false
  const shown = $derived(notice && card(notice))

  /** Closes a window with nothing to show; Rust then drops its question. */
  function close() {
    getCurrentWindow().close().catch(failed('close'))
  }

  onMount(() => {
    invoke<Notice | null>('notice')
      .then((n) => {
        notice = n
        if (!n) close()
      })
      .catch((e: unknown) => {
        failed('notice')(e)
        close()
      })
  })

  // Enter clicks the main button. No focus ring until the keyboard moves focus, as on the board.
  $effect(() => primary?.focus({ focusVisible: false }))

  /** Sends the button clicked; Rust closes the window. */
  function answer(isPrimary: boolean) {
    if (answered) return
    answered = true
    invoke('notice_answer', { primary: isPrimary }).catch((e: unknown) => {
      answered = false
      failed('notice_answer')(e)
    })
  }

  function onkeydown(e: KeyboardEvent) {
    if (!shown) return
    if (e.key === 'Escape') {
      e.preventDefault()
      answer(shown.escapeIsPrimary)
    } else if (e.key === 'Enter' && !(e.target instanceof HTMLButtonElement)) {
      // Focus left the buttons (a click on the card): Enter still means the main one.
      e.preventDefault()
      answer(true)
    }
  }

  /** The board's 36 px buttons. */
  const button = 'h-9 rounded-[8px] px-3.5 text-[13px]'
</script>

<svelte:window {onkeydown} />

{#if shown}
  <!-- The board's 24 px padding plus its 1 px border, which the system draws on the window's
  edge here: the text wraps as on the board. -->
  <main
    data-tauri-drag-region
    class="flex h-screen flex-col gap-5 bg-background p-[25px] leading-[normal] text-foreground select-none"
  >
    <div data-tauri-drag-region class="flex items-start gap-4">
      <!-- The app icon has the macOS grid's margin (its tile is 824 of 1024 px): drawn at 56 px,
      the tile is the board's 45 px in a 48 px box. -->
      <div class="relative size-12 shrink-0">
        <img
          src={icon}
          alt="xcreenshot"
          class="absolute -top-1 -left-1 size-14 max-w-none"
          draggable="false"
        />
      </div>
      <div data-tauri-drag-region class={['flex flex-col', shown.keys ? 'gap-2' : 'gap-1']}>
        <h1 class="text-[15px] font-semibold">{shown.title}</h1>
        <p class="text-[13px]/5 text-subtle-foreground">{shown.text}</p>
        {#if shown.keys}
          <ShortcutKbd accelerator={shown.keys} class="text-xs/[normal]" />
        {/if}
      </div>
    </div>
    <footer class="mt-auto flex justify-end gap-2">
      <Button variant="outline" class={button} onclick={() => answer(false)}>
        {shown.secondary}
      </Button>
      <!-- No border on the board's filled button. -->
      <Button bind:ref={primary} class="{button} border-0" onclick={() => answer(true)}>
        {shown.primary}
      </Button>
    </footer>
  </main>
{/if}
