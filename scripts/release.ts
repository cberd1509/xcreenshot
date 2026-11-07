// Release helpers for .github/workflows/release.yml.
// Usage:
//   node scripts/release.ts notes <version> [--unsigned]
//     Prints the GitHub Release notes for a version, taken from CHANGELOG.md.
//   node scripts/release.ts updater-urls <latest.json> <assets.json> <owner/repo> <tag>
//     Prints latest.json with every download URL pointing at the published tag.
//   node scripts/release.ts add-windows <latest.json> <installer.exe.sig> <owner/repo> <tag>
//     Prints latest.json with the Windows installer added next to the macOS entries.
import { readFileSync } from 'node:fs'
import { basename } from 'node:path'

/** Hidden in the notes of unsigned builds; release.yml's updater-urls job looks for it. */
export const UNSIGNED_MARKER = '<!-- xcreenshot:unsigned -->'

export const UNSIGNED_WARNING =
  `${UNSIGNED_MARKER}\n` +
  '> [!WARNING]\n' +
  '> Unsigned test build: on first open use System Settings › Privacy & Security › Open Anyway.\n' +
  '> The in-app updater does not offer this build.'

/** The body of the `## <version>` section of a Changesets CHANGELOG.md, or '' if missing. */
export function changelogSection(changelog: string, version: string): string {
  const lines = changelog.split('\n')
  const start = lines.findIndex((line) => line.trim() === `## ${version}`)
  if (start === -1) return ''
  const rest = lines.slice(start + 1)
  const end = rest.findIndex((line) => /^##? /.test(line))
  return (end === -1 ? rest : rest.slice(0, end)).join('\n').trim()
}

export const WINDOWS_NOTE =
  '> [!NOTE]\n' +
  "> The Windows installer isn't code signed yet. If Windows says it protected your PC, click\n" +
  '> More info › Run anyway.'

export function releaseNotes(section: string, unsigned: boolean): string {
  const body = section || 'No changelog entry for this version.'
  return `${[...(unsigned ? [UNSIGNED_WARNING] : []), WINDOWS_NOTE, body].join('\n\n')}\n`
}

export interface UpdaterManifest {
  platforms: Record<string, { url: string; signature: string }>
  [key: string]: unknown
}

export interface ReleaseAsset {
  id: number
  name: string
}

/**
 * tauri-action writes API asset URLs (`<api>/repos/<owner>/<repo>/releases/assets/<id>`) into
 * latest.json. The app only downloads from `https://github.com/<owner>/<repo>/releases/download/`,
 * so point every platform at the published tag's download URL for the same asset.
 */
const downloadBase = (repo: string, tag: string) =>
  `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/`

export function tagDownloadUrls(
  manifest: UpdaterManifest,
  assets: ReleaseAsset[],
  repo: string,
  tag: string,
): UpdaterManifest {
  const base = downloadBase(repo, tag)
  const names = new Map(assets.map((asset) => [asset.id, asset.name]))
  const platforms = Object.fromEntries(
    Object.entries(manifest.platforms).map(([platform, entry]) => {
      if (entry.url.startsWith(base)) return [platform, entry]
      const id = /\/repos\/[^/]+\/[^/]+\/releases\/assets\/(\d+)$/.exec(entry.url)?.[1]
      const name = id === undefined ? undefined : names.get(Number(id))
      if (name === undefined) throw new Error(`${platform}: no release asset for ${entry.url}`)
      return [platform, { ...entry, url: base + encodeURIComponent(name) }]
    }),
  )
  return { ...manifest, platforms }
}

/** The updater looks for `windows-x86_64-nsis` first, then `windows-x86_64`, like tauri-action. */
const WINDOWS_PLATFORMS = ['windows-x86_64', 'windows-x86_64-nsis']

/**
 * Adds the Windows NSIS installer to the macOS latest.json, pointing at the tag download URL of
 * `asset` (the updater installs the setup .exe itself). Other platforms are kept as they are.
 */
export function addWindows(
  manifest: UpdaterManifest,
  signature: string,
  asset: string,
  repo: string,
  tag: string,
): UpdaterManifest {
  if (!asset.endsWith('-setup.exe')) throw new Error(`${asset}: not an NSIS installer`)
  if (!signature.trim()) throw new Error(`${asset}: empty signature`)
  const entry = {
    signature: signature.trim(),
    url: downloadBase(repo, tag) + encodeURIComponent(asset),
  }
  const windows = Object.fromEntries(WINDOWS_PLATFORMS.map((platform) => [platform, entry]))
  return { ...manifest, platforms: { ...manifest.platforms, ...windows } }
}

const readJson = <T>(path: string) => JSON.parse(readFileSync(path, 'utf8')) as T
const printJson = (value: unknown) => `${JSON.stringify(value, null, 2)}\n`

function readChangelog(path: string): string {
  try {
    return readFileSync(path, 'utf8')
  } catch {
    return '' // No release has written a changelog yet.
  }
}

export function main(args: string[], changelogPath = 'CHANGELOG.md'): string {
  const [command, ...rest] = args
  if (command === 'notes' && rest[0]) {
    const [version, flag] = rest
    return releaseNotes(
      changelogSection(readChangelog(changelogPath), version),
      flag === '--unsigned',
    )
  }
  if (command === 'updater-urls' && rest.length === 4) {
    const [manifestPath, assetsPath, repo, tag] = rest
    const manifest = readJson<UpdaterManifest>(manifestPath)
    return printJson(tagDownloadUrls(manifest, readJson<ReleaseAsset[]>(assetsPath), repo, tag))
  }
  if (command === 'add-windows' && rest.length === 4) {
    const [manifestPath, sigPath, repo, tag] = rest
    const manifest = readJson<UpdaterManifest>(manifestPath)
    const asset = basename(sigPath).replace(/\.sig$/, '')
    return printJson(addWindows(manifest, readFileSync(sigPath, 'utf8'), asset, repo, tag))
  }
  throw new Error(
    'usage: release.ts notes <version> [--unsigned] | ' +
      'updater-urls <latest.json> <assets.json> <owner/repo> <tag> | ' +
      'add-windows <latest.json> <installer.exe.sig> <owner/repo> <tag>',
  )
}

if (import.meta.main) process.stdout.write(main(process.argv.slice(2)))
