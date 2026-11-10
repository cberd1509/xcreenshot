// The copy, save and share request, pure. The annotation pixels go as the raw IPC body (never JSON);
// everything else goes in one header. Rust reads it in `overlay/export.rs`.

import type { Rect } from '../geometry'
import type { AnnotationExport } from './board'
import type { Factor } from './upscale'

/** Must match `HEADER` in `overlay/export.rs`. */
export const OUTPUT_HEADER = 'x-xcreenshot-output'

export type OutputRequest = { body: Uint8Array; headers: Record<string, string> }

/** How a copy, save or share differs from the plain one. */
export type OutputOptions = {
  /** The Share button, in logical px. */
  anchor?: Rect
  /** `false` when the region must not be remembered (a quick copy). */
  remember?: boolean
  /** A larger size from the Copy or Save menu; the layer must be exported at it. */
  factor?: Factor
}

/**
 * The request to copy, save or share `rect` (physical px) of `session`, with the annotations
 * exported for that rect, if any. The header has `layer` only when the body has its pixels,
 * `anchor` (the Share button, logical px) only for a share, `remember: false` only when the
 * region must not be remembered (a quick copy), and `factor` only for a larger size.
 */
export function outputRequest(
  session: number,
  rect: Rect,
  out?: AnnotationExport | null,
  { anchor, remember = true, factor = 1 }: OutputOptions = {},
): OutputRequest {
  const layer = out?.layer
  const head = {
    session,
    rect,
    blurs: out?.blurs ?? [],
    ...(layer && { layer: { x: layer.x, y: layer.y, width: layer.width, height: layer.height } }),
    ...(anchor && { anchor }),
    ...(!remember && { remember }),
    ...(factor !== 1 && { factor }),
  }
  const body = layer
    ? new Uint8Array(layer.rgba.buffer, layer.rgba.byteOffset, layer.rgba.byteLength)
    : new Uint8Array(0)
  return { body, headers: { [OUTPUT_HEADER]: JSON.stringify(head) } }
}
