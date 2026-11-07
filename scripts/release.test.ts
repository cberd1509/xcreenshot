// @vitest-environment node
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  UNSIGNED_MARKER,
  UNSIGNED_WARNING,
  WINDOWS_NOTE,
  addWindows,
  changelogSection,
  main,
  releaseNotes,
  tagDownloadUrls,
  type UpdaterManifest,
} from './release.ts'

const CHANGELOG = `# xcreenshot

## 0.2.0

### Minor Changes

- abc123: Add the editor.

## 0.1.1

### Patch Changes

- def456: Fix the tray icon.
`

const REPO = 'cberd1509/xcreenshot'
const API = `https://api.github.com/repos/${REPO}/releases/assets`
const DOWNLOAD = `https://github.com/${REPO}/releases/download/v0.2.0`
const TARBALL = 'xcreenshot_0.2.0_universal.app.tar.gz'

// The shape tauri-action v1 writes for a universal macOS build.
const MANIFEST: UpdaterManifest = {
  version: '0.2.0',
  notes: 'Add the editor.',
  pub_date: '2026-11-06T12:00:00.000Z',
  platforms: {
    'darwin-aarch64': { signature: 'SIG', url: `${API}/101` },
    'darwin-x86_64': { signature: 'SIG', url: `${API}/101` },
    'darwin-aarch64-app': { signature: 'SIG', url: `${API}/101` },
    'darwin-x86_64-app': { signature: 'SIG', url: `${API}/101` },
  },
}
const ASSETS = [
  { id: 100, name: 'xcreenshot_0.2.0_universal.dmg' },
  { id: 101, name: TARBALL },
  { id: 102, name: `${TARBALL}.sig` },
]

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
  it('adds the macOS warning to unsigned builds only, and always the Windows note', () => {
    expect(releaseNotes('- a', true)).toBe(`${UNSIGNED_WARNING}\n\n${WINDOWS_NOTE}\n\n- a\n`)
    expect(releaseNotes('- a', false)).toBe(`${WINDOWS_NOTE}\n\n- a\n`)
  })

  it('marks unsigned notes the way the publish check looks for them', () => {
    expect(releaseNotes('- a', true)).toContain(UNSIGNED_MARKER)
    expect(releaseNotes('- a', false)).not.toContain(UNSIGNED_MARKER)
    const workflow = readFileSync(
      new URL('../.github/workflows/release.yml', import.meta.url),
      'utf8',
    )
    expect(workflow).toContain(`*"${UNSIGNED_MARKER}"*`)
  })

  it('tells Windows users how to get past SmartScreen', () => {
    expect(WINDOWS_NOTE).toContain('More info › Run anyway')
  })

  it('falls back when the changelog has no entry', () => {
    expect(releaseNotes('', false)).toBe(
      `${WINDOWS_NOTE}\n\nNo changelog entry for this version.\n`,
    )
  })
})

describe('tagDownloadUrls', () => {
  it('points every platform at the tag download URL and keeps everything else', () => {
    const entry = { signature: 'SIG', url: `${DOWNLOAD}/${TARBALL}` }
    expect(tagDownloadUrls(MANIFEST, ASSETS, REPO, 'v0.2.0')).toEqual({
      version: '0.2.0',
      notes: 'Add the editor.',
      pub_date: '2026-11-06T12:00:00.000Z',
      platforms: {
        'darwin-aarch64': entry,
        'darwin-x86_64': entry,
        'darwin-aarch64-app': entry,
        'darwin-x86_64-app': entry,
      },
    })
  })

  it('leaves URLs that already point at the tag', () => {
    const done = tagDownloadUrls(MANIFEST, ASSETS, REPO, 'v0.2.0')
    expect(tagDownloadUrls(done, [], REPO, 'v0.2.0')).toEqual(done)
  })

  it('encodes asset names', () => {
    const manifest = { platforms: { 'darwin-aarch64': { signature: 'S', url: `${API}/7` } } }
    const out = tagDownloadUrls(manifest, [{ id: 7, name: 'a b.tar.gz' }], REPO, 'v0.2.0')
    expect(out.platforms['darwin-aarch64'].url).toBe(`${DOWNLOAD}/a%20b.tar.gz`)
  })

  it('fails on an unknown asset or URL rather than writing a broken manifest', () => {
    expect(() => tagDownloadUrls(MANIFEST, [], REPO, 'v0.2.0')).toThrow('darwin-aarch64')
    const other = { platforms: { 'darwin-aarch64': { signature: 'S', url: 'https://x/y' } } }
    expect(() => tagDownloadUrls(other, ASSETS, REPO, 'v0.2.0')).toThrow('https://x/y')
  })
})

