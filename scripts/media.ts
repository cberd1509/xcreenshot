// Pure helpers for scripts/readme-media.ts: human-paced pointer paths and the GIF encoding.

import type { Point } from '../src/overlay/geometry.ts'

/** Slow start, fast middle, slow end: t in [0, 1] to [0, 1]. */
export function ease(t: number): number {
  const c = Math.min(1, Math.max(0, t))
  return c < 0.5 ? 4 * c ** 3 : 1 - (-2 * c + 2) ** 3 / 2
}

/**
 * The `count` points a hand moves the pointer through from `from` to `to`, `from` excluded: eased,
 * along a slight arc (`bend`, as a share of the distance, to the left of the way).
 */
export function path(from: Point, to: Point, count: number, bend = 0.08): Point[] {
  const dx = to.x - from.x
  const dy = to.y - from.y
  // The arc's control point, off the middle of the straight line.
  const cx = from.x + dx / 2 + dy * bend
  const cy = from.y + dy / 2 - dx * bend
  const n = Math.max(1, Math.round(count))
  return Array.from({ length: n }, (_, i) => {
    const t = ease((i + 1) / n)
    const u = 1 - t
    return {
      x: u * u * from.x + 2 * u * t * cx + t * t * to.x,
      y: u * u * from.y + 2 * u * t * cy + t * t * to.y,
    }
  })
}

/** How many pointer steps a move of `from` to `to` takes, at `fps`, for a human pace. */
export function steps(from: Point, to: Point, fps: number): number {
  const distance = Math.hypot(to.x - from.x, to.y - from.y)
  // A fifth of a second for a short move, up to 0.7 s for one across the screen.
  const seconds = Math.min(0.7, 0.2 + distance / 2000)
  return Math.max(2, Math.round(seconds * fps))
}

/** A captured frame: its file, and when it showed (seconds). */
export type Shot = { file: string; time: number }

/**
 * An ffconcat list that shows each frame until the next one came, and the last until `end`, each
 * for at least `minimum` seconds. The browser sends a frame only when the page changes, so still
 * moments cost one frame.
 */
export function concat(shots: Shot[], end: number, minimum: number): string {
  const lines = ['ffconcat version 1.0']
  shots.forEach(({ file, time }, i) => {
    const next = shots[i + 1]?.time ?? end
    lines.push(`file '${file}'`, `duration ${Math.max(minimum, next - time).toFixed(3)}`)
  })
  // The concat demuxer only honors the last duration when the file is listed again.
  if (shots.length) lines.push(`file '${shots.at(-1)!.file}'`)
  return `${lines.join('\n')}\n`
}

export type Gif = {
  /** An ffconcat list (see `concat`). */
  input: string
  output: string
  width: number
  fps: number
}

/**
 * ffmpeg arguments that turn captured frames into a looping GIF: one palette for the whole clip,
 * built from what changes between frames, and only the changed part of each frame stored.
 * Repeated frames are dropped, and the kept one is shown for longer.
 */
export function gifArgs({ input, output, width, fps }: Gif): string[] {
  const filter =
    `fps=${fps},scale=${width}:-1:flags=lanczos,mpdecimate,split[a][b];` +
    `[a]palettegen=stats_mode=diff[p];` +
    `[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle`
  return [
    '-y',
    '-loglevel',
    'error',
    '-f',
    'concat',
    '-safe',
    '0',
    '-i',
    input,
    '-filter_complex',
    filter,
    '-fps_mode',
    'vfr',
    '-loop',
    '0',
    output,
  ]
}

/** A byte count for the log, e.g. "1.2 MB". */
export function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}
