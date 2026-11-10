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
  compareVersions,
  flagFix,
  isPrerelease,
  main,
  parseVersion,
  releaseNotes,
  tagDownloadUrls,
  type UpdaterManifest,
  wantsManifest,
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

const workflow = (name: string) =>
  readFileSync(new URL(`../.github/workflows/${name}`, import.meta.url), 'utf8')

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
    expect(workflow('release.yml')).toContain(`*"${UNSIGNED_MARKER}"*`)
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
    expect(tagDownloadUrls(done, ASSETS, REPO, 'v0.2.0')).toEqual(done)
  })

  it('moves download URLs to another tag, by asset name (#135)', () => {
    const draft = tagDownloadUrls(MANIFEST, ASSETS, REPO, 'v0.2.0')
    const renamed = [{ id: 201, name: TARBALL }]
    const out = tagDownloadUrls(draft, renamed, REPO, 'v0.2.0-fixed')
    expect(out.platforms['darwin-aarch64'].url).toBe(
      `https://github.com/${REPO}/releases/download/v0.2.0-fixed/${TARBALL}`,
    )
  })

  it('encodes asset names', () => {
    const manifest = { platforms: { 'darwin-aarch64': { signature: 'S', url: `${API}/7` } } }
    const out = tagDownloadUrls(manifest, [{ id: 7, name: 'a b.tar.gz' }], REPO, 'v0.2.0')
    expect(out.platforms['darwin-aarch64'].url).toBe(`${DOWNLOAD}/a%20b.tar.gz`)
  })

  it('fails on an unknown asset or URL rather than writing a broken manifest', () => {
    expect(() => tagDownloadUrls(MANIFEST, [], REPO, 'v0.2.0')).toThrow('darwin-aarch64')
    const done = tagDownloadUrls(MANIFEST, ASSETS, REPO, 'v0.2.0')
    expect(() => tagDownloadUrls(done, ASSETS.slice(0, 1), REPO, 'v0.2.0')).toThrow(TARBALL)
    for (const url of [
      'https://x/y',
      `${DOWNLOAD}/`,
      `${DOWNLOAD}/a/${TARBALL}`,
      `https://github.com/other/repo/releases/download/v0.2.0/${TARBALL}`,
    ]) {
      const other = { platforms: { 'darwin-aarch64': { signature: 'S', url } } }
      expect(() => tagDownloadUrls(other, ASSETS, REPO, 'v0.2.0')).toThrow(url)
    }
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
    const assets = [...ASSETS, { id: 103, name: SETUP }]
    expect(tagDownloadUrls(both, assets, REPO, 'v0.2.0')).toEqual(both)
  })

  it('refuses anything but a signed NSIS installer', () => {
    expect(() => addWindows(MANIFEST, 'S', 'xcreenshot.msi', REPO, 'v0.2.0')).toThrow('NSIS')
    expect(() => addWindows(MANIFEST, ' \n', SETUP, REPO, 'v0.2.0')).toThrow('empty signature')
  })
})

describe('parseVersion', () => {
  it('reads versions and tags, with prerelease identifiers', () => {
    expect(parseVersion('1.0.0-beta.2')).toEqual({ core: [1, 0, 0], pre: ['beta', '2'] })
    expect(parseVersion('v0.4.0')).toEqual({ core: [0, 4, 0], pre: [] })
    expect(parseVersion('1.2.3+build.5')).toEqual({ core: [1, 2, 3], pre: [] })
  })

  it('rejects anything else', () => {
    for (const bad of ['', '1.0', 'v1.0.0-', 'latest']) {
      expect(() => parseVersion(bad)).toThrow('not a SemVer version')
    }
  })
})

describe('compareVersions', () => {
  it('orders versions by SemVer precedence', () => {
    const ordered = [
      '0.4.0',
      '0.10.0',
      '1.0.0-alpha',
      '1.0.0-alpha.1',
      '1.0.0-alpha.beta',
      '1.0.0-beta',
      '1.0.0-beta.2',
      '1.0.0-beta.11',
      '1.0.0-rc.1',
      '1.0.0',
      '1.0.1',
      '2.0.0',
    ]
    for (let i = 1; i < ordered.length; i++) {
      expect(compareVersions(ordered[i - 1], ordered[i])).toBeLessThan(0)
      expect(compareVersions(ordered[i], ordered[i - 1])).toBeGreaterThan(0)
    }
  })

  it('treats a tag and its version, or different build metadata, as equal', () => {
    expect(compareVersions('v1.0.0-beta.1', '1.0.0-beta.1')).toBe(0)
    expect(compareVersions('1.0.0+a', '1.0.0+b')).toBe(0)
  })
})

describe('isPrerelease', () => {
  it('is true only with prerelease identifiers', () => {
    expect(isPrerelease('1.0.0-beta.0')).toBe(true)
    expect(isPrerelease('v1.0.0-rc.1')).toBe(true)
    expect(isPrerelease('1.0.0')).toBe(false)
    expect(isPrerelease('1.0.0+build-1')).toBe(false)
  })
})

