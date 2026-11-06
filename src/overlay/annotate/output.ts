// The copy and save request, pure. The annotation pixels go as the raw IPC body (never JSON);
// everything else goes in one header. Rust reads it in `overlay/export.rs`.

import type { Rect } from '../geometry'
import type { AnnotationExport } from './board'

/** Must match `HEADER` in `overlay/export.rs`. */
export const OUTPUT_HEADER = 'x-xcreenshot-output'

export type OutputRequest = { body: Uint8Array; headers: Record<string, string> }

/**
 * The request to copy or save `rect` (physical px) of `session`, with the annotations exported
 * for that rect, if any. The header has `layer` only when the body has its pixels.
 */
export function outputRequest(
  session: number,
  rect: Rect,
  out?: AnnotationExport | null,
): OutputRequest {
  const layer = out?.layer
  const head = {
    session,
    rect,
    blurs: out?.blurs ?? [],
    ...(layer && { layer: { x: layer.x, y: layer.y, width: layer.width, height: layer.height } }),
  }
  const body = layer
    ? new Uint8Array(layer.rgba.buffer, layer.rgba.byteOffset, layer.rgba.byteLength)
    : new Uint8Array(0)
  return { body, headers: { [OUTPUT_HEADER]: JSON.stringify(head) } }
}
