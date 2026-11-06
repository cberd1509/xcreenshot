import { render, screen } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { getVersion } from '@tauri-apps/api/app'
import { describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import { settings, shortcutKeys } from '$lib/settings.svelte'

describe('Settings window', () => {
  it('shows every section and the app version', async () => {
    render(App)
    for (const name of ['Shortcuts', 'Capture', 'General']) {
      expect(screen.getByRole('heading', { name })).toBeInTheDocument()
    }
    expect(await screen.findByText('Version 0.1.0')).toBeInTheDocument()
  })

  it('shows the capture shortcut as keys', () => {
    render(App)
    expect(screen.getByText('Take a screenshot')).toBeInTheDocument()
    for (const key of shortcutKeys(settings.shortcut)) {
      expect(screen.getByText(key)).toBeInTheDocument()
    }
  })

  it('toggles a setting from its switch', async () => {
    render(App)
    const magnifier = screen.getByRole('switch', { name: 'Magnifier' })
    expect(magnifier).not.toBeChecked()
    await userEvent.click(magnifier)
    expect(settings.magnifier).toBe(true)
    expect(magnifier).toBeChecked()
  })

  it('marks the clicked section as current', async () => {
    render(App)
    const general = screen.getByRole('link', { name: 'General' })
    expect(general).not.toHaveAttribute('aria-current')
    await userEvent.click(general)
    expect(general).toHaveAttribute('aria-current', 'true')
  })

  it('hides the version when it cannot be read', async () => {
    vi.mocked(getVersion).mockRejectedValueOnce(new Error('no runtime'))
    render(App)
    await Promise.resolve()
    expect(screen.queryByText(/^Version/)).not.toBeInTheDocument()
  })
})
