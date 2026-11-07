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
    ['navigator.platform on Linux', { platform: 'Linux x86_64' }, 'other'],
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
    ['an iPad, which reports a Mac', { platform: 'MacIntel', maxTouchPoints: 5 }, 'other'],
    [
      'an iPhone user agent',
      { userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)' },
      'other',
    ],
    ['a Mac trackpad', { platform: 'MacIntel', maxTouchPoints: 0 }, 'mac'],
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

const page = () => {
  document.body.innerHTML = `
    <a data-os="mac">Download for Mac</a>
    <a data-os="other" hidden>Get notified for Windows</a>
    <button data-os="mac" data-os-toggle>Not on a Mac?</button>
    <button data-os="other" data-os-toggle hidden>On a Mac?</button>
    <div role="tablist">
      <button role="tab" data-os-tab="mac" aria-selected="true">macOS</button>
      <button role="tab" data-os-tab="other" aria-selected="false" tabindex="-1">Windows</button>
      <button role="tab" data-os-tab="linux">Not a choice</button>
    </div>
    <p>Always shown</p>`
  return {
    mac: get('a[data-os="mac"]'),
    other: get('a[data-os="other"]'),
    notMac: get('button[data-os="mac"]'),
    onMac: get('button[data-os="other"]'),
    macTab: get('[data-os-tab="mac"]'),
    winTab: get('[data-os-tab="other"]'),
    oddTab: get('[data-os-tab="linux"]'),
    always: get('p'),
  }
}

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

  it('switches and remembers when a toggle is clicked', () => {
    const { mac, other, notMac, onMac } = page()
    setUpOs(document, { platform: 'MacIntel' }, local)

    notMac.click()
    expect(other.hidden).toBe(false)
    expect(mac.hidden).toBe(true)
    expect(readChoice(local)).toBe('other')
    expect(document.activeElement).toBe(onMac)

    onMac.click()
    expect(mac.hidden).toBe(false)
    expect(readChoice(local)).toBe('mac')
  })

  it('still switches when storage is blocked', () => {
    const { other, notMac } = page()
    setUpOs(document, {}, blocked)
    notMac.click()
    expect(other.hidden).toBe(false)
  })

  it('selects the tab for the shown version', () => {
    const { macTab, winTab } = page()
    setUpOs(document, { platform: 'Win32' }, local)
    expect(winTab.getAttribute('aria-selected')).toBe('true')
    expect(winTab.tabIndex).toBe(0)
    expect(macTab.getAttribute('aria-selected')).toBe('false')
    expect(macTab.tabIndex).toBe(-1)
  })

  it('switches the whole page when a tab is clicked', () => {
    const { mac, other, macTab, winTab } = page()
    setUpOs(document, { platform: 'MacIntel' }, local)
    winTab.click()
    expect(other.hidden).toBe(false)
    expect(readChoice(local)).toBe('other')
    macTab.click()
    expect(mac.hidden).toBe(false)
    expect(readChoice(local)).toBe('mac')
  })

  it('moves between tabs with the arrow keys, Home and End', () => {
    const { macTab, winTab, oddTab } = page()
    setUpOs(document, { platform: 'MacIntel' }, local)

    expect(press(macTab, 'ArrowRight').defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(winTab)
    expect(readChoice(local)).toBe('other')

    press(winTab, 'ArrowLeft')
    expect(document.activeElement).toBe(macTab)
    expect(readChoice(local)).toBe('mac')

    press(macTab, 'ArrowLeft')
    expect(document.activeElement).toBe(oddTab)
    winTab.click()
    press(winTab, 'Home')
    expect(document.activeElement).toBe(macTab)
    expect(readChoice(local)).toBe('mac')
    expect(macTab.getAttribute('aria-selected')).toBe('true')
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
})
