// The overlay's shared looks, as designed. Classes, so Tailwind sees them here.

/** Bars and menus: zinc-900 on a zinc-800 border. */
export const PANEL =
  'rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-50 shadow-[0_12px_32px_rgb(0_0_0/0.35)] ring-0'

/** A 36 px icon button: grey, white on hover, and zinc-700 while on. */
export const BUTTON =
  'flex size-9 items-center justify-center rounded-lg text-zinc-400 hover:bg-zinc-800 hover:text-zinc-50 disabled:pointer-events-none disabled:text-zinc-600 data-[state=on]:bg-zinc-700 data-[state=on]:text-zinc-50 [&_svg]:size-[18px]'

/** A key cap, in a tooltip or on Copy. */
export const KEY = 'rounded bg-zinc-200 px-[5px] font-sans text-[11px] font-normal text-zinc-600'

/** A line between groups of the rail. */
export const DIVIDER = 'mx-1 my-1 h-px bg-zinc-800'

/** A short message, centred above the key hint. */
export const TOAST = `pointer-events-none fixed bottom-[72px] left-1/2 z-1 -translate-x-1/2 px-3.5 py-2 text-xs whitespace-nowrap ${PANEL}`
