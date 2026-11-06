// Text annotations, pure. Sizes are in logical px like every shape, so S, M and L look the same
// on 1x and 2x displays and are exported at the display's own resolution.

import { clamp, type Point, type Rect } from '../geometry'

export type TextSize = 'S' | 'M' | 'L'

/** Font size for each option, in logical px. */
export const TEXT_SIZES: Record<TextSize, number> = { S: 16, M: 24, L: 36 }

/** The text tool's options: the font size and a solid box behind the text. */
export type TextOptions = { size: TextSize; background: boolean }

export const DEFAULT_TEXT: TextOptions = { size: 'M', background: false }

/** The options as the toolbar shows them: no size for text resized off S, M and L. */
export type ShownTextOptions = { size: TextSize | null; background: boolean }

/** The system UI font, the same in the editor and on the board. */
export const FONT = 'system-ui, -apple-system, "Segoe UI", sans-serif'
export const LINE_HEIGHT = 1.25

/** The most characters a text keeps; a longer paste is cut, so layout stays fast. */
export const MAX_TEXT = 2000

/** Font sizes a resize stays within. */
export const MIN_FONT = 8
export const MAX_FONT = 400

/** Space between the text and the edge of its box (and its background). */
export const padding = (fontSize: number) => Math.round(fontSize * 0.3)

/** How text is laid out at `fontSize`: the same in the editor, the measure and on the board. */
export const textFont = (fontSize: number) => ({
  fontSize,
  fontFamily: FONT,
  lineHeight: LINE_HEIGHT,
  padding: padding(fontSize),
})

/** The background's corner radius. */
export const radius = (fontSize: number) => Math.round(fontSize * 0.25)

/** The option for a font size, or `null` if it was resized to another size. */
export function sizeOf(fontSize: number): TextSize | null {
  const found = Object.entries(TEXT_SIZES).find(([, px]) => px === fontSize)
  return found ? (found[0] as TextSize) : null
}

/** WCAG relative luminance of a `#rrggbb` color, 0 (black) to 1 (white). */
export function luminance(color: string): number {
  const n = parseInt(color.slice(1, 7), 16)
  const [r, g, b] = [n >> 16, (n >> 8) & 255, n & 255].map((v) => {
    const c = v / 255
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** Black or white, whichever has the higher WCAG contrast ratio on `color`. */
export function ink(color: string): string {
  const l = luminance(color)
  // Contrast with black is (l + 0.05) / 0.05, with white 1.05 / (l + 0.05).
  return (l + 0.05) / 0.05 >= 1.05 / (l + 0.05) ? '#000000' : '#FFFFFF'
}

/** The text's colors: on a background, the background takes the color and the text contrasts. */
export function textColors(color: string, background: boolean): { fill: string; box: string } {
  return background ? { fill: ink(color), box: color } : { fill: color, box: 'transparent' }
}

/** Where `box` goes to sit wholly in `area`, moved as little as possible; top-left if too big. */
export function within(box: Rect, area: Rect): Point {
  const x = clamp(box.x, area.x, area.x + area.width - box.width)
  const y = clamp(box.y, area.y, area.y + area.height - box.height)
  return { x: Math.max(x, area.x), y: Math.max(y, area.y) }
}

/** Where a text box goes so that its first line's caret sits at `p`. */
export function boxAt(p: Point, fontSize: number): Point {
  const pad = padding(fontSize)
  return { x: p.x - pad, y: p.y - (fontSize * LINE_HEIGHT) / 2 - pad }
}
