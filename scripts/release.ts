// Release helpers for .github/workflows/release.yml.
// Usage:
//   node scripts/release.ts notes <version> [--unsigned]
//     Prints the GitHub Release notes for a version, taken from CHANGELOG.md.
//   node scripts/release.ts updater-urls <latest.json> <assets.json> <owner/repo> <tag>
//     Prints latest.json with every download URL pointing at the published tag.
//   node scripts/release.ts add-windows <latest.json> <installer.exe.sig> <owner/repo> <tag>
//     Prints latest.json with the Windows installer added next to the macOS entries.
//   node scripts/release.ts release-type <version> [--signed]
//     Prints `prerelease=<bool>` and `manifest=<bool>` lines for $GITHUB_OUTPUT.
//   node scripts/release.ts flag-fix <tag> <published as prerelease: true|false> [<latest tag>]
//     Prints what to do about a published release's prerelease flag (see FlagFix).
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

interface Version {
  core: [number, number, number]
  /** Prerelease identifiers: `1.0.0-beta.2` has ['beta', '2']; a release has none. */
  pre: string[]
}

/** Parses a SemVer version or tag (`1.0.0-beta.2`, `v1.0.0`); build metadata is ignored. */
export function parseVersion(text: string): Version {
  const match = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(text)
  if (!match) throw new Error(`${text}: not a SemVer version`)
  const [, major, minor, patch, pre] = match
  return { core: [Number(major), Number(minor), Number(patch)], pre: pre ? pre.split('.') : [] }
}

const isNumeric = (id: string) => /^\d+$/.test(id)

function compareIdentifiers(a: string, b: string): number {
  if (isNumeric(a) && isNumeric(b)) return Number(a) - Number(b)
  if (isNumeric(a) !== isNumeric(b)) return isNumeric(a) ? -1 : 1
  return a < b ? -1 : a > b ? 1 : 0
}

/** SemVer precedence: negative when `a` is older than `b`, 0 when equal, positive when newer. */
export function compareVersions(a: string, b: string): number {
  const [x, y] = [parseVersion(a), parseVersion(b)]
  for (let i = 0; i < 3; i++) if (x.core[i] !== y.core[i]) return x.core[i] - y.core[i]
  // A prerelease comes before its release: 1.0.0-beta.9 < 1.0.0.
  if (!x.pre.length || !y.pre.length) return y.pre.length - x.pre.length
  for (let i = 0; i < Math.min(x.pre.length, y.pre.length); i++) {
    const order = compareIdentifiers(x.pre[i], y.pre[i])
    if (order) return order
  }
  return x.pre.length - y.pre.length
}

/** 1.0.0-beta.0 and the like, from Changesets pre mode: drafted as a GitHub prerelease. */
export const isPrerelease = (version: string) => parseVersion(version).pre.length > 0

/**
 * Whether a release gets a latest.json. Unsigned macOS test builds don't, and neither do betas:
 * testers install betas by hand, and without a manifest even a beta published by mistake as a
 * full release updates no one.
 */
export const wantsManifest = (version: string, signed: boolean) => signed && !isPrerelease(version)

/**
 * What to do about a published release's prerelease flag:
 * - `none`: it matches the version.
 * - `prerelease`: a beta published as a full release; mark it a prerelease so releases/latest
 *   (the updater and the landing page) skips it.
 * - `latest`: a stable version published as a prerelease and newer than the latest release; make
 *   it the latest release, once its latest.json is rewritten.
 * - `older`: the same, but not newer than the latest release; leave it and warn.
 */
export type FlagFix = 'none' | 'prerelease' | 'latest' | 'older'

export function flagFix(tag: string, prerelease: boolean, latestTag = ''): FlagFix {
  const beta = isPrerelease(tag)
  if (beta === prerelease) return 'none'
  if (beta) return 'prerelease'
  return latestTag && compareVersions(tag, latestTag) <= 0 ? 'older' : 'latest'
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
  if (command === 'release-type' && rest.length >= 1 && rest.length <= 2) {
    const [version, flag] = rest
    const signed = flag === '--signed'
    if (flag !== undefined && !signed) throw new Error(`release-type: unknown flag ${flag}`)
    return `prerelease=${isPrerelease(version)}\nmanifest=${wantsManifest(version, signed)}\n`
  }
  if (command === 'flag-fix' && rest.length >= 2 && rest.length <= 3) {
    const [tag, prerelease, latestTag] = rest
    if (prerelease !== 'true' && prerelease !== 'false') {
      throw new Error(`flag-fix: expected true or false, not ${prerelease}`)
    }
    return `${flagFix(tag, prerelease === 'true', latestTag)}\n`
  }
  throw new Error(
    'usage: release.ts notes <version> [--unsigned] | ' +
      'updater-urls <latest.json> <assets.json> <owner/repo> <tag> | ' +
      'add-windows <latest.json> <installer.exe.sig> <owner/repo> <tag> | ' +
      'release-type <version> [--signed] | ' +
      'flag-fix <tag> <true|false> [<latest tag>]',
  )
}

if (import.meta.main) process.stdout.write(main(process.argv.slice(2)))
