import { render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { COLORS, DEFAULT_STYLE, type Style, type Tool } from './shapes'
import Toolbar, { type ToolbarActions } from './Toolbar.svelte'

type Props = Partial<{ tool: Tool | null; canUndo: boolean; mac: boolean; drawable: boolean }>

function setup(props: Props = {}) {
  const actions: ToolbarActions = {
    copy: vi.fn<() => void>(),
    save: vi.fn<() => void>(),
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
/** A tool, color or size: one choice of a toggle group. */
const choice = (name: string) => screen.getByLabelText(name)
const item = async (name: string) =>
  (await screen.findByText(name)).closest('[role=menuitemcheckbox]')!
const tooltip = (name: string) =>
  screen.findByText(name, { selector: '[data-slot=tooltip-content]' })

describe('Toolbar', () => {
  it('shows the tools in order', () => {
    setup()
    const names = [...screen.getByLabelText('Tools').children].map((b) =>
      b.getAttribute('aria-label'),
    )
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

  it('enables the tools that draw and shows the current one on', () => {
    setup({ tool: 'rect' })
    expect(choice('Rectangle')).toHaveAttribute('data-state', 'on')
    expect(choice('Arrow')).toHaveAttribute('data-state', 'off')
    expect(choice('Arrow')).toBeEnabled()
    expect(choice('Pen')).toBeDisabled()
    expect(choice('Blur to hide info')).toBeDisabled()
  })

  it('shows every tool as unavailable when drawing could not load', () => {
    setup({ drawable: false })
    expect(choice('Rectangle')).toBeDisabled()
    expect(choice('Arrow')).toBeDisabled()
  })

  it('picks a tool, and drops it when pressed again', async () => {
    const { handlers, user, view } = setup()
    await user.click(choice('Arrow'))
    expect(handlers.ontool).toHaveBeenLastCalledWith('arrow')
    await view.rerender({ tool: 'arrow' })
    await user.click(choice('Arrow'))
    expect(handlers.ontool).toHaveBeenLastCalledWith(null)
  })

  it('shows the shortcut in the tooltip', async () => {
    const { user } = setup()
    await user.hover(choice('Rectangle'))
    expect(await tooltip('Rectangle')).toHaveTextContent('R')
  })

  it('uses Ctrl in shortcuts off the Mac', async () => {
    const { user } = setup({ mac: false, canUndo: true })
    await user.hover(button('Undo'))
    expect(await tooltip('Undo')).toHaveTextContent('Ctrl+Z')
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

  it('picks a color and a size, and keeps one of each', async () => {
    const { style, user } = setup()
    await user.click(button('Color and size'))
    await user.click(await screen.findByLabelText(`Color ${COLORS[4]}`))
    expect(style().color).toBe(COLORS[4])
    await user.click(choice('Thick'))
    expect(style()).toEqual({ color: COLORS[4], size: 8 })
    // Pressing the current one again does not leave none.
    await user.click(choice('Thick'))
    await user.click(choice(`Color ${COLORS[4]}`))
    expect(style()).toEqual({ color: COLORS[4], size: 8 })
    expect(screen.getByTestId('current-color').style.background).not.toBe('')
  })

  it('copies and saves; Share waits for #19', async () => {
    const { actions, user } = setup()
    await user.click(button('Copy'))
    await user.click(button('Save'))
    expect(actions.copy).toHaveBeenCalledOnce()
    expect(actions.save).toHaveBeenCalledOnce()
    expect(button('Share')).toBeDisabled()
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

  it('closes an open menu for Esc, and says whether one was open', async () => {
    const { user, view } = setup()
    const bar = view.component as unknown as {
      closeMenus: () => boolean
      menuOpen: () => boolean
    }
    expect(bar.closeMenus()).toBe(false)
    expect(bar.menuOpen()).toBe(false)
    await user.click(button('More'))
    await item('Magnifier')
    expect(bar.menuOpen()).toBe(true)
    expect(bar.closeMenus()).toBe(true)
    expect(bar.menuOpen()).toBe(false)
    await waitFor(() => expect(screen.queryByText('Magnifier')).not.toBeInTheDocument())
    await user.click(button('Color and size'))
    await screen.findByLabelText('Thick')
    expect(bar.closeMenus()).toBe(true)
  })
})
