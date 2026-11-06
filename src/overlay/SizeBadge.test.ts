import { render, screen } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import SizeBadge from './SizeBadge.svelte'
import type { Size } from './geometry'

function badge(size: Size = { width: 400, height: 200 }) {
  const onresize = vi.fn<(size: Size, keepRatio: boolean) => void>()
  render(SizeBadge, { props: { size, onresize } })
  return onresize
}

const open = () => userEvent.click(screen.getByRole('button', { name: '400 × 200' }))
const width = () => screen.getByLabelText<HTMLInputElement>('Width')
const height = () => screen.getByLabelText<HTMLInputElement>('Height')

describe('SizeBadge', () => {
  it('shows the size and opens with it selected', async () => {
    badge()
    expect(screen.getByTestId('size-badge')).toHaveTextContent('400 × 200')
    await open()
    expect(width().value).toBe('400')
    expect(height().value).toBe('200')
    expect(width()).toHaveFocus()
  })

  it('applies the typed size on Enter', async () => {
    const onresize = badge()
    await open()
    await userEvent.clear(height())
    await userEvent.type(height(), '50{Enter}')
    expect(onresize).toHaveBeenCalledWith({ width: 400, height: 50 }, false)
    expect(screen.queryByLabelText('Width')).toBeNull()
  })

  it('keeps the ratio while typing when locked', async () => {
    const onresize = badge()
    await open()
    await userEvent.click(screen.getByRole('button', { name: 'Lock ratio' }))
    await userEvent.clear(width())
    await userEvent.type(width(), '100')
    expect(height().value).toBe('50')
    await userEvent.clear(height())
    // An empty field leaves the other side alone.
    expect(width().value).toBe('100')
    await userEvent.type(height(), '30{Enter}')
    expect(onresize).toHaveBeenCalledWith({ width: 60, height: 30 }, true)
  })

  it('ignores an empty size', async () => {
    const onresize = badge()
    await open()
    await userEvent.clear(width())
    await userEvent.type(width(), '{Enter}')
    expect(onresize).not.toHaveBeenCalled()
  })

  it('cancels when focus leaves', async () => {
    const onresize = badge()
    const outside = document.createElement('button')
    document.body.append(outside)
    await open()
    await userEvent.click(screen.getByRole('button', { name: 'Lock ratio' }))
    expect(width()).toBeInTheDocument()
    await userEvent.click(outside)
    expect(screen.queryByLabelText('Width')).toBeNull()
    expect(onresize).not.toHaveBeenCalled()
    outside.remove()
  })
})