const SETUP = 'xcreenshot_0.2.0_x64-setup.exe'

describe('addWindows', () => {
  it('adds the installer under both Windows keys and keeps the macOS entries', () => {
    const out = addWindows(MANIFEST, 'WINSIG\n', SETUP, REPO, 'v0.2.0')
    const entry = { signature: 'WINSIG', url: `${DOWNLOAD}/${SETUP}` }
    expect(out).toEqual({
      ...MANIFEST,
      platforms: {
        ...MANIFEST.platforms,
        'windows-x86_64': entry,
        'windows-x86_64-nsis': entry,
      },
    })
  })

  it('keeps the URL when the tag download rewrite runs on it later', () => {
    const done = tagDownloadUrls(MANIFEST, ASSETS, REPO, 'v0.2.0')
    const both = addWindows(done, 'WINSIG', SETUP, REPO, 'v0.2.0')
    expect(tagDownloadUrls(both, [], REPO, 'v0.2.0')).toEqual(both)
  })

  it('refuses anything but a signed NSIS installer', () => {
    expect(() => addWindows(MANIFEST, 'S', 'xcreenshot.msi', REPO, 'v0.2.0')).toThrow('NSIS')
    expect(() => addWindows(MANIFEST, ' \n', SETUP, REPO, 'v0.2.0')).toThrow('empty signature')
  })
})

describe('main', () => {
  const dir = mkdtempSync(join(tmpdir(), 'release-'))
  const changelogPath = join(dir, 'CHANGELOG.md')
  writeFileSync(changelogPath, CHANGELOG)

  it('prints the notes for a version', () => {
    expect(main(['notes', '0.2.0'], changelogPath)).toBe(
      `${WINDOWS_NOTE}\n\n### Minor Changes\n\n- abc123: Add the editor.\n`,
    )
    expect(main(['notes', '0.2.0', '--unsigned'], changelogPath)).toContain(UNSIGNED_WARNING)
    expect(main(['notes', '0.1.0'], join(dir, 'none.md'))).toContain(
      'No changelog entry for this version.\n',
    )
  })

  it('prints latest.json with tag URLs', () => {
    const manifestPath = join(dir, 'latest.json')
    const assetsPath = join(dir, 'assets.json')
    writeFileSync(manifestPath, JSON.stringify(MANIFEST))
    writeFileSync(assetsPath, JSON.stringify(ASSETS))
    const out = JSON.parse(main(['updater-urls', manifestPath, assetsPath, REPO, 'v0.2.0']))
    expect(out.platforms['darwin-x86_64']).toEqual({
      signature: 'SIG',
      url: `${DOWNLOAD}/${TARBALL}`,
    })
    expect(out.version).toBe('0.2.0')
  })

  it('prints latest.json with the Windows installer added', () => {
    const manifestPath = join(dir, 'mac.json')
    const sigPath = join(dir, `${SETUP}.sig`)
    writeFileSync(manifestPath, JSON.stringify(MANIFEST))
    writeFileSync(sigPath, 'WINSIG')
    const out = JSON.parse(main(['add-windows', manifestPath, sigPath, REPO, 'v0.2.0']))
    expect(out.platforms['windows-x86_64']).toEqual({
      signature: 'WINSIG',
      url: `${DOWNLOAD}/${SETUP}`,
    })
    expect(out.platforms['darwin-aarch64']).toEqual(MANIFEST.platforms['darwin-aarch64'])
  })

  it('rejects bad arguments', () => {
    expect(() => main([])).toThrow('usage')
    expect(() => main(['notes'])).toThrow('usage')
    expect(() => main(['updater-urls', 'a'])).toThrow('usage')
    expect(() => main(['add-windows', 'a'])).toThrow('usage')
  })
})
