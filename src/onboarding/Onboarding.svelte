<script lang="ts">
  // The First run artboard (design/Onboarding.dc.html). macOS: the Screen Recording steps,
  // then the shortcut and a place to try it. Elsewhere no permission is needed, so it's the
  // shortcut only, plus how to give PrtSc to xcreenshot when Snipping Tool has it (Windows).
  import { failed } from '$lib/log'
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { Check } from '@lucide/svelte'
  import { onMount } from 'svelte'
  import ShortcutKbd from '$lib/components/ShortcutKbd.svelte'
  import ShortcutRecorder from '$lib/components/ShortcutRecorder.svelte'
  import { isMac } from '$lib/keys'
  import { setSetting, settings } from '$lib/settings.svelte'
  // Bundled by Vite and served from the app itself, so the CSP allows it on every platform.
  import icon from '../../src-tauri/icons/128x128@2x.png'
  import { Button } from '$lib/components/ui/button'
  import { FAILURES_SHOWN, nextDelay } from './poll'

  /** Sent by Rust when a capture closes (copy, save, share or Esc). */
  const CAPTURE_CLOSED = 'capture:closed'
  const IN_USE = 'That shortcut is in use by another app.'

  /** Capture works: the permission is on (macOS; always elsewhere). */
  let granted = $state(!isMac)
  /** Back from System Settings twice and still not allowed: macOS likely needs a restart.
   * (Whether the switch is on can't be read without Full Disk Access, so this is a guess.) */
  let needsRestart = $state(false)
  /** Times the window came back from System Settings without the permission. */
  let returns = 0
  /** Whether the capture shortcut is registered (another app may own it); unknown until read. */
  let registered = $state<boolean>()
  /** A capture opened and closed while this window was open. */
  let tried = $state(false)
  /** Windows: Snipping Tool has PrtSc, so the shortcut is Ctrl Shift PrtSc. */
  let taken = $state(false)
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

  function openKeyboardSettings() {
    invoke('open_keyboard_settings').catch(failed('open_keyboard_settings'))
  }

  function restart() {
    invoke('restart_app').catch(failed('restart_app'))
  }

  /** Closes the window. Rust records onboarding as done if capture works by then; without the
   * permission (Later), the capture shortcut opens it again. */
  function close() {
    getCurrentWindow().close().catch(failed('close'))
  }

  /** Whether the shortcut is registered; again when it changes or the window gets focus. */
  function loadRegistered() {
    invoke<{ registered: boolean }>('capture_shortcut')
      .then((s) => (registered = s.registered))
      .catch(failed('capture_shortcut'))
  }

  /** Windows: whether Snipping Tool still has PrtSc. Rust switches to PrtSc once it doesn't,
   * and the new shortcut arrives as a settings change. */
  function checkPrintScreen() {
    if (isMac) return
    invoke<boolean>('onboarding_print_screen')
      .then((t) => (taken = t))
      .catch(failed('onboarding_print_screen'))
  }

  function onfocus() {
    loadRegistered()
    checkPrintScreen()
    if (!isMac || !openedSettings || granted) return
    void check().then((now) => {
      if (now === false && ++returns >= 2) needsRestart = true
    })
  }

  // Follows the shortcut, which can change here, in Settings or when PrtSc is claimed.
  $effect(() => {
    void settings.shortcut
    loadRegistered()
  })

  onMount(() => {
    const unlisten = listen(CAPTURE_CLOSED, () => (tried = true))
    unlisten.catch(failed('listen'))
    checkPrintScreen()
    if (isMac) {
      void check()
      invoke<string>('permission_pane')
        .then((name) => (pane = name))
        .catch((e: unknown) => {
          // Recognisable on every version.
          pane = 'Screen Recording'
          failed('permission_pane')(e)
        })
    }
    return () => {
      stopped = true
      clearTimeout(timer)
      void unlisten.then((stop) => stop()).catch(() => {})
    }
  })

  const waiting = $derived(isMac && !granted)
  /** The shortcut can work: capture is allowed and the shortcut is registered. */
  const canTry = $derived(granted && registered !== false)
  const note = $derived(
    taken
      ? 'PrtSc opens Snipping Tool, so xcreenshot uses this. Click to change it.'
      : 'Works from any app. Click to change it.',
  )

  /** The footer's 40 px buttons. */
  const big = 'h-10 rounded-md px-4'
  /** Boxes with a status line. */
  const box = 'flex items-center gap-2.5 rounded-[10px] px-3.5 py-3 text-[13px]'
</script>

<svelte:window {onfocus} />

