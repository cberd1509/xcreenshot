import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { UNSIGNED_WARNING, changelogSection, latestJson, main, releaseNotes } from './release.ts'

const CHANGELOG = `# xcreenshot

## 0.2.0

### Minor Changes

- abc123: Add the editor.

## 0.1.1

### Patch Changes

- def456: Fix the tray icon.
`

describe('changelogSection', () => {
  it('returns the body of the matching version only', () => {
    expect(changelogSection(CHANGELOG, '0.2.0')).toBe(
      '### Minor Changes\n\n- abc123: Add the editor.',
    )
    expect(changelogSection(CHANGELOG, '0.1.1')).toBe(
      '### Patch Changes\n\n- def456: Fix the tray icon.',
    )
  })

  it('returns an empty string for a missing version', () => {
    expect(changelogSection(CHANGELOG, '9.9.9')).toBe('')
    expect(changelogSection('', '0.1.0')).toBe('')
  })

  it('does not match a version that only shares a prefix', () => {
    expect(changelogSection('## 0.2.0-beta\n\n- x\n', '0.2.0')).toBe('')
  })
})

describe('releaseNotes', () => {
  it('adds the warning to unsigned builds only', () => {
    expect(releaseNotes('- a', true)).toBe(`${UNSIGNED_WARNING}\n\n- a\n`)
    expect(releaseNotes('- a', false)).toBe('- a\n')
  })

  it('falls back when the changelog has no entry', () => {
    expect(releaseNotes('', false)).toBe('No changelog entry for this version.\n')
  })
})

describe('latestJson', () => {
  it('points both Mac architectures at the universal build', () => {
    const json = JSON.parse(
      latestJson('0.2.0', 'notes', 'https://x/a.tar.gz', 'sig\n', new Date('2026-01-02T03:04:05Z')),
    )
    const platform = { signature: 'sig', url: 'https://x/a.tar.gz' }
    expect(json).toEqual({
      version: '0.2.0',
      notes: 'notes',
      pub_date: '2026-01-02T03:04:05.000Z',
      platforms: { 'darwin-aarch64': platform, 'darwin-x86_64': platform },
    })
  })
})

function setup() {
  const dir = mkdtempSync(join(tmpdir(), 'release-'))
  writeFileSync(join(dir, 'app.sig'), 'SIG')
  writeFileSync(join(dir, 'CHANGELOG.md'), CHANGELOG)
  return dir
}

describe('main', () => {
  const now = new Date('2026-01-02T00:00:00Z')

  it('writes notes.md and latest.json', () => {
    const dir = setup()
    const args = ['0.2.0', 'https://x/a.tar.gz', join(dir, 'app.sig'), dir, '--unsigned']
    main(args, now, join(dir, 'CHANGELOG.md'))
    const notes = readFileSync(join(dir, 'notes.md'), 'utf8')
    expect(notes).toContain(UNSIGNED_WARNING)
    expect(notes).toContain('Add the editor.')
    const json = JSON.parse(readFileSync(join(dir, 'latest.json'), 'utf8'))
    expect(json.notes).toBe('### Minor Changes\n\n- abc123: Add the editor.')
    expect(json.platforms['darwin-aarch64'].signature).toBe('SIG')
  })

  it('works without a changelog', () => {
    const dir = setup()
    main(['0.1.0', 'https://x/a.tar.gz', join(dir, 'app.sig'), dir], now, join(dir, 'none.md'))
    expect(readFileSync(join(dir, 'notes.md'), 'utf8')).toBe(
      'No changelog entry for this version.\n',
    )
  })

  it('rejects missing arguments', () => {
    expect(() => main(['0.1.0'])).toThrow('usage')
  })
})
