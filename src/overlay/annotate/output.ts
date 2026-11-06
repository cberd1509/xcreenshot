// The copy and save request, pure. The annotation pixels go as the raw IPC body (never JSON);
// everything else goes in one header. Rust reads it in `overlay/compose.rs`.

import type { Rect } from '../geometry'
import type { AnnotationExport } from './board'

/** Must match `compose::HEADER` in Rust. */
export const OUTPUT_HEADER = 'x-xcreenshot-output'

export type OutputRequest = { body: Uint8Array; headers: Record<string, string> }

/**
 * The request to copy or save `rect` (physical px) of `session`, with the annotations exported
 * for that rect, if any.
 */
export function outputRequest(
  session: number,
  rect: Rect,
  out: AnnotationExport | null | undefined,
): OutputRequest {
  const marked = out && (out.rgba || out.blurs.length > 0)
  const annotations = marked
    ? { x: out.x, y: out.y, width: out.width, height: out.height, blurs: out.blurs }
    : null
  const body = out?.rgba
    ? new Uint8Array(out.rgba.buffer, out.rgba.byteOffset, out.rgba.byteLength)
    : new Uint8Array(0)
  return { body, headers: { [OUTPUT_HEADER]: JSON.stringify({ session, rect, annotations }) } }
}
