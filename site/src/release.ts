// Picks the version and download link the landing page shows, from the latest published release.
// Pure so it can be tested without the network; site/vite.config.ts does the file reading.

const REPO_URL = 'https://github.com/cberd1509/xcreenshot'
export const LATEST_RELEASE_URL = `${REPO_URL}/releases/latest`

export interface Download {
  version: string
  /** The universal DMG, or the releases page. */
  url: string
  /** The Windows installer, or null when the release has none (before the first Windows one). */
  windowsUrl: string | null
}

interface Asset {
  name: string
  browser_download_url: string
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null

const isAsset = (value: unknown): value is Asset =>
  isRecord(value) &&
  typeof value.name === 'string' &&
  typeof value.browser_download_url === 'string'

/**
 * Reads a GitHub "latest release" API response. Falls back to `fallbackVersion` and the releases
 * page when there is no release or it is a draft or prerelease, and to the releases page when it
 * has no universal DMG. `windowsUrl` is the 64-bit NSIS installer, or null without one.
 */
export function pickDownload(release: unknown, fallbackVersion: string): Download {
  const fallback = { version: fallbackVersion, url: LATEST_RELEASE_URL, windowsUrl: null }
  if (!isRecord(release) || typeof release.tag_name !== 'string') return fallback
  if (release.draft === true || release.prerelease === true) return fallback

  const version = release.tag_name.replace(/^v/, '')
  const assets = Array.isArray(release.assets) ? release.assets.filter(isAsset) : []
  const find = (suffix: string) =>
    assets.find((asset) => asset.name.endsWith(suffix))?.browser_download_url
  return {
    version,
    url: find('_universal.dmg') ?? LATEST_RELEASE_URL,
    windowsUrl: find('_x64-setup.exe') ?? null,
  }
}

/** Parses the saved API response, or returns null when there is none or it isn't JSON. */
export function parseRelease(text: string | undefined): unknown {
  if (!text) return null
  try {
    return JSON.parse(text)
  } catch {
    return null
  }
}

const ESCAPES: Record<string, string> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
}

/** Escapes text for HTML content and quoted attributes: a release tag is not markup. */
export const escapeHtml = (text: string) => text.replace(/[&<>"']/g, (c) => ESCAPES[c])

/** `<!-- name -->…<!-- /name -->`, the markers included. */
const block = (name: string) => new RegExp(`<!-- ${name} -->[\\s\\S]*?<!-- /${name} -->`, 'g')
const marker = /<!-- \/?(windows|no-windows) -->/g

/**
 * Keeps the page's `windows` blocks when there is a Windows installer and its `no-windows` blocks
 * when there isn't, so the page never offers a download that doesn't exist. Then replaces the
 * `%VERSION%`, `%DOWNLOAD_URL%` and `%WINDOWS_URL%` placeholders, escaped.
 */
export function fillPlaceholders(html: string, download: Download): string {
  const { windowsUrl } = download
  return html
    .replace(block(windowsUrl ? 'no-windows' : 'windows'), '')
    .replace(marker, '')
    .replaceAll('%VERSION%', escapeHtml(download.version))
    .replaceAll('%DOWNLOAD_URL%', escapeHtml(download.url))
    .replaceAll('%WINDOWS_URL%', escapeHtml(windowsUrl ?? LATEST_RELEASE_URL))
}
