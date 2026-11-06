<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import CircleCheck from '@lucide/svelte/icons/circle-check'
  import { onMount } from 'svelte'
  import { Button } from '$lib/components/ui/button'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { settings, shortcutKeys } from '$lib/settings.svelte'

  const steps = [
    {
      title: 'Open System Settings',
      description: 'This opens Privacy & Security › Screen Recording.',
      action: { label: 'Open System Settings', command: 'open_permission_settings' },
    },
    {
      title: 'Turn on xcreenshot',
      description:
        'Find xcreenshot in the list and turn on its switch. Your Mac may ask for your password.',
    },
    {
      title: 'Quit and reopen',
      description: 'If macOS asks you to quit and reopen xcreenshot, use this button.',
      action: { label: 'Restart xcreenshot', command: 'restart_app' },
    },
  ]

  let granted = $state(false)

  function run(command: string) {
    invoke(command).catch((e: unknown) => console.error(`${command} failed`, e))
  }

  onMount(() => {
    // Listen first, so a grant while this page loads is never missed.
    const stop = listen('permission:granted', () => (granted = true))
    void stop.then(() => invoke<boolean>('permission_granted')).then((now) => (granted ||= now))
    return () => void stop.then((unlisten) => unlisten())
  })
</script>

<main class="flex h-screen flex-col gap-6 bg-background px-8 py-7 text-foreground select-none">
  {#if granted}
    <div class="flex grow flex-col items-center justify-center gap-3 text-center">
      <CircleCheck class="size-10" aria-hidden="true" />
      <h1 class="text-base font-semibold">All set.</h1>
      <p class="text-sm text-muted-foreground">
        Press
        <KbdGroup>
          {#each shortcutKeys(settings.shortcut) as key, k (k)}<Kbd>{key}</Kbd>{/each}
        </KbdGroup>
        to take a screenshot.
      </p>
    </div>
  {:else}
    <header class="flex flex-col gap-1">
      <h1 class="text-base font-semibold">Allow Screen Recording</h1>
      <p class="text-[13px] text-muted-foreground">
        macOS asks for this before xcreenshot can take screenshots.
      </p>
    </header>

    <ol class="flex flex-col gap-5">
      {#each steps as step, i (step.title)}
        <li class="flex gap-3">
          <span
            class="flex size-6 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-semibold"
            aria-hidden="true">{i + 1}</span
          >
          <div class="flex grow flex-col items-start gap-2">
            <div>
              <div class="text-sm font-medium">{step.title}</div>
              <div class="text-[13px] text-muted-foreground">{step.description}</div>
            </div>
            {#if step.action}
              <Button
                size="sm"
                variant={i === 0 ? 'default' : 'outline'}
                onclick={() => run(step.action.command)}
              >
                {step.action.label}
              </Button>
            {/if}
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</main>
