<script lang="ts">
  import { getVersion } from '@tauri-apps/api/app'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { Separator } from '$lib/components/ui/separator'
  import { Switch } from '$lib/components/ui/switch'
  import SettingRow from '$lib/components/SettingRow.svelte'
  import { settings, shortcutKeys, type ToggleKey } from '$lib/settings.svelte'

  type Section = {
    id: string
    label: string
    toggles: { key: ToggleKey; title: string; description: string }[]
  }

  const sections: Section[] = [
    { id: 'shortcuts', label: 'Shortcuts', toggles: [] },
    {
      id: 'capture',
      label: 'Capture',
      toggles: [
        {
          key: 'rememberRegion',
          title: 'Remember region',
          description: 'Start with your last selection.',
        },
        { key: 'magnifier', title: 'Magnifier', description: 'Zoom in to select exact pixels.' },
      ],
    },
    {
      id: 'general',
      label: 'General',
      toggles: [
        {
          key: 'openAtLogin',
          title: 'Open at login',
          description: 'Start xcreenshot when you log in.',
        },
        {
          key: 'checkForUpdates',
          title: 'Check for updates',
          description: 'Get new versions automatically.',
        },
      ],
    },
  ]

  let active = $state('shortcuts')
  const version = getVersion().catch(() => '')
</script>

<div class="flex h-screen bg-background text-foreground select-none">
  <nav
    aria-label="Settings sections"
    class="flex w-48 shrink-0 flex-col gap-0.5 border-r bg-sidebar p-3 text-sm"
  >
    {#each sections as s (s.id)}
      <a
        href={`#${s.id}`}
        aria-current={active === s.id ? 'true' : undefined}
        onclick={() => (active = s.id)}
        class="rounded-md px-2.5 py-2 text-muted-foreground no-underline hover:bg-sidebar-accent aria-[current=true]:bg-sidebar-accent aria-[current=true]:font-medium aria-[current=true]:text-foreground"
      >
        {s.label}
      </a>
    {/each}
    <div class="grow"></div>
    {#await version then v}
      {#if v}<div class="px-2.5 py-2 text-xs text-muted-foreground">Version {v}</div>{/if}
    {/await}
  </nav>

  <main class="flex min-w-0 grow flex-col gap-6 overflow-y-auto px-9 py-7">
    {#each sections as s, i (s.id)}
      {#if i > 0}<Separator />{/if}
      <section id={s.id} class="flex flex-col gap-3.5">
        <h2 class="text-sm font-semibold">{s.label}</h2>
        {#if s.id === 'shortcuts'}
          <!-- Display only; recording a new shortcut comes in #21. -->
          <SettingRow title="Take a screenshot" description="Works from any app.">
            <KbdGroup>
              {#each shortcutKeys(settings.shortcut) as key, k (k)}<Kbd>{key}</Kbd>{/each}
            </KbdGroup>
          </SettingRow>
        {/if}
        {#each s.toggles as t (t.key)}
          <SettingRow title={t.title} description={t.description} for={t.key}>
            <Switch id={t.key} bind:checked={settings[t.key]} />
          </SettingRow>
        {/each}
      </section>
    {/each}
  </main>
</div>
