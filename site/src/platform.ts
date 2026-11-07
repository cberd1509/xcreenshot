// Which version of the page to show. The HTML ships the Mac version; other systems swap after load.

export type Os = 'mac' | 'other'

export interface NavigatorLike {
  userAgentData?: { platform?: string }
  platform?: string
  userAgent?: string
  maxTouchPoints?: number
}

const STORAGE_KEY = 'xcreenshot-os'

const isOs = (value: unknown): value is Os => value === 'mac' || value === 'other'

/**
 * Mac unless the browser says otherwise; Windows, Linux, iPhones and iPads all get the "other"
 * page. iPads report a Mac, so a touch screen rules it out.
 */
export function detectOs(nav: NavigatorLike | undefined): Os {
  const hint = [nav?.userAgentData?.platform, nav?.platform, nav?.userAgent].find(Boolean)
  if (!hint) return 'mac'
  const mac = /mac/i.test(hint) && !/iphone|ipad|ipod/i.test(hint)
  return mac && (nav?.maxTouchPoints ?? 0) <= 1 ? 'mac' : 'other'
}

/** The version the visitor picked, if storage allows it. */
export function readChoice(storage: () => Storage): Os | null {
  try {
    const value = storage().getItem(STORAGE_KEY)
    return isOs(value) ? value : null
  } catch {
    return null
  }
}

export function saveChoice(storage: () => Storage, os: Os): void {
  try {
    storage().setItem(STORAGE_KEY, os)
  } catch {
    // Private mode or blocked storage: the switch still works for this visit.
  }
}

/** Shows the `data-os` elements for this system, hides the rest and selects its `data-os-tab`. */
export function applyOs(root: ParentNode, os: Os): void {
  for (const el of root.querySelectorAll<HTMLElement>('[data-os]')) {
    el.hidden = el.dataset.os !== os
  }
  for (const tab of root.querySelectorAll<HTMLElement>('[data-os-tab]')) {
    const selected = tab.dataset.osTab === os
    tab.setAttribute('aria-selected', String(selected))
    tab.tabIndex = selected ? 0 : -1
  }
}

const TAB_KEYS: Record<string, (i: number, n: number) => number> = {
  ArrowRight: (i, n) => (i + 1) % n,
  ArrowLeft: (i, n) => (i - 1 + n) % n,
  Home: () => 0,
  End: (_, n) => n - 1,
}

/**
 * Shows the remembered or detected version and wires the `data-os-tab` tabs that pick one. Every
 * tablist on the page (the hero's and Install's) shows the same choice, and the arrow keys move
 * within the tablist they're pressed in. Every choice is remembered.
 */
export function setUpOs(root: ParentNode, nav: NavigatorLike, storage: () => Storage): Os {
  const os = readChoice(storage) ?? detectOs(nav)
  applyOs(root, os)

  for (const tab of root.querySelectorAll<HTMLElement>('[data-os-tab]')) {
    const value = tab.dataset.osTab
    if (!isOs(value)) continue
    tab.addEventListener('click', () => {
      saveChoice(storage, value)
      applyOs(root, value)
    })
    tab.addEventListener('keydown', (event) => {
      const move = TAB_KEYS[event.key]
      if (!move) return
      event.preventDefault()
      const list = tab.closest('[role="tablist"]') ?? root
      const tabs = [...list.querySelectorAll<HTMLElement>('[data-os-tab]')]
      const next = tabs[move(tabs.indexOf(tab), tabs.length)]
      next.focus()
      next.click()
    })
  }
  return os
}
