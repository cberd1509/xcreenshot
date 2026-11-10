import { render, screen } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import SizeBadge from './SizeBadge.svelte'
import type { Size } from './geometry'

vi.mock('$lib/settings.svelte', () => ({ isMac: false }))

function badge(size: Size = { width: 400, height: 200 }) {
  const onresize = vi.fn<(size: Size, keepRatio: boolean) => void>()
  const view = render(SizeBadge, { props: { size, onresize } })
  return { onresize, view }
}

const width = () => screen.getByLabelText<HTMLInputElement>('Width')
const height = () => screen.getByLabelText<HTMLInputElement>('Height')
const lock = () => screen.getByRole('button', { name: 'Lock aspect ratio' })

describe('SizeBadge', () => {
  it('shows the size in its fields, following the selection', async () => {
    const { view } = badge()
    expect(screen.getByRole('group', { name: 'Selection size' })).toHaveTextContent('W')
    expect([width().value, height().value]).toEqual(['400', '200'])
    await view.rerender({ size: { width: 30, height: 20 } })
    expect([width().value, height().value]).toEqual(['30', '20'])
  })

  it('applies the typed size on Enter and leaves the field', async () => {
    const { onresize } = badge()
    await userEvent.clear(height())
    await userEvent.type(height(), '50{Enter}')
    expect(onresize).toHaveBeenCalledWith({ width: 400, height: 50 }, false)
    expect(height()).not.toHaveFocus()
  })

  it('holds the typed size while the selection changes under it', async () => {
    const { view } = badge()
    await userEvent.clear(width())
    await userEvent.type(width(), '12')
    await view.rerender({ size: { width: 30, height: 20 } })
    expect(width().value).toBe('12')
  })

  it('keeps the ratio while typing when locked', async () => {
    const { onresize } = badge()
    await userEvent.click(lock())
    expect(lock()).toHaveAttribute('aria-pressed', 'true')
    await userEvent.clear(width())
    await userEvent.type(width(), '100')
    expect(height().value).toBe('50')
    await userEvent.clear(height())
    // An empty field leaves the other side alone.
    expect(width().value).toBe('100')
    await userEvent.type(height(), '30{Enter}')
    expect(onresize).toHaveBeenCalledWith({ width: 60, height: 30 }, true)
  })

  it('applies the typed size before Copy and Save go on', async () => {
    const { onresize } = badge()
    await userEvent.clear(width())
    await userEvent.type(width(), '120')
    expect(onresize).not.toHaveBeenCalled()
    await userEvent.keyboard('{Control>}c{/Control}')
    expect(onresize).toHaveBeenCalledWith({ width: 120, height: 200 }, false)
    expect(width()).not.toHaveFocus()
  })

  it('applies it from the lock toggle, Tabbed to after an edit', async () => {
    const { onresize } = badge()
    await userEvent.clear(width())
    await userEvent.type(width(), '120')
    await userEvent.tab()
    expect(lock()).toHaveFocus()
    expect(onresize).not.toHaveBeenCalled()
    await userEvent.keyboard('{Control>}s{/Control}')
    expect(onresize).toHaveBeenCalledWith({ width: 120, height: 200 }, false)
  })

  it('applies nothing for other keys, or with nothing changed', async () => {
    const { onresize } = badge()
    await userEvent.click(width())
    await userEvent.keyboard('{Control>}s{/Control}')
    lock().focus()
    await userEvent.keyboard('{Control>}c{/Control}')
    // Other shortcuts leave the edit pending.
    await userEvent.clear(height())
    await userEvent.type(height(), '70{Control>}a{/Control}')
    expect(height()).toHaveFocus()
    expect(height().value).toBe('70')
    expect(onresize).not.toHaveBeenCalled()
    await userEvent.keyboard('{Enter}')
    expect(onresize).toHaveBeenCalledWith({ width: 400, height: 70 }, false)
  })

  it('ignores an empty size', async () => {
    const { onresize } = badge()
    await userEvent.clear(width())
    await userEvent.type(width(), '{Enter}')
    expect(onresize).not.toHaveBeenCalled()
  })

  it('cancels when focus leaves, but not for the lock', async () => {
    const { onresize } = badge()
    const outside = document.createElement('button')
    document.body.append(outside)
    await userEvent.clear(width())
    await userEvent.type(width(), '9')
    // The lock takes no focus, so the edit goes on.
    await userEvent.click(lock())
    expect(width()).toHaveFocus()
    await userEvent.click(outside)
    expect(width().value).toBe('400')
    expect(onresize).not.toHaveBeenCalled()
    outside.remove()
  })

  it('leaves the field when the edit is cancelled from outside', async () => {
    const { view } = badge()
    await userEvent.click(width())
    expect(width()).toHaveFocus()
    await view.rerender({ editing: false })
    expect(width()).not.toHaveFocus()
  })
})
