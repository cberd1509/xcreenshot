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
 * Shows the remembered or detected version, and wires the controls that switch it: the
 * `data-os-toggle` links flip it, and the `data-os-tab` tabs pick one (arrow keys move between
 * them). Every choice is remembered.
 */
export function setUpOs(root: ParentNode, nav: NavigatorLike, storage: () => Storage): Os {
  let os = readChoice(storage) ?? detectOs(nav)
  const choose = (next: Os) => {
    os = next
    saveChoice(storage, os)
    applyOs(root, os)
  }
  applyOs(root, os)

  const toggles = [...root.querySelectorAll<HTMLElement>('[data-os-toggle]')]
  for (const toggle of toggles) {
    toggle.addEventListener('click', () => {
      choose(os === 'mac' ? 'other' : 'mac')
      // The clicked link is now hidden: keep focus on the one that took its place.
      toggles.find((el) => !el.closest('[hidden]'))?.focus()
    })
  }

  const tabs = [...root.querySelectorAll<HTMLElement>('[data-os-tab]')]
  tabs.forEach((tab, i) => {
    const value = tab.dataset.osTab
    if (!isOs(value)) return
    tab.addEventListener('click', () => choose(value))
    tab.addEventListener('keydown', (event) => {
      const move = TAB_KEYS[event.key]
      if (!move) return
      event.preventDefault()
      const next = tabs[move(i, tabs.length)]
      next.focus()
      next.click()
    })
  })
  return os
}
