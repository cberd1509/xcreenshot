import { describe, expect, it } from 'vitest'
import { COLORS } from './shapes'
import {
  FONT,
  TEXT_SIZES,
  boxAt,
  ink,
  luminance,
  padding,
  radius,
  sizeOf,
  textColors,
  textFont,
  within,
} from './text'

/** WCAG contrast ratio between two luminances. */
const ratio = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)

describe('text', () => {
  it('has small, medium and large sizes in logical px, the same on any display', () => {
    expect(TEXT_SIZES).toEqual({ S: 16, M: 24, L: 36 })
  })

  it('names a font size, or nothing for a resized one', () => {
    expect(sizeOf(16)).toBe('S')
    expect(sizeOf(36)).toBe('L')
    expect(sizeOf(30)).toBeNull()
  })

  it('pads and rounds the box in proportion to the font', () => {
    expect(padding(20)).toBe(6)
    expect(radius(20)).toBe(5)
  })

  it('measures WCAG relative luminance', () => {
    expect(luminance('#000000')).toBe(0)
    expect(luminance('#FFFFFF')).toBeCloseTo(1)
    // A dark channel uses the linear part of the curve.
    expect(luminance('#000A00')).toBeCloseTo((10 / 255 / 12.92) * 0.7152)
  })

  it('picks black or white text by WCAG contrast ratio, for every palette color', () => {
    for (const { color } of COLORS) {
      const l = luminance(color)
      const black = ratio(l, 0)
      const white = ratio(l, 1)
      // The pick has the higher ratio, and reads (at least 3:1, WCAG's large-text minimum).
      const picked = ink(color)
      expect(picked).toBe(black >= white ? '#000000' : '#FFFFFF')
      expect(Math.max(black, white)).toBeGreaterThanOrEqual(3)
    }
    expect(COLORS.map(({ color }) => ink(color))).toEqual([
      '#000000', // red
      '#000000', // orange
      '#000000', // yellow
      '#000000', // green: black reads far better than white
      '#000000', // blue
      '#000000', // purple
      '#000000', // white
      '#FFFFFF', // black
    ])
  })

  it('lays text out in the system UI font', () => {
    expect(textFont(20)).toEqual({
      fontSize: 20,
      fontFamily: FONT,
      lineHeight: 1.25,
      padding: 6,
    })
  })

  it('moves a box as little as needed to sit wholly in an area', () => {
    const area = { x: 100, y: 100, width: 400, height: 300 }
    expect(within({ x: 150, y: 150, width: 50, height: 50 }, area)).toEqual({ x: 150, y: 150 })
    expect(within({ x: 480, y: 390, width: 50, height: 50 }, area)).toEqual({ x: 450, y: 350 })
    expect(within({ x: 50, y: 60, width: 50, height: 50 }, area)).toEqual({ x: 100, y: 100 })
    // Too big: its top-left corner goes in.
    expect(within({ x: 300, y: 300, width: 900, height: 900 }, area)).toEqual({ x: 100, y: 100 })
  })

  it('colors the text, or the background with contrasting text', () => {
    expect(textColors('#FF3B30', false)).toEqual({ fill: '#FF3B30', box: 'transparent' })
    expect(textColors('#FFCC00', true)).toEqual({ fill: '#000000', box: '#FFCC00' })
  })

  it('puts the box so the caret is where the click was', () => {
    // 20 px font: 6 px padding, and a 25 px line centered on the click.
    expect(boxAt({ x: 100, y: 100 }, 20)).toEqual({ x: 94, y: 81.5 })
  })
})
