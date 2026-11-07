// Picks the version and download link the landing page shows, from the latest published release.
// Pure so it can be tested without the network; site/vite.config.ts does the file reading.

const REPO_URL = 'https://github.com/cberd1509/xcreenshot'
export const LATEST_RELEASE_URL = `${REPO_URL}/releases/latest`

export interface Download {
  version: string
  url: string
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
 * has no universal DMG.
 */
export function pickDownload(release: unknown, fallbackVersion: string): Download {
  const fallback = { version: fallbackVersion, url: LATEST_RELEASE_URL }
  if (!isRecord(release) || typeof release.tag_name !== 'string') return fallback
  if (release.draft === true || release.prerelease === true) return fallback

  const version = release.tag_name.replace(/^v/, '')
  const assets = Array.isArray(release.assets) ? release.assets.filter(isAsset) : []
  const dmg = assets.find((asset) => asset.name.endsWith('_universal.dmg'))
  return { version, url: dmg?.browser_download_url ?? LATEST_RELEASE_URL }
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

/** Replaces the page's `%VERSION%` and `%DOWNLOAD_URL%` placeholders, escaped. */
export function fillPlaceholders(html: string, download: Download): string {
  return html
    .replaceAll('%VERSION%', escapeHtml(download.version))
    .replaceAll('%DOWNLOAD_URL%', escapeHtml(download.url))
}
