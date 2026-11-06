# Annotations

The annotation layer lives in `src/overlay/annotate/`. Konva draws it; the pure modules hold the
logic.

| File               | What it does                                                        |
| ------------------ | ------------------------------------------------------------------- |
| `shapes.ts`        | Shape model, tools and their shortcuts, drawing and transform math  |
| `history.ts`       | Undo and redo as snapshots of the shape list (last 200)             |
| `placement.ts`     | Where the toolbar goes                                              |
| `board.ts`         | The Konva stage, and `exportShapes`                                 |
| `Annotator.svelte` | Pointer input, history, the toolbar, and the `key`/`escape` API     |
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

Bottom to top: frozen frame, annotation stage (covers only the selection, so it clips), the
selection outline and handles (only the handles take presses), then the size badge and toolbar.

The overlay owns the keyboard and asks the annotator first:

- `key(e)` handles ⌘Z / ⇧⌘Z, Delete/Backspace and arrow keys on the selected shape, and the tool
  letters. It returns whether it handled the key.
- `escape()` undoes one step: the shape being drawn, an open menu, the selected shape, then the
  tool. Only when it returns `false` does Esc clear the selection or close the overlay.

While there are annotations, a press outside the selection is ignored rather than starting a new
selection. They go on Esc (with the selection), on the next capture, and when the user starts a
new selection on another display.

## Speed

Konva and the toolbar are separate chunks, fetched when the page is idle (the overlay windows
load at startup, long before the hotkey). The stage is built on the first selection. If Konva
fails to load, the tools show as unavailable and the next selection change tries again.

## Export contract

The final image is composed in Rust at native resolution. The page sends the annotations on
their own:

```ts
annotator.exportAnnotations(crop: Rect, scale: number): Promise<AnnotationExport>

type AnnotationExport = {
  rgba: Uint8ClampedArray | null // null: nothing drawn inside the crop
  x: number // where the RGBA goes, relative to the crop's top-left
  y: number
  width: number
  height: number
  blurs: Rect[]
}
```

- `crop` is the physical rect being captured on the frame (`toPhysical(selection)`), and `scale`
  is physical px per logical px.
- `rgba` covers only the box the shapes paint (strokes and arrow heads included), clipped to the
  crop: `width × height` pixels, row by row, **straight (not premultiplied) alpha**, transparent
  around the shapes. It has every shape except blur.
- `blurs` are rects in physical px relative to the crop, rounded outwards to whole pixels and
  clipped to it.

## Copy and save request

Copy (Enter, ⌘C), Save (⌘S) and the toolbar's Copy and Save buttons export the annotations for
the selection's physical rect and send one raw IPC request (`overlay/annotate/output.ts`):

```ts
invoke('overlay_copy' | 'overlay_save', body, { headers: { 'x-xcreenshot-output': json } })
```

- `body` is the `rgba` bytes as a `Uint8Array` (empty without a layer). It reaches Rust as
  `InvokeBody::Raw`, never as JSON.
- `json` is `{ session, rect, annotations }`: `rect` is the selection in physical px, and
  `annotations` is `null` or `{ x, y, width, height, blurs }` from the export above.

Rust (`overlay/compose.rs`) checks the body is exactly `width × height × 4` bytes, crops the
frame, then:

1. pixelates each rect in `blurs`, destructively, in blocks of at least 16 px (larger for large
   rects), so nothing under a blur survives (issue #16);
2. draws the layer at (`x`, `y`) with straight-alpha source-over, clipped to the crop;
3. encodes the PNG and copies or saves it.

Share (#19) will send the same request.
