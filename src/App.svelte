<script lang="ts">
  import { getVersion } from '@tauri-apps/api/app'
  import { invoke } from '@tauri-apps/api/core'
  import { Button } from '$lib/components/ui/button'
  import { Separator } from '$lib/components/ui/separator'
  import { Switch } from '$lib/components/ui/switch'
  import SettingRow from '$lib/components/SettingRow.svelte'
  import ShortcutRecorder from '$lib/components/ShortcutRecorder.svelte'
  import { isMac, isWindows } from '$lib/keys'
  import { sectionInView } from '$lib/scrollspy'
  import { setSetting, settings, type ToggleKey } from '$lib/settings.svelte'

  type Toggle = { key: ToggleKey; title: string; description: string }

  const loginStarts = isWindows ? 'sign in' : isMac ? 'turn on your Mac' : 'log in'

  /** Sections in page and sidebar order. */
  const sections: { id: string; label: string; toggles: Toggle[] }[] = [
    {
      id: 'general',
      label: 'General',
      toggles: [
        {
          key: 'openAtLogin',
          title: 'Open at login',
          description: `Start xcreenshot when you ${loginStarts}.`,
        },
        {
          key: 'checkForUpdates',
          title: 'Check for updates',
          description: 'Get new versions automatically.',
        },
      ],
    },
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
    { id: 'about', label: 'About', toggles: [] },
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
    // A focus check can overlap the effect's; only the latest one counts. Turning Open at
    // login off drops any check still running.
    const check = ++checks
    if (!settings.openAtLogin) {
      needsApproval = false
      return
    }
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

  /** Opens the repo in the browser (the page can't leave the app). */
  function openRepo() {
    invoke('open_repo').catch((err: unknown) => console.error('open_repo failed', err))
  }

  /** The sidebar marks the section in view; a clicked one stays marked while it's visible. */
  let active = $state(sections[0].id)
  let clicked: string | undefined
  let page: HTMLElement

  function onscroll() {
    const { scrollTop: top, clientHeight, scrollHeight } = page
    const spans = sections.map(({ id }) => {
      const el = document.getElementById(id)!
      return { id, top: el.offsetTop, bottom: el.offsetTop + el.offsetHeight }
    })
    const view = {
      top,
      bottom: top + clientHeight,
      // Just under the page's top padding.
      line: top + 40,
      atBottom: top + clientHeight >= scrollHeight - 1,
    }
    active = sectionInView(spans, view, clicked)!
    if (active !== clicked) clicked = undefined
  }

  function select(id: string) {
    clicked = active = id
  }
  const version = getVersion().catch(() => '')

  // 44 × 24 px track with a 20 px thumb, as designed.
  const switchSize =
    'data-[size=default]:h-6 data-[size=default]:w-11 px-px [&_[data-slot=switch-thumb]]:size-5! [&_[data-slot=switch-thumb][data-state=checked]]:translate-x-full! [&[data-state=unchecked]_[data-slot=switch-thumb]]:shadow-[0_1px_2px_rgba(0,0,0,0.15)]'
</script>

<svelte:window onfocus={checkLoginItem} />

<div class="flex h-screen bg-background leading-[normal] text-foreground select-none">
  <nav
    aria-label="Settings sections"
    class="flex w-[200px] shrink-0 flex-col gap-0.5 border-r bg-sidebar px-3 py-4 text-[13px]"
  >
    {#each sections as s (s.id)}
      <a
        href={`#${s.id}`}
        aria-current={active === s.id ? 'page' : undefined}
        onclick={() => select(s.id)}
        class="rounded-md px-2.5 py-2 text-subtle-foreground no-underline hover:bg-sidebar-accent aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium aria-[current=page]:text-foreground"
      >
        {s.label}
      </a>
    {/each}
    <div class="grow"></div>
    {#await version then v}
      {#if v}<div class="px-2.5 py-2 text-xs/[normal] text-muted-foreground">Version {v}</div>{/if}
    {/await}
  </nav>

  <main
    bind:this={page}
    {onscroll}
    class="relative flex min-w-0 grow flex-col gap-[26px] overflow-y-auto px-9 py-7"
  >
    {#each sections as s, i (s.id)}
      {#if i > 0}<Separator />{/if}
      <section id={s.id} class="flex flex-col gap-3.5">
        <h2 class="text-sm/[normal] font-semibold">{s.label}</h2>
        {#if s.id === 'shortcuts'}
          <ShortcutRecorder
            title="Take a screenshot"
            description="Works from any app."
            accelerator={settings.shortcut}
            onchange={(a) => setSetting('shortcut', a)}
            onreset={() => setSetting('shortcut', null)}
          />
        {:else if s.id === 'about'}
          <div class="flex flex-col gap-1 text-[13px] text-muted-foreground">
            <div class="text-sm/[normal] font-medium text-foreground">xcreenshot</div>
            {#await version then v}{#if v}<div>Version {v}</div>{/if}{/await}
            <!-- A button, not a link: the page must never navigate to the site itself. -->
            <button
              type="button"
              onclick={openRepo}
              class="cursor-pointer self-start text-blue-600 hover:text-blue-700 dark:text-blue-400"
            >
              github.com/cberd1509/xcreenshot
            </button>
            <div>MIT License</div>
          </div>
        {/if}
        {#each s.toggles as t (t.key)}
          <SettingRow title={t.title} description={t.description} for={t.key}>
            <Switch
              id={t.key}
              class={switchSize}
              disabled={saving[t.key]}
              bind:checked={() => settings[t.key], (on) => toggle(t.key, on)}
            />
            {#snippet below()}
              {#if failed?.key === t.key}
                <p role="alert" class="text-[13px] text-destructive">{failed.message}</p>
              {/if}
              {#if t.key === 'openAtLogin' && settings.openAtLogin && needsApproval}
                <p class="flex items-center gap-1 text-[13px] text-muted-foreground">
                  Allow xcreenshot in System Settings › Login Items.
                  <Button
                    variant="link"
                    class="h-auto p-0 text-[13px] font-normal text-foreground"
                    onclick={openLoginItems}
                  >
                    Open System Settings
                  </Button>
                </p>
              {/if}
            {/snippet}
          </SettingRow>
        {/each}
      </section>
    {/each}
  </main>
</div>
