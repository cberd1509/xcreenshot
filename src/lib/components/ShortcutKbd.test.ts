import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import { shortcutKeys } from '$lib/keys'
import ShortcutKbd from './ShortcutKbd.svelte'

describe('ShortcutKbd', () => {
  it('shows one key per part of the accelerator', () => {
    const accelerator = 'Control+Shift+S'
    const { container } = render(ShortcutKbd, { accelerator })
    const keys = shortcutKeys(accelerator)
    expect(container.querySelectorAll('[data-slot="kbd"]')).toHaveLength(keys.length)
    for (const key of keys) expect(screen.getByText(key)).toBeInTheDocument()
  })
})
