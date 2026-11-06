# Annotations

The annotation layer lives in `src/overlay/annotate/`. Konva draws it; the pure modules hold the
logic.

| File               | What it does                                                        |
| ------------------ | ------------------------------------------------------------------- |
| `shapes.ts`        | Shape model, tools and their shortcuts, drawing and transform math  |
| `history.ts`       | Undo and redo as snapshots of the shape list (last 200)             |
| `placement.ts`     | Where the toolbar goes                                              |
| `board.ts`         | The Konva stage, and `exportShapes`                                 |
| `Annotator.svelte` | Pointer input, history, the toolbar, and the API the overlay calls |
| `Toolbar.svelte`   | The toolbar (presentational, loaded on demand)                      |

## Shape model

A shape is plain JSON: `{ id, kind, style: { color, size }, ...geometry }`, in **logical px
relative to the display**, the same space as the selection. Shapes stay on the same pixels of
the frozen frame when the selection moves or resizes; the selection only clips what is shown and
exported. A shape dragged out of the selection is pulled back until 8 px of it shows.

- `rect` and `blur`: `x`, `y`, `width`, `height`
- `arrow`: `points: [x1, y1, x2, y2]`, head at the end

The other tools (#14–#16) add kinds here.

## Layers and input

Bottom to top: frozen frame, annotation stage, the selection outline and handles (only the
handles take presses), then the size badge and toolbar.

The Konva stage covers the display while there is a selection (0×0 otherwise). Shapes sit on a
layer clipped to the selection. The shape being drawn and the resize handles sit on a small top
layer, unclipped, so handles at the selection's edge stay reachable, and drawing redraws only
that layer. Presses the annotations don't use (outside the selection, or with no tool) go on to
the overlay. The overlay focuses its window on every press, in the capture phase.

The overlay owns the keyboard. An open toolbar menu owns it in turn (`menuOpen()`); otherwise
the overlay asks the annotator first:

- `key(e)` handles undo and redo (⌘Z / ⇧⌘Z on macOS, Ctrl elsewhere), Delete/Backspace and arrow
  keys on the selected shape, and the tool letters. It returns whether it handled the key.
- Esc undoes one thing per press: the size edit, then `escape()` (the shape being drawn, an open
  menu, the selected shape, the tool), then the selection, then the overlay closes.

While there are annotations (`annotated()`), a press outside the selection is ignored rather than
starting a new selection, and a resize cannot shrink the selection past what the shapes paint
inside it (`painted()`). Annotations go on Esc (with the selection), on the next capture, and when
a selection starts on another display.

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

## Copy and save request

Copy (Enter, ⌘C), Save (⌘S) and the toolbar's Copy and Save buttons export the annotations for
the selection's physical rect and send one raw IPC request (`overlay/annotate/output.ts`):

```ts
invoke('overlay_copy' | 'overlay_save', body, { headers: { 'x-xcreenshot-output': json } })
```

- `body` is the layer's `rgba` as a `Uint8Array` (empty without a layer). It reaches Rust as
  `InvokeBody::Raw`, never as JSON, and Rust reads the pixels in place.
- `json` is `{ session, rect, layer?: { x, y, width, height }, blurs }`. `rect` is the selection in
  physical px; `layer` is there only when the body has its pixels.

`overlay/export.rs` parses the request (the body must be exactly `width × height × 4` bytes) and
crops the frame. Then `output/compose.rs`:

1. pixelates each rect in `blurs`, destructively, in blocks of at least 16 px (larger for large
   rects), so nothing under a blur survives (issue #16);
2. draws the layer at (`x`, `y`), straight-alpha source-over with rounding (an opaque crop stays
   opaque), clipped to the crop.

Then the PNG is encoded and copied or saved. Composing a full Retina screen (3024×1964) with a
layer over all of it and a 1000×600 blur takes about 24 ms in a release build.

Share (#19) will send the same request.
