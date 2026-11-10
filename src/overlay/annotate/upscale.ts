// Copy and Save at a larger size, pure: the factors under the Copy and Save carets, the size each
// gives and whether it is allowed. Mirrors `output/upscale.rs`; their tests pin the same values.

import type { Size } from '../geometry'

/** The factors on offer, besides 1×. */
export const FACTORS = [2, 3, 4] as const

export type Factor = 1 | (typeof FACTORS)[number]

/** The largest output, in px per side. Must match `MAX_SIDE` in `output/upscale.rs`. */
export const MAX_SIDE = 8192

/** One item of the menu: the factor, the size it gives, and whether that is within the cap. */
export type SizeOption = { factor: Factor; width: number; height: number; enabled: boolean }

/** The larger sizes of a `size` px selection, each disabled past `MAX_SIDE` on either side. */
export function sizeOptions(size: Size): SizeOption[] {
  return FACTORS.map((factor) => {
    const width = size.width * factor
    const height = size.height * factor
    return { factor, width, height, enabled: width <= MAX_SIDE && height <= MAX_SIDE }
  })
}
