// Prints the GitHub Release notes for a version, taken from CHANGELOG.md.
// Usage: node scripts/release.ts <version> [--unsigned]
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

export function main(args: string[], changelogPath = 'CHANGELOG.md'): string {
  const [version, flag] = args
  if (!version) throw new Error('usage: release.ts <version> [--unsigned]')
  let changelog = ''
  try {
    changelog = readFileSync(changelogPath, 'utf8')
  } catch {
    // No release has written a changelog yet.
  }
  return releaseNotes(changelogSection(changelog, version), flag === '--unsigned')
}

if (import.meta.main) process.stdout.write(main(process.argv.slice(2)))
