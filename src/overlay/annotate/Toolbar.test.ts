import { render, screen } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { COLORS, DEFAULT_STYLE, type Style, type Tool } from './shapes'
import Toolbar, { type ToolbarActions } from './Toolbar.svelte'

function setup(props: Partial<{ tool: Tool | null; canUndo: boolean; mac: boolean }> = {}) {
  const actions: ToolbarActions = {
    copy: vi.fn<() => void>(),
    save: vi.fn<() => void>(),
    share: vi.fn<() => void>(),
    option: vi.fn<ToolbarActions['option']>(),
  }
  const handlers = {
    ontool: vi.fn<(t: Tool | null) => void>(),
    onundo: vi.fn<() => void>(),
    onredo: vi.fn<() => void>(),
    onclear: vi.fn<() => void>(),
  }
  let style: Style = DEFAULT_STYLE
  const view = render(Toolbar, {
    props: {
      tool: null,
      get style() {
        return style
      },
      set style(s) {
        style = s
      },
      canUndo: false,
      canRedo: true,
      canClear: true,
      options: { rememberRegion: true, magnifier: false },
      actions,
      mac: true,
      ...handlers,
      ...props,
    },
  })
  return { view, actions, handlers, style: () => style, user: userEvent.setup() }
}

const button = (name: string) => screen.getByRole('button', { name })
const item = async (name: string) =>
  (await screen.findByText(name)).closest('[role=menuitemcheckbox]')!

describe('Toolbar', () => {
  it('shows the tools in order', () => {
    setup()
    const names = screen
      .getAllByRole('button')
      .slice(0, 8)
      .map((b) => b.getAttribute('aria-label'))
    expect(names).toEqual([
      'Pen',
      'Line',
      'Arrow',
      'Rectangle',
      'Ellipse',
      'Highlighter',
      'Text',
      'Blur to hide info',
    ])
  })

  it('enables the tools that draw and shows the current one pressed', () => {
    setup({ tool: 'rect' })
    expect(button('Rectangle')).toHaveAttribute('aria-pressed', 'true')
    expect(button('Arrow')).toHaveAttribute('aria-pressed', 'false')
    expect(button('Arrow')).toBeEnabled()
    expect(button('Pen')).toBeDisabled()
    expect(button('Blur to hide info')).toBeDisabled()
  })

  it('picks a tool, and drops it when pressed again', async () => {
    const { handlers, user } = setup({ tool: 'rect' })
    await user.click(button('Arrow'))
    expect(handlers.ontool).toHaveBeenLastCalledWith('arrow')
    await user.click(button('Rectangle'))
    expect(handlers.ontool).toHaveBeenLastCalledWith(null)
  })

  it('shows the shortcut in the tooltip', async () => {
    const { user } = setup()
    await user.hover(button('Rectangle'))
    const tip = await screen.findByText('Rectangle', { selector: '[data-slot=tooltip-content]' })
    expect(tip).toHaveTextContent('R')
  })

  it('uses Ctrl in shortcuts off the Mac', async () => {
    const { user } = setup({ mac: false, canUndo: true })
    await user.hover(button('Undo'))
    expect(
      await screen.findByText('Undo', { selector: '[data-slot=tooltip-content]' }),
    ).toHaveTextContent('Ctrl+Z')
  })

  it('runs undo, redo and clear, disabled when there is nothing to do', async () => {
    const { handlers, user } = setup()
    expect(button('Undo')).toBeDisabled()
    await user.click(button('Redo'))
    await user.click(button('Clear all'))
    expect(handlers.onredo).toHaveBeenCalledOnce()
    expect(handlers.onclear).toHaveBeenCalledOnce()
    expect(handlers.onundo).not.toHaveBeenCalled()
  })

  it('picks a color and a size', async () => {
    const { style, user } = setup()
    await user.click(button('Color and size'))
    await user.click(await screen.findByLabelText(`Color ${COLORS[4]}`))
    expect(style().color).toBe(COLORS[4])
    await user.click(screen.getByLabelText('Thick'))
    expect(style()).toEqual({ color: COLORS[4], size: 8 })
    expect(screen.getByTestId('current-color').style.background).not.toBe('')
  })

  it('copies, saves and shares', async () => {
    const { actions, user } = setup()
    await user.click(button('Copy'))
    await user.click(button('Save'))
    await user.click(button('Share'))
    expect(actions.copy).toHaveBeenCalledOnce()
    expect(actions.save).toHaveBeenCalledOnce()
    expect(actions.share).toHaveBeenCalledOnce()
  })

  it('toggles options from the menu', async () => {
    const { actions, user } = setup()
    await user.click(button('More'))
    const remember = await item('Remember region')
    expect(remember).toHaveAttribute('aria-checked', 'true')
    await user.click(remember)
    expect(actions.option).toHaveBeenLastCalledWith('rememberRegion', false)
    await user.click(await item('Magnifier'))
    expect(actions.option).toHaveBeenLastCalledWith('magnifier', true)
  })

  it('keeps Enter and arrow keys from reaching the overlay', () => {
    setup()
    const seen = vi.fn<(e: KeyboardEvent) => void>()
    window.addEventListener('keydown', seen)
    const bar = screen.getByRole('toolbar')
    for (const key of ['Enter', 'ArrowLeft', ' ']) {
      bar.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
    }
    bar.dispatchEvent(new KeyboardEvent('keydown', { key: 'r', bubbles: true }))
    window.removeEventListener('keydown', seen)
    expect(seen.mock.calls.map(([e]) => e.key)).toEqual(['r'])
  })
})
