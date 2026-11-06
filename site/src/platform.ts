// Which version of the page to show. The HTML ships the Mac version; other systems swap after load.

export type Os = 'mac' | 'other'

export interface NavigatorLike {
  userAgentData?: { platform?: string }
  platform?: string
  userAgent?: string
}

const STORAGE_KEY = 'xcreenshot-os'

/** Mac unless the browser says otherwise; Windows, Linux and phones all get the "other" page. */
export function detectOs(nav: NavigatorLike | undefined): Os {
  const hint = [nav?.userAgentData?.platform, nav?.platform, nav?.userAgent].find(Boolean)
  if (!hint) return 'mac'
  return /mac/i.test(hint) ? 'mac' : 'other'
}

/** The version the visitor picked with "Not on a Mac?" / "On a Mac?", if storage allows it. */
export function readChoice(storage: () => Storage): Os | null {
  try {
    const value = storage().getItem(STORAGE_KEY)
    return value === 'mac' || value === 'other' ? value : null
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

/** Shows the elements marked `data-os` for this system and hides the rest. */
export function applyOs(root: ParentNode, os: Os): void {
  for (const el of root.querySelectorAll<HTMLElement>('[data-os]')) {
    el.hidden = el.dataset.os !== os
  }
}

/**
 * Shows the remembered or detected version and wires the `data-os-toggle` buttons to switch it.
 */
export function setUpOs(root: ParentNode, nav: NavigatorLike, storage: () => Storage): Os {
  let os = readChoice(storage) ?? detectOs(nav)
  applyOs(root, os)
  for (const button of root.querySelectorAll('[data-os-toggle]')) {
    button.addEventListener('click', () => {
      os = os === 'mac' ? 'other' : 'mac'
      saveChoice(storage, os)
      applyOs(root, os)
    })
  }
  return os
}
