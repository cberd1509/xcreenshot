<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import CircleCheck from '@lucide/svelte/icons/circle-check'
  import { onMount } from 'svelte'
  import ShortcutKbd from '$lib/components/ShortcutKbd.svelte'
  import { Button } from '$lib/components/ui/button'

  /** How often the page re-checks while it is open. */
  const RECHECK_MS = 1000

  let granted = $state(false)
  /** Back from System Settings and still not allowed: macOS needs a restart. */
  let needsRestart = $state(false)
  /** The registered capture shortcut; null if none, undefined until known. */
  let shortcut = $state<string | null>()
  let openedSettings = false
  let timer: ReturnType<typeof setInterval> | undefined

  const failed = (command: string) => (e: unknown) => console.error(`${command} failed`, e)

  async function check() {
    if (await invoke<boolean>('permission_granted')) {
      granted = true
      clearInterval(timer)
    }
    return granted
  }

  const recheck = () => void check().catch(failed('permission_granted'))

  function openSettings() {
    openedSettings = true
    invoke('open_permission_settings').catch(failed('open_permission_settings'))
  }

  function restart() {
    invoke('restart_app').catch(failed('restart_app'))
  }

  function onfocus() {
    if (!openedSettings || granted) return
    check()
      .then((now) => (needsRestart = !now))
      .catch(failed('permission_granted'))
  }

  onMount(() => {
    timer = setInterval(recheck, RECHECK_MS)
    recheck()
    invoke<string | null>('capture_shortcut')
      .then((s) => (shortcut = s))
      .catch((e: unknown) => {
        shortcut = null
        failed('capture_shortcut')(e)
      })
    return () => clearInterval(timer)
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
      {#if shortcut}
        <p class="text-sm text-muted-foreground">
          Press <ShortcutKbd accelerator={shortcut} /> to take a screenshot.
        </p>
      {:else if shortcut === null}
        <p class="text-sm text-muted-foreground">Set a shortcut in Settings.</p>
      {/if}
    </div>
    <p class="flex items-center justify-center gap-1 text-[13px] text-muted-foreground">
      Screenshots look empty?
      <Button size="sm" variant="link" class="h-auto px-0" onclick={restart}>
        Restart xcreenshot
      </Button>
    </p>
  {:else}
    <header class="flex flex-col gap-1">
      <h1 class="text-base font-semibold">Allow Screen Recording</h1>
      <p class="text-[13px] text-muted-foreground">
        macOS asks for this before xcreenshot can take screenshots.
      </p>
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
