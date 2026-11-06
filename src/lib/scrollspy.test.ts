import { describe, expect, it } from 'vitest'
import { sectionInView, type View } from './scrollspy'

const sections = [
  { id: 'shortcuts', top: 28, bottom: 150 },
  { id: 'capture', top: 176, bottom: 330 },
  { id: 'general', top: 356, bottom: 510 },
  { id: 'about', top: 536, bottom: 640 },
]

/** A 300 px tall view scrolled to `top`. */
const view = (top: number, atBottom = false): View => ({
  top,
  bottom: top + 300,
  line: top + 40,
  atBottom,
})

describe('sectionInView', () => {
  it('is the first section at the top', () => {
    expect(sectionInView(sections, view(0))).toBe('shortcuts')
  })

  it('is the last section whose top passed the line', () => {
    expect(sectionInView(sections, view(140))).toBe('capture')
    expect(sectionInView(sections, view(330))).toBe('general')
  })

  it('is the last section at the bottom, even if it never reaches the line', () => {
    expect(sectionInView(sections, view(340, true))).toBe('about')
  })

  it('keeps the clicked section while it is visible', () => {
    expect(sectionInView(sections, view(340, true), 'general')).toBe('general')
    expect(sectionInView(sections, view(0), 'general')).toBe('shortcuts')
    expect(sectionInView(sections, view(0), 'nope')).toBe('shortcuts')
  })

  it('has nothing to mark without sections', () => {
    expect(sectionInView([], view(0))).toBeUndefined()
  })
})
