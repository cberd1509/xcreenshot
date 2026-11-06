import { describe, expect, it } from 'vitest'
import { TEXT_SIZES, boxAt, ink, padding, radius, sizeOf, textColors } from './text'

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

  it('picks black or white text to read on a background', () => {
    expect(ink('#FFFFFF')).toBe('#000000')
    expect(ink('#FFCC00')).toBe('#000000')
    expect(ink('#000000')).toBe('#FFFFFF')
    expect(ink('#FF3B30')).toBe('#FFFFFF')
    expect(ink('#007AFF')).toBe('#FFFFFF')
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
