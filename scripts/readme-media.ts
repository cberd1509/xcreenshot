// Renders the README media from the real UI in a browser, with Tauri mocked: nothing comes from
// the screen. The overlay shows a made-up desktop (docs/media/src/desktop.html) as its frame.
// Usage: pnpm readme-media
// Needs ffmpeg on PATH and Playwright's Chromium (`pnpm exec playwright install chromium`).
// Writes docs/media/overlay.png, docs/media/settings.png and docs/media/demo.gif.
/// <reference lib="dom" />
import { execFileSync } from 'node:child_process'
import { mkdtempSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium, type Browser, type BrowserContextOptions, type Page } from 'playwright'
import { createServer } from 'vite'
import { gifArgs, path, size, steps, type Point } from './media.ts'

const root = fileURLToPath(new URL('..', import.meta.url))
const out = join(root, 'docs/media')
const viewport = { width: 1440, height: 900 }
const GIF = { width: 1280, fps: 15 }
/** Pointer moves per second while recording. */
const FPS = 20

type Box = { x: number; y: number; width: number; height: number }
/** Where things are on the demo desktop, in logical px. */
type Spots = { email: Box; save: Box; general: Box; notifications: Box }

const sleep = (ms: number) => new Promise((r) => setTimeout(r, Math.max(0, ms)))
const center = (b: Box): Point => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 })

/** The pointer, moved like a hand moves it. */
class Hand {
  at: Point = { x: 720, y: 450 }
  readonly page: Page
  /** Slower for the recording, instant for stills. */
  readonly paced: boolean

  constructor(page: Page, paced: boolean) {
    this.page = page
    this.paced = paced
  }

  async move(to: Point, slow = 1) {
    const n = this.paced ? Math.round(steps(this.at, to, FPS) * slow) : 1
    const start = Date.now()
    // One move after another, on the clock however long each takes.
    for (const [i, p] of path(this.at, to, n, this.paced ? 0.08 : 0).entries()) {
      // oxlint-disable-next-line no-await-in-loop
      await this.page.mouse.move(p.x, p.y)
      // oxlint-disable-next-line no-await-in-loop
      if (this.paced) await sleep(start + ((i + 1) * 1000) / FPS - Date.now())
    }
    this.at = to
  }

  async drag(from: Point, to: Point) {
    await this.move(from)
    await this.page.mouse.down()
    if (this.paced) await sleep(80)
    await this.move(to, 1.1)
    if (this.paced) await sleep(80)
    await this.page.mouse.up()
  }

  async click(to: Point) {
    await this.move(to)
    await this.page.mouse.down()
    await this.page.mouse.up()
  }

  /** Picks a tool on the rail. */
  async tool(name: string) {
    const box = await this.page.getByLabel(name, { exact: true }).first().boundingBox()
    if (!box) throw new Error(`no ${name} tool on the rail`)
    await this.click(center(box))
    if (this.paced) await sleep(150)
  }
}

/** Renders the demo desktop: the frame, and where its fields are. */
async function desktop(browser: Browser, base: string) {
  const page = await browser.newPage({ viewport, deviceScaleFactor: 2 })
  await page.goto(`${base}docs/media/src/desktop.html`)
  await page.evaluate(() => document.fonts.ready)
  const frame = await page.screenshot()
  const box = (selector: string) => page.locator(selector).first().boundingBox() as Promise<Box>
  const spots: Spots = {
    email: await box('.field >> nth=1'),
    save: await box('.btn.dark'),
    general: await box('.card'),
    notifications: await box('.card >> nth=1'),
  }
  await page.close()
  return { frame, spots }
}

/** A page with the overlay open on the frame, ready for a drag. */
async function overlay(
  browser: Browser,
  base: string,
  frame: Buffer,
  options: BrowserContextOptions,
) {
  const context = await browser.newContext({ viewport, deviceScaleFactor: 2, ...options })
  await context.route('**/frame.png', (route) =>
    route.fulfill({ body: frame, contentType: 'image/png' }),
  )
  const page = await context.newPage()
  // A recording starts with the page.
  const created = Date.now()
  page.on('pageerror', (e) => console.error('page error:', e))
  await page.goto(`${base}docs/media/src/overlay.html`)
  await page.waitForFunction(() => 'demo' in window)
  // The desktop behind the overlay, loaded before anything shows.
  await page.evaluate(async () => {
    const image = new Image()
    image.src = '/frame.png'
    await image.decode()
  })
  return { context, page, created }
}

/** The overlay page's controls (docs/media/src/overlay.ts). */
type Demo = Record<string, (...args: unknown[]) => unknown>

const demo = (page: Page, name: string, ...values: unknown[]) =>
  page.evaluate(([n, v]) => (window as unknown as { demo: Demo }).demo[n](...v), [
    name,
    values,
  ] as const)

async function open(page: Page) {
  await demo(page, 'open')
  await page.getByTestId('hint').waitFor()
}