describe('wantsManifest', () => {
  it('gives latest.json to signed stable releases only', () => {
    expect(wantsManifest('1.0.0', true)).toBe(true)
    expect(wantsManifest('1.0.0', false)).toBe(false)
    expect(wantsManifest('1.0.0-beta.0', true)).toBe(false)
    expect(wantsManifest('1.0.0-beta.0', false)).toBe(false)
  })
})

describe('flagFix', () => {
  it('leaves a flag that matches the version', () => {
    expect(flagFix('v1.0.0-beta.0', true, 'v0.4.0')).toBe('none')
    expect(flagFix('v1.0.0', false, 'v1.0.0')).toBe('none')
  })

  it('marks a beta published as a full release a prerelease', () => {
    expect(flagFix('v1.0.0-beta.0', false, 'v1.0.0-beta.0')).toBe('prerelease')
    expect(flagFix('v1.0.0-beta.0', false)).toBe('prerelease')
  })

  it('makes a stable prerelease the latest release only when it is newer', () => {
    expect(flagFix('v1.0.0', true, 'v0.4.0')).toBe('latest')
    expect(flagFix('v1.0.0', true)).toBe('latest')
    expect(flagFix('v0.4.1', true, 'v1.0.0')).toBe('older')
    expect(flagFix('v1.0.0', true, 'v1.0.0')).toBe('older')
  })
})

describe('the workflows', () => {
  it('take the release decisions from this script', () => {
    const release = workflow('release.yml')
    expect(release).toContain('node scripts/release.ts release-type "$VERSION"')
    expect(release).toContain("uploadUpdaterJson: ${{ steps.type.outputs.manifest == 'true' }}")
    expect(release).toContain('prerelease: ${{ steps.type.outputs.prerelease }}')
    expect(release).toContain('node scripts/release.ts flag-fix "$TAG"')
  })

  it('point the draft latest.json at the tag before it is published (#135)', () => {
    const release = workflow('release.yml')
    const record = release.indexOf('- name: Record the draft')
    const tag = release.indexOf('node scripts/release.ts updater-urls "$dir/latest.json"')
    const upload = release.indexOf('gh release upload "v$VERSION" "$dir/tagged/latest.json"')
    const merge = release.indexOf('node scripts/release.ts add-windows')
    expect(record).toBeGreaterThan(-1)
    expect(tag).toBeGreaterThan(record)
    expect(upload).toBeGreaterThan(tag)
    expect(merge).toBeGreaterThan(upload)
  })

  it('make a release the latest only after latest.json is rewritten', () => {
    const release = workflow('release.yml')
    const rewrite = release.indexOf('gh release upload "$TAG" "$dir/latest.json"')
    const latest = release.indexOf('gh release edit "$TAG" --prerelease=false --latest')
    const pages = release.indexOf('gh workflow run pages.yml')
    expect(rewrite).toBeGreaterThan(-1)
    expect(latest).toBeGreaterThan(rewrite)
    expect(pages).toBeGreaterThan(latest)
  })

  it("doesn't rebuild the landing page for a prerelease", () => {
    expect(workflow('pages.yml')).toContain(
      "if: github.event_name == 'release' && !github.event.release.prerelease",
    )
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

  it('prints the release type for $GITHUB_OUTPUT', () => {
    expect(main(['release-type', '1.0.0', '--signed'])).toBe('prerelease=false\nmanifest=true\n')
    expect(main(['release-type', '1.0.0'])).toBe('prerelease=false\nmanifest=false\n')
    expect(main(['release-type', '1.0.0-beta.1', '--signed'])).toBe(
      'prerelease=true\nmanifest=false\n',
    )
    expect(() => main(['release-type', '1.0.0', '--unsigned'])).toThrow('unknown flag')
  })

  it('prints the prerelease flag fix', () => {
    expect(main(['flag-fix', 'v1.0.0-beta.0', 'false', 'v0.4.0'])).toBe('prerelease\n')
    expect(main(['flag-fix', 'v1.0.0', 'true', ''])).toBe('latest\n')
    expect(main(['flag-fix', 'v1.0.0', 'true'])).toBe('latest\n')
    expect(() => main(['flag-fix', 'v1.0.0', 'yes'])).toThrow('expected true or false')
  })

  it('rejects bad arguments', () => {
    expect(() => main([])).toThrow('usage')
    expect(() => main(['release-type'])).toThrow('usage')
    expect(() => main(['flag-fix', 'v1.0.0'])).toThrow('usage')
    expect(() => main(['notes'])).toThrow('usage')
    expect(() => main(['updater-urls', 'a'])).toThrow('usage')
    expect(() => main(['add-windows', 'a'])).toThrow('usage')
  })
})
