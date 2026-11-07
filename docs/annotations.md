# Annotations

The annotation layer lives in `src/overlay/annotate/`. Konva draws it; the pure modules hold the
logic.

| File               | What it does                                                        |
| ------------------ | ------------------------------------------------------------------- |
| `shapes.ts`        | Shape model, tools and their shortcuts, drawing and transform math  |
| `text.ts`          | Text sizes, font, padding and colors                                |
| `history.ts`       | Undo and redo as snapshots of the shape list (last 200)             |
| `memory.ts`        | The last color and size, in localStorage                            |
| `blocks.ts`        | The blur preview's blocks, as Rust paints them                      |
| `placement.ts`     | Where the tool rail and the action bar go                           |
| `board.ts`         | The Konva stage, and `exportShapes`                                 |
| `Annotator.svelte` | Pointer input, history, the toolbar, and the API the overlay calls |
| `Toolbar.svelte`   | The toolbar: rail and action bar, placed (loaded on demand)         |
| `Rail.svelte`      | The vertical tool rail: Select, the tools, color, undo, redo, clear |
| `ColorMenu.svelte` | The color popover: swatches, a hex field and the stroke size        |
| `ActionBar.svelte` | More (options), Share, Save, Copy and Cancel                        |
| `Tip.svelte`       | A tooltip with the label and its key                                |
| `TextOptions.svelte` | The text tool's size and background, in the rail                  |
| `TextEditor.svelte`  | The textarea that edits a text in place                           |

## Shape model

A shape is plain JSON: `{ id, kind, style: { color, size }, ...geometry }`, in **logical px
relative to the display**, the same space as the selection. Shapes stay on the same pixels of
the frozen frame when the selection moves or resizes; the selection only clips what is shown and
exported. A shape dragged out of the selection is pulled back until 8 px of it shows.

- `rect`, `ellipse` (inside the box) and `blur`: `x`, `y`, `width`, `height`
- `line` and `arrow`: `points: [x1, y1, x2, y2]`, an arrow's head at the end
- `pen` and `highlighter`: `points: [x1, y1, x2, y2, …]`, freehand
- `text`: `x`, `y`, `width`, `height` (its box, measured by Konva), `text`, `fontSize` and
  `background`. Font sizes are logical px (S 16, M 24, L 36), so they look the same on any
  display. Resizing scales the font; the box is measured again.

## Tools

- **Pen** keeps a point once the pointer has moved 2 px, and smooths the stroke with two passes
  of Chaikin corner cutting (`smooth`). `Freehand` smooths as points come, redoing only the
  segments a new point touches. Smoothing never leaves the box of the points, so the painted
  box stays exact.
- **Highlighter** is a pen 4× as wide, at 40% opacity, with flat ends. It draws the same way on
  screen and in the export (straight-alpha source-over), so text under it stays readable.
- **Shift** (`constrain`): lines and arrows snap to 15° steps; rects, ellipses and blurs become
  squares and circles. The end is pulled back along the drag to stay in the selection, so the
  shape keeps its angle or stays square. Pressing or letting go of Shift mid-drag updates the
  shape without a pointer move.
