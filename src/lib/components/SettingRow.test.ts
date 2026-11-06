import { render, screen } from '@testing-library/svelte'
import { createRawSnippet } from 'svelte'
import { describe, expect, it } from 'vitest'
import SettingRow from './SettingRow.svelte'

const control = createRawSnippet(() => ({ render: () => '<input id="ctl" />' }))

describe('SettingRow', () => {
  it('links the title to its control when "for" is set', () => {
    render(SettingRow, {
      title: 'Magnifier',
      description: 'Zoom in.',
      for: 'ctl',
      children: control,
    })
    expect(screen.getByLabelText('Magnifier')).toBeInTheDocument()
    expect(screen.getByText('Zoom in.')).toBeInTheDocument()
  })

  it('renders a plain title and no description otherwise', () => {
    render(SettingRow, { title: 'Take a screenshot', children: control })
    const title = screen.getByText('Take a screenshot')
    expect(title.tagName).toBe('DIV')
    expect(document.querySelector('.text-muted-foreground')).toBeNull()
  })
})
