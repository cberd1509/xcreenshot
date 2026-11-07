// The page's per-OS copy, checked against the Landing artboard.
import { describe, expect, it } from 'vitest'
import html from '../index.html?raw'
import { fillPlaceholders, type Download } from './release'

const download = 'https://github.com/cberd1509/xcreenshot/releases/download/v0.4.0'
const dmgUrl = `${download}/xcreenshot_0.4.0_universal.dmg`
const exeUrl = `${download}/xcreenshot_0.4.0_x64-setup.exe`
const RELEASES = 'https://github.com/cberd1509/xcreenshot/releases'

const withWindows: Download = { version: '0.4.0', url: dmgUrl, windowsUrl: exeUrl }
const macOnly: Download = { version: '0.4.0', url: dmgUrl, windowsUrl: null }

const render = (release: Download) => {
  document.documentElement.innerHTML = fillPlaceholders(html, release)
}

const keys = (el: Element | null) =>
  [...(el?.querySelectorAll('kbd') ?? [])].map((k) => k.textContent)
const text = (el: Element | null) => el?.textContent?.replace(/\s+/g, ' ').trim()
const $ = (selector: string) => document.querySelector(selector)
const $$ = (selector: string) => [...document.querySelectorAll(selector)]
const hrefs = (selector: string) =>
  $$(`${selector} a`).map((a) => [text(a), a.getAttribute('href')])

/** The "Press the shortcut" key caps for one OS. */
const shortcut = (os: string) => $(`.step .keys[data-os="${os}"]`)

/** The text of step `n` in an Install panel. */
const step = (panel: string, n: number) =>
  $$(`#${panel} li`)[n - 1]?.querySelector('span:last-child') ?? null

describe.each([
  ['with a Windows release', withWindows],
  ['before the first Windows release', macOnly],
])('%s', (_, release) => {
  it('leaves no markers or placeholders', () => {
    render(release)
    const source = document.documentElement.innerHTML
    expect(source).not.toMatch(/%[A-Z_]+%|<!-- \/?(no-)?windows -->/)
  })

  it('shows ⌃⇧⌘S on a Mac and a single PrtSc on Windows', () => {
    render(release)
    expect(keys(shortcut('mac'))).toEqual(['⌃', '⇧', '⌘', 'S'])
    expect(keys(shortcut('other'))).toEqual(['PrtSc'])
  })

  it('tells Windows users to press PrtSc, with Ctrl Shift PrtSc as the fallback', () => {
    render(release)
    const third = step('install-windows', 3)
    expect(text(third)).toBe(
      'Press PrtSc to take a screenshot, or Ctrl Shift PrtSc if Snipping Tool already uses PrtSc.',
    )
    expect(keys(third)).toEqual(['PrtSc', 'Ctrl', 'Shift', 'PrtSc'])
    expect(keys(step('install-mac', 3))).toEqual(['⌃⇧⌘S'])
  })

  it('has a platform switch above the hero buttons and on Install', () => {
    render(release)
    const lists = $$('[role="tablist"]')
    expect(lists.map((list) => $$('[role="tab"]').filter((t) => list.contains(t)).length)).toEqual([
      2, 2,
    ])
    for (const tab of $$('[role="tab"]')) {
      expect($(`#${tab.getAttribute('aria-controls')}`)?.getAttribute('role')).toBe('tabpanel')
    }
    const hero = $('.hero')!
    expect(hero.querySelector('[role="tablist"]')?.nextElementSibling?.id).toBe('hero-mac')
  })

  it('keeps the Mac hero as it was', () => {
    render(release)
    expect(hrefs('#hero-mac')[0]).toEqual(['Download for Mac', dmgUrl])
    expect(text($('#hero-mac .meta'))).toBe(
      'Version 0.4.0 · macOS 14 or later · Apple Silicon and Intel · Free and open source',
    )
  })
})

describe('with a Windows release', () => {
  it('offers the installer in the hero', () => {
    render(withWindows)
    expect(hrefs('#hero-windows')).toEqual([
      ['Download for Windows', exeUrl],
      ['View source', 'https://github.com/cberd1509/xcreenshot'],
    ])
    expect(text($('#hero-windows .meta'))).toBe(
      'Version 0.4.0 · Windows 10 and 11, 64-bit · Free and open source',
    )
  })

  it('has the Windows install steps', () => {
    render(withWindows)
    expect(text($('.install-intro p[data-os="other"]'))).toBe(
      "It updates itself, and you can turn that off in Settings. It isn't code signed yet, so " +
        'Windows asks before the first run.',
    )
    expect(text(step('install-windows', 1))).toBe('Download the installer and run it.')
    expect(step('install-windows', 1)?.querySelector('a')?.getAttribute('href')).toBe(exeUrl)
    expect(text(step('install-windows', 2))).toBe(
      'If Windows says it protected your PC, click More info › Run anyway.',
    )
  })

  it('says both platforms in the footer', () => {
    render(withWindows)
    expect(text($('.footer'))).toContain('macOS and Windows.')
    expect(text($('.footer'))).not.toContain('in progress')
  })
})

describe('before the first Windows release', () => {
  it('offers no Windows download', () => {
    render(macOnly)
    expect(text(document.body)).not.toContain('Download for Windows')
    expect(document.body.innerHTML).not.toContain('.exe')
    expect(hrefs('#hero-windows')).toEqual([
      ['Get notified for Windows', RELEASES],
      ['Download for Mac', dmgUrl],
    ])
    expect(text($('#hero-windows .meta'))).toBe(
      'Mac only for now. The Windows version is in progress.',
    )
  })

  it('says it is in progress on Install and in the footer', () => {
    render(macOnly)
    expect(text($('.install-intro p[data-os="other"]'))).toBe('The Windows version is in progress.')
    expect(text(step('install-windows', 1))).toContain("It isn't out yet.")
    expect(text($('.footer'))).toContain('Windows version in progress.')
  })
})
