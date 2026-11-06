// @vitest-environment node
import { describe, expect, it } from 'vitest'
import { ease, gifArgs, path, size, steps } from './media.ts'

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

describe('gifArgs', () => {
  it('trims, scales and encodes a looping GIF with one diff palette', () => {
    const args = gifArgs({
      input: 'in.webm',
      output: 'out.gif',
      start: 0.4,
      duration: 8.123,
      width: 1280,
      fps: 15,
    })
    expect(args.slice(args.indexOf('-ss'), args.indexOf('-i') + 2)).toEqual([
      '-ss',
      '0.40',
      '-t',
      '8.12',
      '-i',
      'in.webm',
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

describe('size', () => {
  it('reads as B, KB or MB', () => {
    expect(size(500)).toBe('500 B')
    expect(size(2048)).toBe('2 KB')
    expect(size(3.5 * 1024 * 1024)).toBe('3.5 MB')
  })
})