- **Blur** shows the blocks Rust will paint: the same block size, from the corner of the part
  of the blur inside the selection (`blocks.ts` mirrors `block_size` and `spans` in
  `output/compose.rs`; their tests pin the same values). Each block is the average color of
  the frame under it, read from a copy shrunk 16× per side (halved step by step, so every
  pixel counts), made on the first blur after each new frame. Blurs are worked out in model
  order over one copy of it, each painting its blocks, so a blur over another averages its
  blocks as in Rust; the blur being drawn comes last. The preview is only for the screen: the
  export is Rust's pixelation (below). Blurs always sit under the other shapes, because Rust
  pixelates the frame before it draws them. There can be at most 256 (Rust's `MAX_BLURS`): the
  tool says so and adds no more, and `blurRects` never sends more.

The color popover has eight preset colors, a hex field for any other (`#RGB` or `#RRGGBB`; in
the page, as the system color panel would open under the overlay) and three sizes. With a shape selected, a new color
or size restyles it as one undo step (text takes only the color; a blur has neither); the tool
takes it too. The last color and size are kept in localStorage (`memory.ts`), each on its own,
so a window that changes one never overwrites another window's newer choice of the other. They
come back on the next capture, and are overlay state, not settings. The tool is not kept: every
capture starts in Select, so a capture opened by mistake closes with one Esc.

Only the top layer redraws while drawing, and a pen stroke only when it gains a point. Drawing
a 5000-point stroke smooths it in about 3 ms in all (smoothing it whole on every point took
about 630 ms).

## Layers and input

Bottom to top: frozen frame, annotation stage, the selection outline and handles (only the
handles take presses), then the size badge and toolbar.

The Konva stage covers the display while there is a selection (0×0 otherwise). Shapes sit on a
layer clipped to the selection. The shape being drawn and the resize handles sit on a small top
layer, unclipped, so handles at the selection's edge stay reachable, and drawing redraws only
that layer. Presses the annotations don't use (outside the selection, or with no tool) go on to
the overlay. The overlay focuses its window on every press, in the capture phase.

The overlay owns the keyboard. Copy and Save (⌘C / ⌘S, Ctrl elsewhere) always work: they close
an open toolbar menu (`closeMenus()`) first, unless text is being typed (`typingText()`), when
they are the text box's. An open toolbar menu owns the other keys (`menuOpen()`); otherwise the
overlay asks the annotator first. A press on the board closes an open menu too, so drawing
dismisses it like a click outside:

- `key(e)` handles undo and redo (⌘Z / ⇧⌘Z on macOS, Ctrl elsewhere), Delete/Backspace and arrow
  keys on the selected shape, and the tool letters. It returns whether it handled the key.
- Esc undoes one thing per press: the size edit, then `escape()` (the shape being drawn, an open
  menu, the text being edited, the selected shape, the tool), then the selection, then the
  overlay closes. The tool counts only while there is a selection, and each capture starts in
  Select, so Esc with no selection closes the overlay.

While there are annotations (`annotated()`), a press outside the selection is ignored rather than
starting a new selection, and a resize cannot shrink the selection past what the shapes paint
inside it (`painted()`). Annotations go on Esc (with the selection), on the next capture, and when
a selection starts on another display.

## Text

With the Text tool, a click in the selection opens a textarea there, laid out like the Konva text
it becomes (system UI font, same padding and line height). Double-click a text, or press Enter
with it selected, to edit it again; it leaves the board meanwhile. Esc, a press elsewhere, or the
textarea losing focus commits it as one undo step (none if nothing changed), as typed (line
breaks and trailing spaces kept) and where the editor was: it is not moved back into the
selection, which clips it like any shape. Text that is all whitespace is dropped (an emptied text
is deleted). A text keeps at most 2,000 characters (`maxlength`, so a long paste is cut), and only
what is kept is measured. Copy and Save commit it first. A press on the board while editing only commits: it is
stopped in the capture phase, so Konva never sees it. Text resizes from its corners and keeps its
proportions (Shift does not stretch it); the font scales and the box is measured again.

While a text is edited, `key(e)` returns true for every key without `preventDefault`, so the
overlay runs no shortcut, copy, save or nudge and the textarea gets the key. Esc comes from Rust
as usual and `escape()` closes an open menu first, then commits the text. While text is typed,
presses on the toolbar and its color menu leave focus in the textarea (`keepFocus`), so the text
stays the active edit and follows the size, Background and color. A tool or Clear all commits it
first; Undo drops it, and Redo waits until it is committed (committing would drop what can be
redone). With a text selected, the toolbar shows its options and they apply to it, one undo step
each, leaving the tool's own options as they were; a text that grows is moved to sit wholly in
the selection, if it fits. With Background, the box is filled with the color and the text is
black or white, whichever has the higher WCAG contrast ratio.

## Speed

Konva and the toolbar are separate chunks, fetched when the page is idle (the overlay windows
load at startup, long before the hotkey). The stage is built on the first selection. If Konva
fails to load, the tools show as unavailable and the next selection change tries again.

## Export

The final image is composed in Rust at native resolution. The page renders the annotations on
their own:

```ts
annotator.exportAnnotations(crop: Rect, scale: Scale): Promise<AnnotationExport>

type AnnotationExport = {
  layer: { rgba: Uint8ClampedArray; x: number; y: number; width: number; height: number } | null
  blurs: Rect[]
}
```

- `crop` is the physical rect being captured on the frame (`toPhysical(selection)`), and `scale`
  is physical px per logical px, per axis.
- `layer` covers only the box the shapes paint (strokes and arrow heads included), clipped to the
  crop, at (`x`, `y`) relative to the crop: `width × height` pixels, row by row, **straight (not
  premultiplied) alpha**, transparent around the shapes. It has every shape except blur, and is
  `null` when nothing is drawn inside the crop.
- `blurs` are rects in physical px relative to the crop, rounded outwards to whole pixels and
  clipped to it.

## Copy, save and share request

Copy (Enter, ⌘C), Save (⌘S) and the toolbar's Copy, Save and Share buttons export the
annotations for the selection's physical rect and send one raw IPC request
(`overlay/annotate/output.ts`):

```ts
invoke('overlay_copy' | 'overlay_save' | 'overlay_share', body, {
  headers: { 'x-xcreenshot-output': json },
})
```

- `body` is the layer's `rgba` as a `Uint8Array` (empty without a layer). It reaches Rust as
  `InvokeBody::Raw`, never as JSON, and Rust reads the pixels in place.
- `json` is `{ session, rect, layer?: { x, y, width, height }, blurs, anchor?, remember? }`.
  `rect` is the selection in physical px; `layer` is there only when the body has its pixels.
  `anchor` is the Share button's rect in logical px, for a share only (see `docs/share.md`).
  `remember: false` keeps the region from being remembered: a quick copy sends it.

`overlay/export.rs` parses the request (the body must be exactly `width × height × 4` bytes, and
there may be at most 256 blurs, each clipped to the selection) and crops the frame. Then `output/compose.rs`:

1. pixelates each rect in `blurs`, destructively, in blocks of at least 16 px (larger for large
   rects; a shorter remainder joins the block before it). Each block's color matches none of the
   pixels it covers, so every pixel under a blur changes, even in a flat area (issue #16);
2. draws the layer at (`x`, `y`), straight-alpha source-over with rounding (an opaque crop stays
   opaque), clipped to the crop.

Then the PNG is encoded and copied or saved. Composing a full Retina screen (3024×1964) with a
layer over all of it and a 1000×600 blur takes about 24 ms in a release build.

Share (#19) will send the same request.

## Toasts

A copy, save or share that worked shows a toast ("Copied · W × H", "Saved · file name", "Shared ·
service") at the bottom of the display the capture was on (`src-tauri/src/toast/`). On macOS it
plays the system screenshot sound; Windows has none, so it is silent there. One window holds
the toasts: built hidden at startup, it never takes focus or clicks, and shows only while a
toast is up. Toasts sent while its page loads, or that fail to reach it, wait until the page
asks for them. The page (`src/toast/`) stacks them in the order of the actions: each fades
about 1.2 s after it shows, the same action again while its toast is up counts on it (×2, ×3),
and at most three show, the older ones smaller and dimmer.
