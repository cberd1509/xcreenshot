// @vitest-environment node
import { describe, expect, it } from 'vitest'
import { fillPlaceholders, LATEST_RELEASE_URL, parseRelease, pickDownload } from './release'

const dmgUrl =
  'https://github.com/cberd1509/xcreenshot/releases/download/v0.4.0/xcreenshot_0.4.0_universal.dmg'

const release = {
  tag_name: 'v0.4.0',
  draft: false,
  prerelease: false,
  assets: [
    { name: 'latest.json', browser_download_url: 'https://example.com/latest.json' },
    {
      name: 'xcreenshot_0.4.0_universal.app.tar.gz',
      browser_download_url: 'https://example.com/a',
    },
    { name: 'xcreenshot_0.4.0_universal.dmg', browser_download_url: dmgUrl },
  ],
}

describe('pickDownload', () => {
  it('uses the release version and its universal DMG', () => {
    expect(pickDownload(release, '0.3.0')).toEqual({ version: '0.4.0', url: dmgUrl })
  })

  it('keeps a tag without a v prefix as is', () => {
    expect(pickDownload({ ...release, tag_name: '1.0.0' }, '0.3.0').version).toBe('1.0.0')
  })

  it('links the releases page when the release has no DMG', () => {
    const noDmg = { ...release, assets: release.assets.slice(0, 2) }
    expect(pickDownload(noDmg, '0.3.0')).toEqual({ version: '0.4.0', url: LATEST_RELEASE_URL })
  })

  it('ignores malformed assets', () => {
    const odd = { ...release, assets: [null, { name: 1 }, 'x'] }
    const kept = { version: '0.4.0', url: LATEST_RELEASE_URL }
    expect(pickDownload(odd, '0.3.0')).toEqual(kept)
    expect(pickDownload({ ...release, assets: 'nope' }, '0.3.0')).toEqual(kept)
  })

  it.each([
    ['no release', null],
    ['not an object', 'v1.0.0'],
    ['no tag', { assets: release.assets }],
    ['a draft', { ...release, draft: true }],
    ['a prerelease', { ...release, prerelease: true }],
  ])('falls back to the local version for %s', (_, input) => {
    expect(pickDownload(input, '0.3.0')).toEqual({ version: '0.3.0', url: LATEST_RELEASE_URL })
  })
})

describe('parseRelease', () => {
  it('parses JSON', () => {
    expect(parseRelease('{"tag_name":"v1"}')).toEqual({ tag_name: 'v1' })
  })

  it('returns null for missing or broken input', () => {
    expect(parseRelease(undefined)).toBeNull()
    expect(parseRelease('')).toBeNull()
    expect(parseRelease('{oops')).toBeNull()
  })
})

describe('fillPlaceholders', () => {
  it('escapes a tag or link that contains markup', () => {
    const html = '<p>Version %VERSION%</p><a href="%DOWNLOAD_URL%">x</a>'
    expect(fillPlaceholders(html, { version: '1<b>"&\'', url: 'https://a/?q="x"' })).toBe(
      '<p>Version 1&lt;b&gt;&quot;&amp;&#39;</p><a href="https://a/?q=&quot;x&quot;">x</a>',
    )
  })

  it('replaces every placeholder', () => {
    const html = '<a href="%DOWNLOAD_URL%">x</a> %VERSION% <a href="%DOWNLOAD_URL%">y</a>'
    expect(fillPlaceholders(html, { version: '1.2.3', url: dmgUrl })).toBe(
      `<a href="${dmgUrl}">x</a> 1.2.3 <a href="${dmgUrl}">y</a>`,
    )
  })
})
