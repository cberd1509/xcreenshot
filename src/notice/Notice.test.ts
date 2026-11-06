import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { shortcutKeys } from '$lib/keys'
import { tick } from '../test/tick'
import { card, type Notice as Shown } from './card'
import Notice from './Notice.svelte'

const UPDATE: Shown = { kind: 'updateReady', version: '0.3.0' }
const SHORTCUT: Shown = {
  kind: 'shortcutChanged',
  reason: 'snippingTool',
  accelerator: 'Control+Shift+PrintScreen',
}

let notice: Shown | null
let answers: boolean[]
let failing: Set<string>

const button = (name: string) => screen.getByRole('button', { name })
const settle = async () => {
  await tick()
  await tick()
}

async function mount(shown: Shown | null) {
  notice = shown
  render(Notice)
  await settle()
}

describe('Notice', () => {
  beforeEach(() => {
    answers = []
    failing = new Set()
    mockWindows('notice-update')
    mockIPC((cmd, args) => {
      if (failing.has(cmd)) throw new Error('nope')
      if (cmd === 'notice') return notice
      if (cmd === 'notice_answer') answers.push((args as { primary: boolean }).primary)
    })
  })

  afterEach(async () => {
    cleanup()
    await tick()
    clearMocks()
    vi.restoreAllMocks()
  })

  it('shows the update with the app icon and focuses Restart now', async () => {
    await mount(UPDATE)
    expect(screen.getByRole('heading', { name: 'Update ready' })).toBeInTheDocument()
    expect(
      screen.getByText('xcreenshot 0.3.0 is downloaded. Restart to finish updating.'),
    ).toBeInTheDocument()
    expect(screen.getByRole('img', { name: 'xcreenshot' })).toHaveAttribute(
      'src',
      expect.stringContaining('128x128@2x.png'),
    )
    const buttons = screen.getAllByRole('button').map((b) => b.textContent?.trim())
    expect(buttons).toEqual(['Later', 'Restart now'])
    expect(document.activeElement).toBe(button('Restart now'))
  })

  it('answers with the button clicked, once', async () => {
    await mount(UPDATE)
    await fireEvent.click(button('Restart now'))
    await fireEvent.click(button('Later'))
    await settle()
    expect(answers).toEqual([true])
  })

  it('takes Esc as Later on the update', async () => {
    await mount(UPDATE)
    await fireEvent.keyDown(window, { key: 'Escape' })
    await settle()
    expect(answers).toEqual([false])
  })

  it('takes Enter outside the buttons as the main one', async () => {
    await mount(UPDATE)
    await fireEvent.keyDown(document.body, { key: 'Enter' })
    await settle()
    expect(answers).toEqual([true])
  })

  it('leaves Enter on a button to the button', async () => {
    await mount(UPDATE)
    await fireEvent.keyDown(button('Later'), { key: 'Enter' })
    await fireEvent.keyDown(window, { key: 'a' })
    await settle()
    expect(answers).toEqual([])
  })

  it('shows the shortcut it moved to as key caps', async () => {
    await mount(SHORTCUT)
    expect(screen.getByRole('heading', { name: 'Shortcut changed' })).toBeInTheDocument()
    expect(screen.getByText(/PrintScreen opens Snipping Tool/)).toBeInTheDocument()
    const keys = [...document.querySelectorAll('kbd')].map((k) => k.textContent)
    expect(keys).toEqual(shortcutKeys(SHORTCUT.accelerator))
    expect(screen.getAllByRole('button').map((b) => b.textContent?.trim())).toEqual([
      'Open Settings',
      'OK',
    ])
    await fireEvent.click(button('Open Settings'))
    await settle()
    expect(answers).toEqual([false])
  })

  it('takes Esc as OK on the shortcut notice', async () => {
    await mount(SHORTCUT)
    await fireEvent.keyDown(window, { key: 'Escape' })
    await settle()
    expect(answers).toEqual([true])
  })

  it('shows nothing and ignores keys without a notice', async () => {
    await mount(null)
    expect(screen.queryByRole('button')).toBeNull()
    await fireEvent.keyDown(window, { key: 'Escape' })
    await settle()
    expect(answers).toEqual([])
  })

  it('logs failed commands and lets a failed answer be tried again', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    await mount(UPDATE)
    failing = new Set(['notice_answer'])
    await fireEvent.click(button('Later'))
    await settle()
    expect(error).toHaveBeenCalledWith('notice_answer failed', expect.any(Error))
    failing.clear()
    await fireEvent.click(button('Later'))
    await settle()
    expect(answers).toEqual([false])
  })

  it('logs when the notice cannot be read', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    failing = new Set(['notice'])
    await mount(UPDATE)
    expect(error).toHaveBeenCalledWith('notice failed', expect.any(Error))
    expect(screen.queryByRole('button')).toBeNull()
  })
})

describe('card', () => {
  it('blames another app when PrintScreen is taken', () => {
    expect(card({ ...SHORTCUT, reason: 'inUse' } as Shown).text).toBe(
      'Another app is using PrintScreen, so xcreenshot uses the shortcut below. You can change it in Settings.',
    )
  })
})