{#snippet number(n: number)}
  <span
    class="flex size-[22px] shrink-0 items-center justify-center rounded-full bg-muted text-xs font-semibold"
    aria-hidden="true">{n}</span
  >
{/snippet}

{#snippet smallNumber(n: number)}
  <span
    class="flex size-5 shrink-0 items-center justify-center rounded-full bg-border text-[11px] font-semibold text-foreground"
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

  <header class="flex flex-col gap-2">
    <h1 class="text-2xl/[normal] font-bold tracking-[-0.01em]">
      {isMac ? 'Allow screen recording' : 'xcreenshot is ready'}
    </h1>
    <p class="text-sm/[22px] text-subtle-foreground">
      {isMac
        ? 'xcreenshot needs this to take screenshots. Nothing leaves your Mac.'
        : 'It lives in the system tray.'}
    </p>
  </header>

  {#if isMac}
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
      <div class="{box} border bg-sidebar text-subtle-foreground">
        <span
          class={['size-2 shrink-0 rounded-full', granted ? 'bg-foreground' : 'bg-zinc-400']}
          aria-hidden="true"
        ></span>
        <span role="status" class="grow">
          {#if granted}
            Screen recording is on.
          {:else if needsRestart}
            macOS needs xcreenshot to restart before the change works.
          {:else}
            Waiting for permission…
          {/if}
        </span>
        {#if waiting && needsRestart}
          <Button size="sm" onclick={restart}>Restart xcreenshot</Button>
        {/if}
      </div>
      {#if failures >= FAILURES_SHOWN}
        <p role="alert" class="text-[13px] text-destructive">
          Can't check the permission right now. Trying again.
        </p>
      {/if}
    </div>
  {/if}

  <div class="flex flex-col gap-1 border-t pt-5">
    <ShortcutRecorder
      title="Take a screenshot"
      description={note}
      descriptionClass="leading-5"
      accelerator={settings.shortcut}
      onchange={(a) => setSetting('shortcut', a)}
    />
    {#if registered === false}
      <p role="alert" class="text-[13px] text-destructive">{IN_USE}</p>
    {/if}
  </div>

  {#if canTry}
    {#if tried}
      <div role="status" class="{box} border bg-sidebar text-subtle-foreground">
        <span
          class="flex size-5 shrink-0 items-center justify-center rounded-full bg-foreground text-background"
        >
          <Check class="size-3" strokeWidth={3} aria-hidden="true" />
        </span>
        <span><b class="font-semibold text-foreground">It works.</b> You're all set.</span>
      </div>
    {:else}
      <div
        class="{box} border border-dashed border-zinc-400 text-subtle-foreground dark:border-zinc-600"
      >
        <ShortcutKbd accelerator={settings.shortcut} class="text-xs" />
        <span>
          <b class="font-semibold text-foreground">Try it now.</b> Press the keys and drag over anything.
        </span>
      </div>
    {/if}
  {/if}

  {#if taken}
    <section
      aria-labelledby="print-screen"
      class="flex flex-col gap-3 rounded-[10px] border bg-sidebar p-4"
    >
      <h2 id="print-screen" class="text-sm/[normal] font-semibold">Want to use PrtSc instead?</h2>
      <ol class="flex flex-col gap-2 text-[13px]/5 text-subtle-foreground">
        <li class="flex items-start gap-3">
          {@render smallNumber(1)}
          <span>Click <b class="font-semibold text-foreground">Open keyboard settings</b>.</span>
        </li>
        <li class="flex items-start gap-3">
          {@render smallNumber(2)}
          <span>
            Turn off
            <b class="font-semibold text-foreground"
              >Use the Print screen key to open screen capture</b
            >.
          </span>
        </li>
        <li class="flex items-start gap-3">
          {@render smallNumber(3)}
          <span>Come back here. xcreenshot switches to PrtSc on its own.</span>
        </li>
      </ol>
      <Button
        variant="outline"
        class="h-8 self-start rounded-md border-zinc-300 px-3 text-[13px] dark:border-input"
        onclick={openKeyboardSettings}
      >
        Open keyboard settings
      </Button>
    </section>
  {/if}

  <footer class="mt-auto flex justify-end gap-2">
    {#if waiting}
      <Button variant="outline" class={big} onclick={close}>Later</Button>
      <Button variant={needsRestart ? 'outline' : 'default'} class={big} onclick={openSettings}>
        Open System Settings
      </Button>
    {:else}
      <Button class={big} onclick={close}>Done</Button>
    {/if}
  </footer>
</main>