/** The region and the marks drawn in every take. */
function plan(s: Spots) {
  const pad = 20
  const card = s.notifications
  const tail = { x: s.save.x - 150, y: s.save.y + s.save.height + 70 }
  return {
    region: {
      from: { x: s.general.x - pad, y: s.general.y - 70 },
      to: { x: card.x + card.width + pad, y: card.y + card.height + pad },
    },
    rect: {
      from: { x: s.save.x - 7, y: s.save.y - 7 },
      to: { x: s.save.x + s.save.width + 7, y: s.save.y + s.save.height + 7 },
    },
    arrow: {
      from: tail,
      to: { x: s.save.x - 14, y: s.save.y + s.save.height / 2 + 6 },
    },
    blur: {
      from: { x: s.email.x + 4, y: s.email.y + 4 },
      to: { x: s.email.x + 200, y: s.email.y + s.email.height - 4 },
    },
    label: { x: tail.x - 160, y: tail.y - 8 },
  }
}

async function annotate(hand: Hand, p: ReturnType<typeof plan>) {
  await hand.tool('Arrow')
  await hand.drag(p.arrow.from, p.arrow.to)
  await hand.tool('Rectangle')
  await hand.drag(p.rect.from, p.rect.to)
  await hand.tool('Blur')
  await hand.drag(p.blur.from, p.blur.to)
}

async function still(browser: Browser, base: string, frame: Buffer, spots: Spots) {
  const { context, page } = await overlay(browser, base, frame, {})
  await demo(page, 'cursor', false)
  await open(page)
  const p = plan(spots)
  const hand = new Hand(page, false)
  await hand.drag(p.region.from, p.region.to)
  await annotate(hand, p)
  await hand.tool('Text')
  await hand.click(p.label)
  await page.keyboard.type('Click Save')
  // A press while typing commits the text.
  await hand.click({ x: p.region.from.x + 30, y: p.region.to.y - 20 })
  await hand.tool('Select')
  await hand.move({ x: 1400, y: 40 })
  await sleep(500)
  await page.screenshot({ path: join(out, 'overlay.png') })
  await context.close()
}

async function settings(browser: Browser, base: string) {
  const page = await browser.newPage({
    viewport: { width: 880, height: 640 },
    deviceScaleFactor: 2,
    colorScheme: 'light',
  })
  await page.goto(`${base}docs/media/src/settings.html`)
  await page.getByText('Check for updates').waitFor()
  await sleep(300)
  await page.screenshot({ path: join(out, 'settings.png') })
  await page.close()
}

async function recording(browser: Browser, base: string, frame: Buffer, spots: Spots, dir: string) {
  const { context, page, created } = await overlay(browser, base, frame, {
    recordVideo: { dir, size: { width: viewport.width * 2, height: viewport.height * 2 } },
  })
  const p = plan(spots)
  const hand = new Hand(page, true)
  await page.mouse.move(hand.at.x, hand.at.y)
  await sleep(300)
  const clip = (Date.now() - created) / 1000
  await sleep(400)
  await demo(page, 'keys', '⌃', '⇧', '⌘', 'S')
  await sleep(450)
  await open(page)
  await demo(page, 'keys')
  await sleep(700)
  await hand.drag(p.region.from, p.region.to)
  await sleep(300)
  await annotate(hand, p)
  await sleep(250)
  await demo(page, 'keys', '⌘', 'C')
  await sleep(200)
  await page.keyboard.press('Meta+c')
  await page.waitForFunction(
    () => ((window as unknown as { demo: Demo }).demo.copies() as number) > 0,
  )
  // The overlay closes: the pointer is the desktop's again.
  await hand.move({ x: hand.at.x + 40, y: hand.at.y + 30 })
  await demo(page, 'keys')
  await sleep(700)
  const duration = (Date.now() - created) / 1000 - clip
  const video = page.video()!
  await context.close()
  return { webm: await video.path(), clip, duration }
}

const server = await createServer({
  root,
  logLevel: 'warn',
  server: { port: 5180, strictPort: false },
})
await server.listen()
const base = server.resolvedUrls!.local[0]
const browser = await chromium.launch()
const dir = mkdtempSync(join(tmpdir(), 'readme-media-'))
try {
  const { frame, spots } = await desktop(browser, base)
  await still(browser, base, frame, spots)
  await settings(browser, base)
  const { webm, clip, duration } = await recording(browser, base, frame, spots, dir)
  const gif = join(out, 'demo.gif')
  execFileSync('ffmpeg', gifArgs({ input: webm, output: gif, start: clip, duration, ...GIF }))
  console.log(`demo.gif: ${duration.toFixed(1)} s at ${GIF.fps} fps`)
  for (const name of ['overlay.png', 'settings.png', 'demo.gif'])
    console.log(`${name}: ${size(statSync(join(out, name)).size)}`)
} finally {
  await browser.close()
  await server.close()
  rmSync(dir, { recursive: true, force: true })
}
