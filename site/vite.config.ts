// The landing page at https://cberd1509.github.io/xcreenshot/, built separately from the app.
import { readFileSync } from 'node:fs'
import { fileURLToPath, URL } from 'node:url'
import { defineConfig, type Plugin } from 'vite'
import { fillPlaceholders, parseRelease, pickDownload } from './src/release.ts'

const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as {
  version: string
}

// CI saves `gh api repos/cberd1509/xcreenshot/releases/latest` here; locally it's unset.
const releaseFile = process.env.SITE_RELEASE_JSON
const download = pickDownload(
  parseRelease(releaseFile ? readFileSync(releaseFile, 'utf8') : undefined),
  pkg.version,
)

// Same-origin only: no fonts, scripts or analytics from anywhere else. Build only, because the dev
// server injects inline styles.
const csp = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self'",
  "img-src 'self'",
  "font-src 'self'",
  "media-src 'self'",
  "base-uri 'none'",
  "form-action 'none'",
].join('; ')

const release: Plugin = {
  name: 'xcreenshot-release',
  transformIndexHtml: { order: 'pre', handler: (html) => fillPlaceholders(html, download) },
}

const contentSecurityPolicy: Plugin = {
  name: 'xcreenshot-csp',
  apply: 'build',
  transformIndexHtml: () => [
    {
      tag: 'meta',
      attrs: { 'http-equiv': 'Content-Security-Policy', content: csp },
      injectTo: 'head-prepend',
    },
  ],
}

export default defineConfig({
  root: fileURLToPath(new URL('.', import.meta.url)),
  base: './',
  plugins: [release, contentSecurityPolicy],
  server: { port: 5181 },
  build: {
    // Keep fonts and images as files; the CSP doesn't allow data: URLs.
    assetsInlineLimit: 0,
  },
})
