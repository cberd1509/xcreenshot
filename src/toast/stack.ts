// The confirmation toasts' stack: what each says, the count on a repeat, when each fades, how
// the older ones look and which take clicks. Pure, so the timing is tested without a clock.

/** A toast as Rust sends it with `toast:show` (see `toast/mod.rs`). */
export type ToastEvent = { seq: number } & (
  | { kind: 'copied'; width: number; height: number }
  | { kind: 'saved'; file: string }
  | { kind: 'shared'; service: string | null }
)

export type Kind = ToastEvent['kind']

export type Toast = {
  /** Keys the toast on the page; a repeat keeps it. */
  id: number
  /** The number of the newest action it shows. */
  seq: number
  kind: Kind
  detail: string
  /** How many times the action ran while this toast was up. */
  count: number
  /** When it starts to fade, in ms. */
  fades: number
}

/** How long a toast stays before it fades, in ms. */
export const SHOWN = 1200
/** How long a Saved toast stays, so its buttons can be reached, in ms. */
export const SAVED_SHOWN = 4000
/**
 * How long a toast under the mouse stays at most, in ms: the mouse leaving brings back its
 * usual time, and a leave the page never hears about can't keep it (and its clicks) up for good.
 */
export const HELD = 60_000
/** How long the fade takes, in ms. */
export const FADE = 200
/** The most toasts on screen at once. */
export const MOST = 3

/** What each toast says first. */
export const TITLES: Record<Kind, string> = { copied: 'Copied', saved: 'Saved', shared: 'Shared' }

/** What the Saved toast's second button says. */
export const reveal = (mac: boolean) => (mac ? 'Show in Finder' : 'Show in folder')

/** How long a `kind` toast stays before it fades, in ms. */
export const shown = (kind: Kind) => (kind === 'saved' ? SAVED_SHOWN : SHOWN)

/** The toast's detail: the size, the file name or the service. */
export function detail(event: ToastEvent): string {
  switch (event.kind) {
    case 'copied':
      return `${event.width} × ${event.height}`
    case 'saved':
      return event.file
    case 'shared':
      return event.service ?? ''
  }
}

/** Whether `toast` is still up (not fading) at `now`. */
export const up = (toast: Toast, now: number) => now < toast.fades

/**
 * Adds a toast in the order of the actions (Rust's numbers): at the bottom, unless it arrived
 * late. The same action while its toast is still up counts on that toast instead, which moves
 * with the new detail; an older one only counts. The oldest go past `MOST`.
 */
export function push(stack: Toast[], event: ToastEvent, now: number): Toast[] {
  const same = stack.find((t) => t.kind === event.kind && up(t, now))
  if (same && event.seq < same.seq)
    return stack.map((t) => (t === same ? { ...t, count: t.count + 1 } : t))
  const toast: Toast = {
    id: same?.id ?? event.seq,
    seq: event.seq,
    kind: event.kind,
    detail: detail(event),
    count: (same?.count ?? 0) + 1,
    // Not sooner than it would have, if the mouse is on it.
    fades: Math.max(now + shown(event.kind), same?.fades ?? 0),
  }
  const rest = stack.filter((t) => t !== same)
  const after = rest.findIndex((t) => t.seq > event.seq)
  rest.splice(after === -1 ? rest.length : after, 0, toast)
  return rest.slice(-MOST)
}

/** The stack without the toasts done fading at `now`. */
export const prune = (stack: Toast[], now: number) => stack.filter((t) => now < t.fades + FADE)

/** When the stack next changes (a toast starts or ends its fade), or `null` if it is empty. */
export function next(stack: Toast[], now: number): number | null {
  const times = stack.flatMap((t) => [t.fades, t.fades + FADE]).filter((t) => t > now)
  return times.length ? Math.min(...times) : null
}

/** Changes a stack at `now` for the toast keyed `id`. */
export type Change = (stack: Toast[], id: number, now: number) => Toast[]

/** Sets when the toast keyed `id` fades, if it is still up at `now`. */
const refade = (stack: Toast[], id: number, now: number, fades: (t: Toast) => number) =>
  stack.map((t) => (t.id === id && up(t, now) ? { ...t, fades: fades(t) } : t))

/** The mouse is on toast `id`: it stays up until the mouse leaves (`HELD` at most). */
export const hover: Change = (stack, id, now) => refade(stack, id, now, () => now + HELD)

/** The mouse left toast `id`: it stays its full time again from `now`. */
export const leave: Change = (stack, id, now) => refade(stack, id, now, (t) => now + shown(t.kind))

/** Toast `id` was acted on: it fades now. */
export const dismiss: Change = (stack, id, now) => refade(stack, id, now, () => now)

/** Whether the window takes clicks at `now`: only while a Saved toast is up. */
export const clickable = (stack: Toast[], now: number) =>
  stack.some((t) => t.kind === 'saved' && up(t, now))

/** How a toast `depth` places above the newest looks: smaller and dimmer, as designed. */
export function look(depth: number): { scale: number; opacity: number } {
  return [
    { scale: 1, opacity: 1 },
    { scale: 0.97, opacity: 0.8 },
    { scale: 0.94, opacity: 0.55 },
  ][Math.min(depth, MOST - 1)]
}
