// Release helpers for .github/workflows/release.yml.
// Usage:
//   node scripts/release.ts notes <version> [--unsigned]
//     Prints the GitHub Release notes for a version, taken from CHANGELOG.md.
//   node scripts/release.ts updater-urls <latest.json> <assets.json> <owner/repo> <tag>
//     Prints latest.json with every download URL pointing at the published tag.
import { readFileSync } from 'node:fs'

export const UNSIGNED_WARNING =
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

export function releaseNotes(section: string, unsigned: boolean): string {
  const body = section || 'No changelog entry for this version.'
  return unsigned ? `${UNSIGNED_WARNING}\n\n${body}\n` : `${body}\n`
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
export function tagDownloadUrls(
  manifest: UpdaterManifest,
  assets: ReleaseAsset[],
  repo: string,
  tag: string,
): UpdaterManifest {
  const base = `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/`
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
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as UpdaterManifest
    const assets = JSON.parse(readFileSync(assetsPath, 'utf8')) as ReleaseAsset[]
    return `${JSON.stringify(tagDownloadUrls(manifest, assets, repo, tag), null, 2)}\n`
  }
  throw new Error(
    'usage: release.ts notes <version> [--unsigned] | ' +
      'updater-urls <latest.json> <assets.json> <owner/repo> <tag>',
  )
}

if (import.meta.main) process.stdout.write(main(process.argv.slice(2)))
