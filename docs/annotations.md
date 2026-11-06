# Annotations

The annotation layer lives in `src/overlay/annotate/`. Konva draws it; the pure modules hold the
logic.

| File               | What it does                                                         |
| ------------------ | -------------------------------------------------------------------- |
| `shapes.ts`        | Shape model, tools and their shortcuts, drawing and transform math   |
| `history.ts`       | Undo and redo as a command stack (`add`, `update`, `remove`, `clear`) |
| `placement.ts`     | Where the toolbar goes                                               |
| `board.ts`         | The Konva stage, and `exportShapes`                                  |
| `Annotator.svelte` | Pointer and keyboard input, history, toolbar, export                 |
| `Toolbar.svelte`   | The toolbar (presentational)                                         |

## Shape model

A shape is plain JSON: `{ id, kind, style: { color, size }, ...geometry }`, in **logical px
relative to the selection's top-left corner**. Moving or resizing the selection moves the Konva
group, never the shapes, so they stay put on the selection.

- `rect` and `blur`: `x`, `y`, `width`, `height`
- `arrow`: `points: [x1, y1, x2, y2]`, head at the end

The other tools (#14–#16) add kinds here.

## Speed

Konva is a separate chunk. It is loaded and the stage built on the first selection, never when
the overlay shows, so hotkey-to-paint does not change.

## Export contract

The final image is composed in Rust at native resolution. The page sends the annotations on
their own:

```ts
annotator.exportAnnotations(crop: Rect, scale: number): Promise<AnnotationExport>

type AnnotationExport = {
  rgba: Uint8ClampedArray | null // null: nothing to draw
  width: number // = crop.width
  height: number // = crop.height
  blurs: Rect[]
}
```

- `crop` is the physical rect being captured on the frame (`toPhysical(selection)`), and `scale`
  is physical px per logical px.
- `rgba` is `width × height` pixels, row by row, **straight (not premultiplied) alpha**, on a
  transparent background. It has every shape except blur.
- `blurs` are rects in physical px of the crop (origin at the crop's top-left), rounded outwards
  to whole pixels and clipped to the crop.

Rust then, on the cropped frame:

1. pixelates each rect in `blurs`;
2. draws `rgba` over it with source-over blending;
3. encodes the PNG and copies, saves or shares it.

The crop's edges are rounded to whole pixels, so the shapes can sit up to half a physical pixel
off from where they were drawn.
