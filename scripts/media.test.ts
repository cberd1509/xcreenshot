// @vitest-environment node
import { describe, expect, it } from 'vitest'
import { concat, ease, gifArgs, mp4Args, path, size, steps, stillArgs } from './media.ts'

describe('ease', () => {
  it('starts and ends slowly, symmetric around the middle', () => {
    expect(ease(0)).toBe(0)
    expect(ease(0.5)).toBe(0.5)
    expect(ease(1)).toBe(1)
    expect(ease(0.1)).toBeLessThan(0.1)
    expect(ease(0.9)).toBeGreaterThan(0.9)
    expect(ease(0.25) + ease(0.75)).toBeCloseTo(1)
  })

  it('clamps outside [0, 1]', () => {
    expect(ease(-1)).toBe(0)
    expect(ease(2)).toBe(1)
  })
})

describe('path', () => {
  const from = { x: 0, y: 0 }
  const to = { x: 100, y: 0 }

  it('ends exactly at the target, without the start', () => {
    const points = path(from, to, 10)
    expect(points).toHaveLength(10)
    expect(points.at(-1)).toEqual(to)
    expect(points[0]).not.toEqual(from)
  })

  it('moves forward all the way, small steps at the ends', () => {
    const xs = path(from, to, 10).map((p) => p.x)
    for (let i = 1; i < xs.length; i++) expect(xs[i]).toBeGreaterThan(xs[i - 1])
    const first = xs[0]
    const middle = xs[5] - xs[4]
    expect(first).toBeLessThan(middle)
  })

  it('bends to the left of the way, or not at all', () => {
    // Left of going right, on screen (y down), is up.
    expect(path(from, to, 10)[4].y).toBeLessThan(0)
    expect(path(from, to, 10, 0).every((p) => p.y === 0)).toBe(true)
  })

  it('takes at least one step', () => {
    expect(path(from, to, 0)).toEqual([to])
  })
})

describe('steps', () => {
  it('takes longer for longer moves, within bounds', () => {
    const at = { x: 0, y: 0 }
    const short = steps(at, { x: 10, y: 0 }, 60)
    const long = steps(at, { x: 1000, y: 0 }, 60)
    expect(short).toBe(12)
    expect(long).toBeGreaterThan(short)
    expect(steps(at, { x: 5000, y: 0 }, 60)).toBe(42)
    expect(steps(at, at, 5)).toBe(2)
  })
})

describe('concat', () => {
  it('shows each frame until the next, the last until the end', () => {
    const list = concat(
      [
        { file: '/t/0.jpg', time: 10 },
        { file: '/t/1.jpg', time: 10.25 },
      ],
      11,
      0.01,
    )
    expect(list).toBe(
      [
        'ffconcat version 1.0',
        "file '/t/0.jpg'",
        'duration 0.250',
        "file '/t/1.jpg'",
        'duration 0.750',
        "file '/t/1.jpg'",
        '',
      ].join('\n'),
    )
  })

  it('shows every frame for at least the minimum, and lists nothing without frames', () => {
    expect(concat([{ file: 'a.jpg', time: 5 }], 4, 1 / 60)).toContain('duration 0.017')
    expect(concat([], 1, 0.01)).toBe('ffconcat version 1.0\n')
  })
})

describe('gifArgs', () => {
  it('reads the frame list and encodes a looping GIF with one diff palette', () => {
    const args = gifArgs({ input: 'frames.txt', output: 'out.gif', width: 1280, fps: 15 })
    expect(args.slice(args.indexOf('-f'), args.indexOf('-i') + 2)).toEqual([
      '-f',
      'concat',
      '-safe',
      '0',
      '-i',
      'frames.txt',
    ])
    const filter = args[args.indexOf('-filter_complex') + 1]
    expect(filter).toContain('fps=15,scale=1280:-1')
    expect(filter).toContain('mpdecimate')
    expect(filter).toContain('palettegen=stats_mode=diff')
    expect(filter).toContain('paletteuse=dither=sierra2_4a')
    expect(args.slice(-3)).toEqual(['-loop', '0', 'out.gif'])
    expect(args).toContain('vfr')
  })
})

describe('mp4Args', () => {
  it('encodes H.264 in yuv420p, without audio, ready to stream', () => {
    const args = mp4Args({ input: 'frames.txt', output: 'demo.mp4', width: 1440, fps: 30, crf: 24 })
    expect(args.slice(args.indexOf('-f'), args.indexOf('-i') + 2)).toEqual([
      '-f',
      'concat',
      '-safe',
      '0',
      '-i',
      'frames.txt',
    ])
    expect(args[args.indexOf('-vf') + 1]).toBe(
      'fps=30,scale=1440:-2:flags=lanczos:out_range=tv,format=yuv420p',
    )
    expect(args[args.indexOf('-c:v') + 1]).toBe('libx264')
    expect(args[args.indexOf('-crf') + 1]).toBe('24')
    expect(args[args.indexOf('-pix_fmt') + 1]).toBe('yuv420p')
    expect(args).toContain('-an')
    expect(args.slice(-3)).toEqual(['-movflags', '+faststart', 'demo.mp4'])
  })
})

describe('stillArgs', () => {
  it('saves one scaled frame at a time', () => {
    const args = stillArgs({ input: 'demo.mp4', output: 'poster.jpg', at: 0, width: 1440 })
    expect(args.slice(args.indexOf('-ss'), args.indexOf('-ss') + 4)).toEqual([
      '-ss',
      '0.000',
      '-i',
      'demo.mp4',
    ])
    expect(args[args.indexOf('-frames:v') + 1]).toBe('1')
    expect(args[args.indexOf('-vf') + 1]).toBe('scale=1440:-2:flags=lanczos')
    expect(args.at(-1)).toBe('poster.jpg')
  })

  it('crops to a height around the middle', () => {
    const args = stillArgs({ input: 'a.mp4', output: 'og.jpg', at: 5.25, width: 1200, height: 630 })
    expect(args).toContain('5.250')
    expect(args[args.indexOf('-vf') + 1]).toBe('scale=1200:-2:flags=lanczos,crop=1200:630')
  })
})

describe('size', () => {
  it('reads as B, KB or MB', () => {
    expect(size(500)).toBe('500 B')
    expect(size(2048)).toBe('2 KB')
    expect(size(3.5 * 1024 * 1024)).toBe('3.5 MB')
  })
})
