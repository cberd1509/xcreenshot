// @vitest-environment node
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { UNSIGNED_WARNING, changelogSection, main, releaseNotes } from './release.ts'

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

describe('main', () => {
  const dir = mkdtempSync(join(tmpdir(), 'release-'))
  const changelogPath = join(dir, 'CHANGELOG.md')
  writeFileSync(changelogPath, CHANGELOG)

  it('returns the notes for the version', () => {
    expect(main(['0.2.0'], changelogPath)).toBe('### Minor Changes\n\n- abc123: Add the editor.\n')
    expect(main(['0.2.0', '--unsigned'], changelogPath)).toContain(UNSIGNED_WARNING)
  })

  it('works without a changelog', () => {
    expect(main(['0.1.0'], join(dir, 'none.md'))).toBe('No changelog entry for this version.\n')
  })

  it('rejects a missing version', () => {
    expect(() => main([])).toThrow('usage')
  })
})
