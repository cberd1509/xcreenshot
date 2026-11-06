<script lang="ts">
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { Separator } from '$lib/components/ui/separator'
  import { Switch } from '$lib/components/ui/switch'
  import { settings } from '$lib/settings.svelte'
  import SettingRow from '$lib/components/SettingRow.svelte'

  const sections = [
    { id: 'shortcuts', label: 'Shortcuts' },
    { id: 'capture', label: 'Capture' },
    { id: 'general', label: 'General' },
  ] as const

  let active = $state<string>('shortcuts')
</script>

<div class="flex h-screen bg-background text-foreground select-none">
  <nav
    aria-label="Settings sections"
    class="flex w-48 shrink-0 flex-col gap-0.5 border-r bg-sidebar p-3 text-sm"
  >
    {#each sections as s (s.id)}
      <a
        href={`#${s.id}`}
        aria-current={active === s.id ? 'page' : undefined}
        onclick={() => (active = s.id)}
        class="rounded-md px-2.5 py-2 text-muted-foreground no-underline hover:bg-sidebar-accent aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium aria-[current=page]:text-foreground"
      >
        {s.label}
      </a>
    {/each}
    <div class="grow"></div>
    <div class="px-2.5 py-2 text-xs text-muted-foreground">Version 0.1.0</div>
  </nav>

  <main class="flex min-w-0 grow flex-col gap-6 overflow-y-auto px-9 py-7">
    <section id="shortcuts" class="flex flex-col gap-3.5">
      <h2 class="text-sm font-semibold">Shortcuts</h2>
      <SettingRow title="Take a screenshot" description="Works from any app.">
        <button
          class="rounded-lg border px-2 py-1.5 hover:bg-accent"
          aria-label={`Change shortcut, currently ${settings.shortcut.join(' ')}`}
        >
          <KbdGroup>
            {#each settings.shortcut as key (key)}<Kbd>{key}</Kbd>{/each}
          </KbdGroup>
        </button>
      </SettingRow>
    </section>

    <Separator />

    <section id="capture" class="flex flex-col gap-3.5">
      <h2 class="text-sm font-semibold">Capture</h2>
      <SettingRow
        title="Remember region"
        description="Start with your last selection."
        for="remember-region"
      >
        <Switch id="remember-region" bind:checked={settings.rememberRegion} />
      </SettingRow>
      <SettingRow title="Magnifier" description="Zoom in to select exact pixels." for="magnifier">
        <Switch id="magnifier" bind:checked={settings.magnifier} />
      </SettingRow>
    </section>

    <Separator />

    <section id="general" class="flex flex-col gap-3.5">
      <h2 class="text-sm font-semibold">General</h2>
      <SettingRow
        title="Open at login"
        description="Start xcreenshot when you turn on your Mac."
        for="open-at-login"
      >
        <Switch id="open-at-login" bind:checked={settings.openAtLogin} />
      </SettingRow>
      <SettingRow
        title="Check for updates"
        description="Get new versions automatically."
        for="check-updates"
      >
        <Switch id="check-updates" bind:checked={settings.checkForUpdates} />
      </SettingRow>
    </section>
  </main>
</div>
