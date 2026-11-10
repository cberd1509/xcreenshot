<script lang="ts">
  import LockIcon from '@lucide/svelte/icons/link-2'
  import { lockedSize, type Size } from './geometry'
  import { shortcut } from './keys'

  /**
   * The selection size in physical px, as two fields: type an exact size and press Enter.
   * `editing` is bindable so Esc can cancel the edit; `lock` keeps the aspect ratio, here and
   * when the selection is resized.
   */
  let {
    size,
    onresize,
    editing = $bindable(false),
    lock = $bindable(false),
  }: {
    size: Size
    onresize: (size: Size, keepRatio: boolean) => void
    editing?: boolean
    lock?: boolean
  } = $props()

  let width = $state<number | null>(0)
  let height = $state<number | null>(0)
  let ratio = 1
  let form = $state<HTMLFormElement>()

  // The fields show the selection's size, except while they are being typed in.
  $effect.pre(() => {
    if (editing) return
    width = size.width
    height = size.height
  })

  // An edit cancelled from outside (Esc) leaves the fields.
  $effect(() => {
    const active = document.activeElement
    if (!editing && active instanceof HTMLElement && form?.contains(active)) active.blur()
  })

  function onfocusin(e: FocusEvent) {
    if (editing || !(e.target instanceof HTMLInputElement)) return
    ratio = size.width / size.height
    editing = true
  }

  function typed(side: keyof Size) {
    const value = side === 'width' ? width : height
    if (!lock || !(value !== null && value >= 1)) return
    ;({ width, height } = lockedSize(side, value, ratio))
  }

  function apply() {
    const next = { width: width ?? 0, height: height ?? 0 }
    editing = false
    if (next.width >= 1 && next.height >= 1) onresize(next, lock)
  }

  function onsubmit(e: SubmitEvent) {
    e.preventDefault()
    apply()
  }

  // Copy and Save go on to the capture (see `fieldShortcut`), with the size typed so far.
  function onkeydown(e: KeyboardEvent) {
    if (shortcut(e, 'c') || shortcut(e, 's')) apply()
  }

  // Leaving the form without Enter cancels the edit.
  function onfocusout(e: FocusEvent) {
    if (!form?.contains(e.relatedTarget as Node | null)) editing = false
  }

  function toggle() {
    lock = !lock
    ratio = size.width / size.height
  }

  const FIELD =
    'h-[26px] w-[52px] rounded-[5px] border border-zinc-700 bg-zinc-950 px-1.5 text-xs text-zinc-50 outline-none focus:border-(--selection)'
</script>

<form
  bind:this={form}
  class="flex h-[34px] items-center gap-1 rounded-lg border border-zinc-800 bg-zinc-900 p-[3px] font-mono text-xs text-zinc-400 tabular-nums"
  role="group"
  aria-label="Selection size"
  data-testid="size-badge"
  data-size-badge
  {onsubmit}
  {onfocusin}
  {onfocusout}
>
  <label for="size-width" class="pl-1.5">W</label>
  <input
    id="size-width"
    class={FIELD}
    type="number"
    min="1"
    inputmode="numeric"
    aria-label="Width"
    bind:value={width}
    oninput={() => typed('width')}
    {onkeydown}
  />
  <button
    type="button"
    class="flex size-[26px] items-center justify-center rounded-[5px] hover:bg-zinc-800 {lock
      ? 'text-zinc-50'
      : 'text-zinc-500'}"
    aria-label="Lock aspect ratio"
    aria-pressed={lock}
    title="Lock aspect ratio"
    onclick={toggle}
    onmousedown={(e) => e.preventDefault()}
  >
    <LockIcon class="size-3.5" />
  </button>
  <label for="size-height">H</label>
  <input
    id="size-height"
    class={FIELD}
    type="number"
    min="1"
    inputmode="numeric"
    aria-label="Height"
    bind:value={height}
    oninput={() => typed('height')}
    {onkeydown}
  />
  <span class="pr-1.5">px</span>
  <button type="submit" hidden aria-hidden="true"></button>
</form>

<style>
  /* Plain fields, as designed: no spin buttons. */
  input {
    appearance: textfield;
  }
  input::-webkit-inner-spin-button,
  input::-webkit-outer-spin-button {
    appearance: none;
    margin: 0;
  }
</style>
