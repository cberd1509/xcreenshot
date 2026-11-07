import { beforeEach, describe, expect, it } from 'vitest'
import { applyOs, detectOs, readChoice, saveChoice, setUpOs } from './platform'

const blocked = (): Storage => {
  throw new Error('SecurityError')
}
const local = () => localStorage

beforeEach(() => localStorage.clear())

describe('detectOs', () => {
  it.each([
    ['client hints on a Mac', { userAgentData: { platform: 'macOS' } }, 'mac'],
    ['client hints on Windows', { userAgentData: { platform: 'Windows' } }, 'other'],
    ['navigator.platform on a Mac', { platform: 'MacIntel' }, 'mac'],
    ['navigator.platform on Windows', { platform: 'Win32' }, 'other'],
    ['navigator.platform on Linux', { platform: 'Linux x86_64' }, 'mac'],
    ['client hints on Linux', { userAgentData: { platform: 'Linux' } }, 'mac'],
    ['client hints on Android', { userAgentData: { platform: 'Android' } }, 'mac'],
    [
      'the user agent alone',
      { platform: '', userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' },
      'other',
    ],
    [
      'a Mac user agent',
      { userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 15_0) AppleWebKit/605.1.15' },
      'mac',
    ],
    ['empty client hints', { userAgentData: { platform: '' }, platform: 'Win32' }, 'other'],
    [
      'an iPhone user agent',
      { userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)' },
      'mac',
    ],
    ['a user agent that mentions Darwin', { userAgent: 'xcreenshot/1.0 Darwin/24.0.0' }, 'mac'],
  ])('reads %s', (_, nav, os) => {
    expect(detectOs(nav)).toBe(os)
  })

  it('defaults to Mac when nothing is known', () => {
    expect(detectOs(undefined)).toBe('mac')
    expect(detectOs({})).toBe('mac')
  })
})

describe('readChoice and saveChoice', () => {
  it('round-trips the choice', () => {
    expect(readChoice(local)).toBeNull()
    saveChoice(local, 'other')
    expect(readChoice(local)).toBe('other')
  })

  it('ignores unknown values', () => {
    localStorage.setItem('xcreenshot-os', 'linux')
    expect(readChoice(local)).toBeNull()
  })

  it('survives blocked storage', () => {
    expect(readChoice(blocked)).toBeNull()
    expect(() => saveChoice(blocked, 'mac')).not.toThrow()
  })
})

const press = (el: HTMLElement, key: string) => {
  const event = new KeyboardEvent('keydown', { key, cancelable: true })
  el.dispatchEvent(event)
  return event
}

const get = (selector: string) => document.querySelector<HTMLElement>(selector)!

/** Two tablists, like the hero and Install sections, plus per-OS content. */
const page = () => {
  document.body.innerHTML = `
    <div role="tablist" id="hero">
      <button role="tab" data-os-tab="mac" aria-selected="true">macOS</button>
      <button role="tab" data-os-tab="other" aria-selected="false" tabindex="-1">Windows</button>
    </div>
    <a data-os="mac">Download for Mac</a>
    <a data-os="other" hidden>Download for Windows</a>
    <div role="tablist" id="install">
      <button role="tab" data-os-tab="mac" aria-selected="true">macOS</button>
      <button role="tab" data-os-tab="other" aria-selected="false" tabindex="-1">Windows</button>
      <button role="tab" data-os-tab="linux">Not a choice</button>
    </div>
    <p>Always shown</p>`
  return {
    mac: get('a[data-os="mac"]'),
    other: get('a[data-os="other"]'),
    heroMac: get('#hero [data-os-tab="mac"]'),
    heroWin: get('#hero [data-os-tab="other"]'),
    macTab: get('#install [data-os-tab="mac"]'),
    winTab: get('#install [data-os-tab="other"]'),
    oddTab: get('[data-os-tab="linux"]'),
    always: get('p'),
  }
}

const selected = (tab: HTMLElement) => tab.getAttribute('aria-selected')

describe('applyOs', () => {
  it('swaps to the other version', () => {
    const { mac, other, always } = page()
    applyOs(document, 'other')
    expect(mac.hidden).toBe(true)
    expect(other.hidden).toBe(false)
    expect(always.hidden).toBe(false)
  })

  it('keeps the Mac version on a Mac', () => {
    const { mac, other } = page()
    applyOs(document, 'mac')
    expect(mac.hidden).toBe(false)
    expect(other.hidden).toBe(true)
  })
})

describe('setUpOs', () => {
  it('shows the detected version', () => {
    const { other } = page()
    expect(setUpOs(document, { platform: 'Win32' }, local)).toBe('other')
    expect(other.hidden).toBe(false)
  })

  it('prefers the saved choice over detection', () => {
    const { mac } = page()
    saveChoice(local, 'mac')
    expect(setUpOs(document, { platform: 'Win32' }, local)).toBe('mac')
    expect(mac.hidden).toBe(false)
  })

  it('preselects the detected platform in every tablist', () => {
    const { heroMac, heroWin, macTab, winTab } = page()
    setUpOs(document, { platform: 'Win32' }, local)
    for (const tab of [heroWin, winTab]) {
      expect(selected(tab)).toBe('true')
      expect(tab.tabIndex).toBe(0)
    }
    for (const tab of [heroMac, macTab]) {
      expect(selected(tab)).toBe('false')
      expect(tab.tabIndex).toBe(-1)
    }
  })

  it('switches the whole page and the other tablist when a tab is clicked', () => {
    const { mac, other, heroMac, heroWin, macTab, winTab } = page()
    setUpOs(document, { platform: 'MacIntel' }, local)

    heroWin.click()
    expect(other.hidden).toBe(false)
    expect(selected(winTab)).toBe('true')
    expect(selected(macTab)).toBe('false')
    expect(readChoice(local)).toBe('other')

    macTab.click()
    expect(mac.hidden).toBe(false)
    expect(selected(heroMac)).toBe('true')
    expect(selected(heroWin)).toBe('false')
    expect(readChoice(local)).toBe('mac')
  })

  it('still switches when storage is blocked', () => {
    const { other, heroWin } = page()
    setUpOs(document, {}, blocked)
    heroWin.click()
    expect(other.hidden).toBe(false)
  })

  it('moves within a tablist with the arrow keys, Home and End', () => {
    const { heroMac, heroWin, macTab, winTab, oddTab } = page()
    setUpOs(document, { platform: 'MacIntel' }, local)

    expect(press(heroMac, 'ArrowRight').defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(heroWin)
    expect(readChoice(local)).toBe('other')
    expect(selected(winTab)).toBe('true')

    // The hero has two tabs: it wraps around without reaching Install's.
    press(heroWin, 'ArrowRight')
    expect(document.activeElement).toBe(heroMac)
    expect(selected(macTab)).toBe('true')

    press(macTab, 'ArrowLeft')
    expect(document.activeElement).toBe(oddTab)
    winTab.click()
    press(winTab, 'Home')
    expect(document.activeElement).toBe(macTab)
    expect(readChoice(local)).toBe('mac')
    press(macTab, 'End')
    expect(document.activeElement).toBe(oddTab)

    expect(press(macTab, 'a').defaultPrevented).toBe(false)
  })

  it('ignores a tab with an unknown value', () => {
    const { oddTab, mac, other } = page()
    setUpOs(document, { platform: 'MacIntel' }, local)
    oddTab.click()
    expect(readChoice(local)).toBeNull()
    expect(localStorage.length).toBe(0)
    expect(mac.hidden).toBe(false)
    expect(other.hidden).toBe(true)
  })

  it('moves among tabs outside a tablist too', () => {
    document.body.innerHTML = `
      <button data-os-tab="mac">macOS</button>
      <button data-os-tab="other">Windows</button>`
    setUpOs(document, { platform: 'MacIntel' }, local)
    press(get('[data-os-tab="mac"]'), 'End')
    expect(document.activeElement).toBe(get('[data-os-tab="other"]'))
  })
})
