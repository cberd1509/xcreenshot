<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { Kbd } from '$lib/components/ui/kbd'
  import ShortcutKbd from '$lib/components/ShortcutKbd.svelte'
  import { onDestroy } from 'svelte'
  import { comboFromEvent, comboProblem, toAccelerator, type Combo } from '$lib/keys'
  import { pauseShortcut, resumeShortcut } from '$lib/settings.svelte'

  let {
    accelerator,
    onchange,
    onreset,
  }: {
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

  const failed = (command: string) => (e: unknown) => console.error(`${command} failed`, e)

  function clear() {
    held = ''
    pending = null
  }

  const NOT_CHANGED = "Couldn't change the shortcut."

  /** Records: the capture shortcut is let go, so pressing it reaches this page. If it can't
   * be let go, recording stops and says so (the old shortcut still works). */
  function start() {
    clear()
    recording = true
    error = ''
    pauseShortcut().catch((e: unknown) => {
      failed('pause_capture_shortcut')(e)
      recording = false
      clear()
      error = NOT_CHANGED
    })
  }

  /** Stops recording and takes the capture shortcut back (before any new one is saved). */
  async function stop() {
    if (!recording) return
    recording = false
    clear()
    await resumeShortcut().catch(failed('resume_capture_shortcut'))
  }

  onDestroy(() => void stop())

  async function save(run: () => Promise<void>) {
    await stop()
    error = ''
    try {
      await run()
    } catch (e) {
      error = typeof e === 'string' ? e : NOT_CHANGED
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

<div class="flex flex-col items-end gap-1.5">
  <div class="flex items-center gap-2">
    {#if recording}
      <span aria-live="polite" class="text-[13px] text-muted-foreground">
        {#if held}<ShortcutKbd accelerator={held} />{:else}<Kbd>Type a shortcut</Kbd>{/if}
      </span>
      <Button size="sm" variant="outline" onclick={() => void stop()}>Cancel</Button>
    {:else}
      <ShortcutKbd {accelerator} />
      <Button size="sm" variant="outline" onclick={start}>Change</Button>
    {/if}
  </div>
  {#if !recording}
    <Button size="sm" variant="link" class="h-auto px-0" onclick={() => save(onreset)}>
      Reset to default
    </Button>
  {/if}
  {#if error}
    <p role="alert" class="text-[13px] text-destructive">{error}</p>
  {/if}
</div>
