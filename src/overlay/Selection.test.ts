import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import Selection from './Selection.svelte'
import { CURSORS, HANDLES } from './geometry'

describe('Selection', () => {
  it('places the box and follows the rect', async () => {
    const { rerender } = render(Selection, { props: { rect: { x: 1, y: 2, width: 3, height: 4 } } })
    const box = screen.getByTestId('selection')
    expect([box.style.left, box.style.top, box.style.width, box.style.height]).toEqual([
      '1px',
      '2px',
      '3px',
      '4px',
    ])
    await rerender({ rect: { x: 5, y: 6, width: 7, height: 8 } })
    expect([box.style.left, box.style.top, box.style.width, box.style.height]).toEqual([
      '5px',
      '6px',
      '7px',
      '8px',
    ])
  })

  it('has eight handles with matching cursors', () => {
    render(Selection, { props: { rect: { x: 0, y: 0, width: 10, height: 10 } } })
    const handles = [
      ...screen.getByTestId('selection').querySelectorAll<HTMLElement>('[data-handle]'),
    ]
    expect(handles.map((h) => h.dataset.handle)).toEqual(HANDLES)
    for (const h of handles) expect(h.style.cursor).toBe(CURSORS[h.dataset.handle as 'n'])
  })
})
