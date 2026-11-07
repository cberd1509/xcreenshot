<script lang="ts">
  import { failed } from '$lib/log'
  import { Button } from '$lib/components/ui/button'
  import SettingRow from '$lib/components/SettingRow.svelte'
  import ShortcutKbd from '$lib/components/ShortcutKbd.svelte'
  import { onDestroy } from 'svelte'
  import { comboFromEvent, comboProblem, spokenKeys, toAccelerator, type Combo } from '$lib/keys'
  import { pauseShortcut, resumeShortcut } from '$lib/settings.svelte'

  let {
    title,
    description,
    accelerator,
    onchange,
    onreset,
  }: {
    title: string
    description: string
    accelerator: string
    /** Saves the new shortcut; rejects with the message to show. */
    onchange: (accelerator: string) => Promise<void>
    onreset: () => Promise<void>
  } = $props()

  let recording = $state(false)
  /** The keys held right now while recording. */
  let held = $state('')
  /** A full shortcut, saved on key-up or Enter. */
  let pending: Combo | null = null
  let error = $state('')

  function clear() {
    held = ''
    pending = null
  }

  const NOT_CHANGED = "Couldn't change the shortcut."

  /** Letting go of the shortcut or saving: the buttons wait, so actions run in click order. */
  let busy = $state(false)
  /** Recording was asked for and not cancelled since. */
  let wanted = false

  const resume = () => resumeShortcut().catch(failed('resume_capture_shortcut'))

  /** Records once the capture shortcut is let go, so pressing it reaches this page. If it
   * can't be let go, says so (the old shortcut still works). */
  async function start() {
    busy = true
    wanted = true
    error = ''
    try {
      await pauseShortcut()
      // Cancelled (window left or closed) while letting go: take it back.
      if (!wanted) return void (await resume())
      clear()
      recording = true
    } catch (e) {
      failed('pause_capture_shortcut')(e)
      error = NOT_CHANGED
    } finally {
      busy = false
    }
  }

  /** Stops recording and takes the capture shortcut back (before any new one is saved). */
  async function stop() {
    wanted = false
    if (!recording) return
    recording = false
    clear()
    await resume()
  }

  onDestroy(() => void stop())

  async function save(run: () => Promise<void>) {
    busy = true
    try {
      await stop()
      error = ''
      await run()
    } catch (e) {
      error = typeof e === 'string' ? e : NOT_CHANGED
    } finally {
      busy = false
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (!recording) return
    e.preventDefault()
    e.stopPropagation()
    const combo = comboFromEvent(e)
    const bare = combo.modifiers.length === 0
    if (bare && combo.key === 'Escape') return void stop()
    if (bare && combo.key === 'Enter' && pending) return void confirm(pending)
    held = toAccelerator(combo)
    if (!combo.key) return
    const problem = comboProblem(combo.modifiers, combo.key)
    error = problem ?? ''
    pending = problem ? null : combo
  }

  function onkeyup(e: KeyboardEvent) {
    if (!recording) return
    e.preventDefault()
    // Windows only sends key-up for PrintScreen.
    const combo = pending ?? comboFromEvent(e)
    if (combo.key && !comboProblem(combo.modifiers, combo.key)) return void confirm(combo)
    held = toAccelerator({ modifiers: comboFromEvent(e).modifiers, key: null })
  }

  function confirm(combo: Combo) {
    return save(() => onchange(toAccelerator(combo)))
  }
</script>

<svelte:window {onkeydown} {onkeyup} onblur={() => void stop()} />

{#snippet below()}
  <div class="flex flex-col gap-1 text-[13px]">
    <Button
      variant="link"
      class="h-auto self-start p-0 text-[13px] font-normal text-muted-foreground"
      disabled={busy}
      onclick={() => (recording ? void stop() : save(onreset))}
    >
      {recording ? 'Cancel' : 'Reset to default'}
    </Button>
    {#if error}
      <p role="alert" class="text-destructive">{error}</p>
    {/if}
  </div>
{/snippet}

<SettingRow {title} {description} {below}>
  <Button
    variant="outline"
    aria-label={recording
      ? 'Recording a shortcut, press to cancel'
      : `Change shortcut, currently ${spokenKeys(accelerator)}`}
    disabled={busy}
    onclick={() => (recording ? void stop() : start())}
    class="h-9 min-w-24 gap-1 rounded-md px-2.5"
  >
    <span aria-live="polite" class="contents">
      {#if recording && !held}
        <span class="text-[13px] text-muted-foreground">Type a shortcut</span>
      {:else}
        <ShortcutKbd accelerator={recording ? held : accelerator} />
      {/if}
    </span>
  </Button>
</SettingRow>
