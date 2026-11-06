<script lang="ts">
  import { getVersion } from '@tauri-apps/api/app'
  import { invoke } from '@tauri-apps/api/core'
  import { Button } from '$lib/components/ui/button'
  import { Separator } from '$lib/components/ui/separator'
  import { Switch } from '$lib/components/ui/switch'
  import SettingRow from '$lib/components/SettingRow.svelte'
  import ShortcutRecorder from '$lib/components/ShortcutRecorder.svelte'
  import { setSetting, settings, type ToggleKey } from '$lib/settings.svelte'

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

  /** Why the last switch change failed, shown under that switch. */
  let failed = $state<{ key: ToggleKey; message: string }>()

  /** Switches being saved; each is disabled until its save answers, so a double click can't
   * send on and then off. */
  let saving = $state<Partial<Record<ToggleKey, boolean>>>({})

  /** Switches show the saved value, so one that couldn't be saved flips back. */
  function toggle(key: ToggleKey, on: boolean) {
    failed = undefined
    saving[key] = true
    setSetting(key, on)
      .catch((e: unknown) => {
        failed = { key, message: typeof e === 'string' ? e : "Couldn't save the settings." }
      })
      .finally(() => (saving[key] = false))
  }

  /** The login item waits for the user to allow it in System Settings (macOS). */
  let needsApproval = $state(false)

  /** Checks started; only the latest one's answer counts. */
  let checks = 0

  /** Checks while Open at login is on: when it turns on and when the window gets focus. */
  function checkLoginItem() {
    if (!settings.openAtLogin) return
    // A focus check can overlap the effect's; only the latest one counts.
    const check = ++checks
    invoke<boolean>('login_item_needs_approval')
      .then((needs) => {
        if (check === checks) needsApproval = needs
      })
      .catch((e: unknown) => console.error('login_item_needs_approval failed', e))
  }

  function openLoginItems() {
    invoke('open_login_items').catch((e: unknown) => console.error('open_login_items failed', e))
  }

  $effect(checkLoginItem)

  let active = $state('shortcuts')
  const version = getVersion().catch(() => '')
</script>

<svelte:window onfocus={checkLoginItem} />

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
          <SettingRow title="Take a screenshot" description="Works from any app.">
            <ShortcutRecorder
              accelerator={settings.shortcut}
              onchange={(a) => setSetting('shortcut', a)}
              onreset={() => setSetting('shortcut', null)}
            />
          </SettingRow>
        {/if}
        {#each s.toggles as t (t.key)}
          <SettingRow title={t.title} description={t.description} for={t.key}>
            <Switch
              id={t.key}
              disabled={saving[t.key]}
              bind:checked={() => settings[t.key], (on) => toggle(t.key, on)}
            />
          </SettingRow>
          {#if failed?.key === t.key}
            <p role="alert" class="text-[13px] text-destructive">{failed.message}</p>
          {/if}
          {#if t.key === 'openAtLogin' && settings.openAtLogin && needsApproval}
            <div class="flex items-center gap-3 text-[13px] text-muted-foreground">
              <span class="grow">Allow xcreenshot in System Settings › Login Items.</span>
              <Button size="sm" variant="outline" onclick={openLoginItems}>
                Open System Settings
              </Button>
            </div>
          {/if}
        {/each}
      </section>
    {/each}
  </main>
</div>
