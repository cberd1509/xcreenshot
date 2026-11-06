// Writes the release notes and the updater manifest (latest.json) for the build-release workflow.
// Usage: node scripts/release.ts <version> <tarball-url> <sig-file> <out-dir> [--unsigned]
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

export const UNSIGNED_WARNING =
  '> [!WARNING]\n' +
  '> Unsigned test build: on first open use System Settings › Privacy & Security › Open Anyway.'

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

/** The manifest tauri-plugin-updater reads. The universal build serves both Mac architectures. */
export function latestJson(
  version: string,
  notes: string,
  url: string,
  signature: string,
  pubDate: Date,
): string {
  const platform = { signature: signature.trim(), url }
  return `${JSON.stringify(
    {
      version,
      notes,
      pub_date: pubDate.toISOString(),
      platforms: { 'darwin-aarch64': platform, 'darwin-x86_64': platform },
    },
    null,
    2,
  )}\n`
}

export function main(args: string[], now = new Date(), changelogPath = 'CHANGELOG.md'): void {
  const [version, url, sigFile, outDir, flag] = args
  if (!version || !url || !sigFile || !outDir) {
    throw new Error('usage: release.ts <version> <tarball-url> <sig-file> <out-dir> [--unsigned]')
  }
  let changelog = ''
  try {
    changelog = readFileSync(changelogPath, 'utf8')
  } catch {
    // No release has written a changelog yet.
  }
  const section = changelogSection(changelog, version)
  writeFileSync(join(outDir, 'notes.md'), releaseNotes(section, flag === '--unsigned'))
  const signature = readFileSync(sigFile, 'utf8')
  writeFileSync(join(outDir, 'latest.json'), latestJson(version, section, url, signature, now))
}

if (import.meta.main) main(process.argv.slice(2))
