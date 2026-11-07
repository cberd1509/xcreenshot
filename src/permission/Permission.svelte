<script lang="ts">
  import { failed } from '$lib/log'
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { onMount } from 'svelte'
  import ShortcutKbd from '$lib/components/ShortcutKbd.svelte'
  // Bundled by Vite and served from the app itself, so the CSP allows it on every platform.
  import icon from '../../src-tauri/icons/128x128@2x.png'
  import { Button } from '$lib/components/ui/button'
  import { FAILURES_SHOWN, nextDelay } from './poll'

  type CaptureShortcut = { accelerator: string; registered: boolean }

  let granted = $state(false)
  /** Back from System Settings twice and still not allowed: macOS likely needs a restart.
   * (Whether the switch is on can't be read without Full Disk Access, so this is a guess.) */
  let needsRestart = $state(false)
  /** Times the window came back from System Settings without the permission. */
  let returns = 0
  /** The capture shortcut and whether it works; null if unknown, undefined until known. */
  let shortcut = $state<CaptureShortcut | null>()
  /** Failed checks in a row. */
  let failures = $state(0)
  /** The Privacy & Security pane's name, which depends on the macOS version (from Rust); none
   * until it's known. */
  let pane = $state<string>()
  let openedSettings = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let stopped = false

  /** Checks now and plans the next check. Resolves to the permission, or null if it failed. */
  async function check(): Promise<boolean | null> {
    clearTimeout(timer)
    let now: boolean | null = null
    try {
      now = await invoke<boolean>('permission_granted')
      granted = now
      failures = 0
    } catch (e) {
      failures++
      failed('permission_granted')(e)
    }
    // A focus check can overlap the timer's; only the last one plans the next.
    clearTimeout(timer)
    if (!stopped) timer = setTimeout(check, nextDelay(granted, failures))
    return now
  }

  function openSettings() {
    invoke('open_permission_settings')
      .then(() => (openedSettings = true))
      .catch(failed('open_permission_settings'))
  }

  function restart() {
    invoke('restart_app').catch(failed('restart_app'))
  }

  /** Closes the window; the capture shortcut opens it again while there's no permission. */
  function later() {
    getCurrentWindow().close().catch(failed('close'))
  }

  /** Reads the capture shortcut; again on focus, as it may have changed in Settings. */
  function loadShortcut() {
    invoke<CaptureShortcut>('capture_shortcut')
      .then((s) => (shortcut = s))
      .catch((e: unknown) => {
        shortcut = null
        failed('capture_shortcut')(e)
      })
  }

  function onfocus() {
    loadShortcut()
    if (!openedSettings || granted) return
    void check().then((now) => {
      if (now === false && ++returns >= 2) needsRestart = true
    })
  }

  onMount(() => {
    void check()
    loadShortcut()
    invoke<string>('permission_pane')
      .then((name) => (pane = name))
      .catch((e: unknown) => {
        // Recognisable on every version.
        pane = 'Screen Recording'
        failed('permission_pane')(e)
      })
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  })

  /** The footer's 40 px buttons. */
  const big = 'h-10 rounded-md px-4'
</script>

<svelte:window {onfocus} />

{#snippet number(n: number)}
  <span
    class="flex size-[22px] shrink-0 items-center justify-center rounded-full bg-muted text-xs font-semibold"
    aria-hidden="true">{n}</span
  >
{/snippet}

<main
  class="flex h-screen flex-col gap-6 bg-background p-10 leading-[normal] text-foreground select-none"
>
  <!-- The app icon has the macOS grid's margin (its tile is 824 of 1024 px): drawn at 70 px,
  the tile itself is the designed 56 px. -->
  <div class="relative size-14 shrink-0">
    <img
      src={icon}
      alt="xcreenshot"
      class="absolute -top-[7px] -left-[7px] size-[70px] max-w-none"
      draggable="false"
    />
  </div>

  {#if granted}
    <header class="flex flex-col gap-2">
      <h1 class="text-2xl/[normal] font-bold tracking-[-0.01em]">All set.</h1>
      {#if shortcut?.registered}
        <p class="text-sm/[22px] text-subtle-foreground">
          Press <ShortcutKbd accelerator={shortcut.accelerator} /> to take a screenshot.
        </p>
      {:else if shortcut}
        <p class="text-sm/[22px] text-subtle-foreground">
          Another app is using <ShortcutKbd accelerator={shortcut.accelerator} />. Close it and
          restart xcreenshot.
        </p>
      {/if}
    </header>
    {#if shortcut?.registered}
      <p class="mt-auto flex items-center justify-end gap-1 text-[13px] text-muted-foreground">
        Screenshots look empty?
        <Button variant="link" class="h-auto px-0 text-[13px]" onclick={restart}>
          Restart xcreenshot
        </Button>
      </p>
    {:else if shortcut}
      <footer class="mt-auto flex justify-end">
        <Button class={big} onclick={restart}>Restart xcreenshot</Button>
      </footer>
    {/if}
  {:else}
    <header class="flex flex-col gap-2">
      <h1 class="text-2xl/[normal] font-bold tracking-[-0.01em]">Allow screen recording</h1>
      <p class="text-sm/[22px] text-subtle-foreground">
        xcreenshot needs this to take screenshots. Nothing leaves your Mac.
      </p>
    </header>

    <ol class="flex flex-col gap-3 text-sm/[22px]">
      <li class="flex items-start gap-3">
        {@render number(1)}
        <span>Click <b class="font-semibold">Open System Settings</b> below.</span>
      </li>
      <li class="flex items-start gap-3">
        {@render number(2)}
        <span>
          Go to <b class="font-semibold">Privacy & Security{pane ? ` › ${pane}` : ''}</b>
          and turn on xcreenshot.
        </span>
      </li>
      <li class="flex items-start gap-3">
        {@render number(3)}
        <span>Come back to this window. It updates on its own.</span>
      </li>
    </ol>

    <div class="flex flex-col gap-1.5">
      <div
        class="flex items-center gap-2.5 rounded-[10px] border bg-sidebar px-3.5 py-3 text-[13px] text-subtle-foreground"
      >
        <span class="size-2 shrink-0 rounded-full bg-amber-500" aria-hidden="true"></span>
        <span role="status" class="grow">
          {needsRestart
            ? 'macOS needs xcreenshot to restart before the change works.'
            : 'Waiting for permission…'}
        </span>
        {#if needsRestart}
          <Button size="sm" onclick={restart}>Restart xcreenshot</Button>
        {/if}
      </div>
      {#if failures >= FAILURES_SHOWN}
        <p role="alert" class="text-[13px] text-destructive">
          Can't check the permission right now. Trying again.
        </p>
      {/if}
    </div>

    <footer class="mt-auto flex justify-end gap-2">
      <Button variant="outline" class={big} onclick={later}>Later</Button>
      <Button variant={needsRestart ? 'outline' : 'default'} class={big} onclick={openSettings}>
        Open System Settings
      </Button>
    </footer>
  {/if}
</main>
