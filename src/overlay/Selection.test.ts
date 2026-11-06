import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import { box } from '../test/canvas'
import Selection from './Selection.svelte'
import { CURSORS, HANDLES } from './geometry'

describe('Selection', () => {
  it('places the box and follows the rect', async () => {
    const { rerender } = render(Selection, { props: { rect: { x: 1, y: 2, width: 3, height: 4 } } })
    const selection = screen.getByTestId('selection')
    expect(box(selection)).toEqual([1, 2, 3, 4])
    await rerender({ rect: { x: 5, y: 6, width: 7, height: 8 } })
    expect(box(selection)).toEqual([5, 6, 7, 8])
  })

  it('has eight handles on the edges, with matching cursors', () => {
    render(Selection, { props: { rect: { x: 0, y: 0, width: 10, height: 10 } } })
    const handles = [
      ...screen.getByTestId('selection').querySelectorAll<HTMLElement>('[data-handle]'),
    ]
    expect(handles.map((h) => h.dataset.handle)).toEqual(HANDLES)
    expect(handles.map((h) => `${h.style.left} ${h.style.top}`)).toEqual([
      '0px 0px', // nw
      '50% 0px', // n
      '100% 0px', // ne
      '100% 50%', // e
      '100% 100%', // se
      '50% 100%', // s
      '0px 100%', // sw
      '0px 50%', // w
    ])
    for (const h of handles) expect(h.style.cursor).toBe(CURSORS[h.dataset.handle as 'n'])
  })
})
