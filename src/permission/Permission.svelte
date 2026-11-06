<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import CircleCheck from '@lucide/svelte/icons/circle-check'
  import { onMount } from 'svelte'
  import ShortcutKbd from '$lib/components/ShortcutKbd.svelte'
  import { Button } from '$lib/components/ui/button'
  import { FAILURES_SHOWN, nextDelay } from './poll'

  type CaptureShortcut = { accelerator: string; registered: boolean }

  let granted = $state(false)
  /** Back from System Settings and still not allowed: macOS needs a restart. */
  let needsRestart = $state(false)
  /** The capture shortcut and whether it works; null if unknown, undefined until known. */
  let shortcut = $state<CaptureShortcut | null>()
  /** Failed checks in a row. */
  let failures = $state(0)
  let openedSettings = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let stopped = false

  const failed = (command: string) => (e: unknown) => console.error(`${command} failed`, e)

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

  function onfocus() {
    if (!openedSettings || granted) return
    void check().then((now) => {
      if (now === false) needsRestart = true
    })
  }

  onMount(() => {
    void check()
    invoke<CaptureShortcut>('capture_shortcut')
      .then((s) => (shortcut = s))
      .catch((e: unknown) => {
        shortcut = null
        failed('capture_shortcut')(e)
      })
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  })
</script>

<svelte:window {onfocus} />

{#snippet number(n: number)}
  <span
    class="flex size-6 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-semibold"
    aria-hidden="true">{n}</span
  >
{/snippet}

<main class="flex h-screen flex-col gap-6 bg-background px-8 py-7 text-foreground select-none">
  {#if granted}
    <div class="flex grow flex-col items-center justify-center gap-3 text-center">
      <CircleCheck class="size-10" aria-hidden="true" />
      <h1 class="text-base font-semibold">All set.</h1>
      {#if shortcut?.registered}
        <p class="text-sm text-muted-foreground">
          Press <ShortcutKbd accelerator={shortcut.accelerator} /> to take a screenshot.
        </p>
      {:else if shortcut}
        <p class="text-sm text-muted-foreground">
          Another app is using <ShortcutKbd accelerator={shortcut.accelerator} />. Close it and
          restart xcreenshot.
        </p>
        <Button size="sm" onclick={restart}>Restart xcreenshot</Button>
      {/if}
    </div>
    {#if shortcut?.registered}
      <p class="flex items-center justify-center gap-1 text-[13px] text-muted-foreground">
        Screenshots look empty?
        <Button size="sm" variant="link" class="h-auto px-0" onclick={restart}>
          Restart xcreenshot
        </Button>
      </p>
    {/if}
  {:else}
    <header class="flex flex-col gap-1">
      <h1 class="text-base font-semibold">Allow Screen Recording</h1>
      <p class="text-[13px] text-muted-foreground">
        macOS asks for this before xcreenshot can take screenshots.
      </p>
      {#if failures >= FAILURES_SHOWN}
        <p role="alert" class="text-[13px] text-destructive">
          Can't check the permission right now. Trying again.
        </p>
      {/if}
    </header>

    <ol class="flex flex-col gap-5">
      <li class="flex gap-3">
        {@render number(1)}
        <div class="flex grow flex-col items-start gap-2">
          <div>
            <div class="text-sm font-medium">Open System Settings</div>
            <div class="text-[13px] text-muted-foreground">
              This opens Privacy & Security › Screen Recording.
            </div>
          </div>
          <Button size="sm" variant={needsRestart ? 'outline' : 'default'} onclick={openSettings}>
            Open System Settings
          </Button>
        </div>
      </li>
      <li class="flex gap-3">
        {@render number(2)}
        <div>
          <div class="text-sm font-medium">Turn on xcreenshot</div>
          <div class="text-[13px] text-muted-foreground">
            Find xcreenshot in the list and turn on its switch. Your Mac may ask for your password.
          </div>
        </div>
      </li>
      <li class="flex gap-3">
        {@render number(3)}
        <div class="flex grow flex-col items-start gap-2">
          <div>
            <div class="text-sm font-medium">Quit and reopen</div>
            <div class="text-[13px] text-muted-foreground">
              {needsRestart
                ? 'macOS needs xcreenshot to restart before the change works.'
                : 'If macOS asks you to quit and reopen xcreenshot, use this button.'}
            </div>
          </div>
          <Button size="sm" variant={needsRestart ? 'default' : 'outline'} onclick={restart}>
            Restart xcreenshot
          </Button>
        </div>
      </li>
    </ol>
  {/if}
</main>
