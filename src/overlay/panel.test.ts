import { describe, expect, it } from 'vitest'
import { PANEL } from './panel'

describe('PANEL', () => {
  it('keeps a highlighted menu item light on the dark panel', () => {
    // The menu items' focus style uses the accent colours, which are for a light background.
    expect(PANEL).toContain('[--accent:var(--color-zinc-800)]')
    expect(PANEL).toContain('[--accent-foreground:var(--color-zinc-50)]')
  })
})
