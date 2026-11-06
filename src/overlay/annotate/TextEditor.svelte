<script lang="ts">
  import type { Point, Size } from '../geometry'
  import { MAX_TEXT, radius, textColors, textFont } from './text'

  /**
   * Edits a text annotation in place: a textarea over the board, laid out like the text it
   * becomes. It takes focus when it opens; losing it commits (`oncommit`). The annotator owns
   * the keyboard meanwhile, so typing reaches only the textarea.
   */
  let {
    text = $bindable(),
    at,
    fontSize,
    background,
    color,
    measure,
    oncommit,
  }: {
    text: string
    /** The box's top-left corner, in window px. */
    at: Point
    fontSize: number
    background: boolean
    color: string
    /** The size Konva gives the text, so the box matches what it commits to. */
    measure: (text: string, fontSize: number) => Size
    oncommit: () => void
  } = $props()

  // A paste longer than MAX_TEXT is cut by `maxlength`; only what is kept is measured.
  const size = $derived(measure(text.slice(0, MAX_TEXT), fontSize))
  const colors = $derived(textColors(color, background))
  const font = $derived(textFont(fontSize))

  /**
   * Losing focus commits, unless it went to a control that gives it back (`data-keeps-text`,
   * the hex color field).
   */
  function onblur(e: FocusEvent) {
    const to = e.relatedTarget
    if (!(to instanceof Element && to.closest('[data-keeps-text]'))) oncommit()
  }

  /** Focused on open, with the caret at the end. */
  function focus(el: HTMLTextAreaElement) {
    el.focus()
    el.setSelectionRange(el.value.length, el.value.length)
  }
</script>

<textarea
  class="editor"
  aria-label="Text"
  spellcheck="false"
  wrap="off"
  maxlength={MAX_TEXT}
  bind:value={text}
  {@attach focus}
  style:left="{at.x}px"
  style:top="{at.y}px"
  style:width="{size.width + 2}px"
  style:height="{size.height}px"
  style:padding="{font.padding}px"
  style:border-radius="{radius(fontSize)}px"
  style:font="{fontSize}px/{font.lineHeight} {font.fontFamily}"
  style:color={colors.fill}
  style:caret-color={colors.fill}
  style:background={colors.box}
  {onblur}
  onpointerdown={(e) => e.stopPropagation()}></textarea>

<style>
  /* Matches the Konva text it commits to: same box, font and line height. */
  .editor {
    position: fixed;
    z-index: 1;
    box-sizing: border-box;
    margin: 0;
    border: none;
    /* White dashes on a dark line, visible on any screen. */
    outline: 1px dashed white;
    box-shadow: 0 0 0 1px rgb(0 0 0 / 0.5);
    resize: none;
    overflow: hidden;
    white-space: pre;
  }
</style>
