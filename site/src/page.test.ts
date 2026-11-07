// The page's per-OS copy, checked against the Landing artboard.
import { beforeAll, describe, expect, it } from 'vitest'
import html from '../index.html?raw'

const keys = (el: Element | null) =>
  [...(el?.querySelectorAll('kbd') ?? [])].map((k) => k.textContent)
const text = (el: Element | null) => el?.textContent?.replace(/\s+/g, ' ').trim()

/** The "Press the shortcut" key caps for one OS. */
const shortcut = (os: string) => document.querySelector(`.step .keys[data-os="${os}"]`)

/** The text of step `n` in an Install panel. */
const step = (panel: string, n: number) =>
  document.querySelectorAll(`#${panel} li`)[n - 1]?.querySelector('span:last-child') ?? null

beforeAll(() => {
  document.documentElement.innerHTML = html
})

describe('How it works', () => {
  it('shows ⌃⇧⌘S on a Mac and a single PrtSc on Windows', () => {
    expect(keys(shortcut('mac'))).toEqual(['⌃', '⇧', '⌘', 'S'])
    expect(keys(shortcut('other'))).toEqual(['PrtSc'])
  })
})

describe('Install', () => {
  it('tells Windows users to press PrtSc, with Ctrl Shift PrtSc as the fallback', () => {
    const third = step('install-windows', 3)
    expect(text(third)).toBe(
      'Press PrtSc to take a screenshot, or Ctrl Shift PrtSc if Snipping Tool already uses PrtSc.',
    )
    expect(keys(third)).toEqual(['PrtSc', 'Ctrl', 'Shift', 'PrtSc'])
  })

  it('keeps the Mac shortcut on the Mac panel', () => {
    expect(keys(step('install-mac', 3))).toEqual(['⌃⇧⌘S'])
  })
})
