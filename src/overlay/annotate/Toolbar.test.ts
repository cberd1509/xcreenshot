import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { COLORS, DEFAULT_STYLE, type Style, type Tool } from './shapes'
import type { ShownTextOptions, TextOptions } from './text'
import Toolbar, { type ToolbarActions } from './Toolbar.svelte'

type Props = Partial<{
  tool: Tool | null
  canUndo: boolean
  mac: boolean
  canShare: boolean
  drawable: boolean
  text: ShownTextOptions | null
  ontext: (t: Partial<TextOptions>) => void
  keepFocus: boolean
}>

function setup(props: Props = {}) {
  const actions: ToolbarActions = {
    copy: vi.fn<() => void>(),
    save: vi.fn<() => void>(),
    share: vi.fn<ToolbarActions['share']>(),
    cancel: vi.fn<() => void>(),
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
      selection: { x: 100, y: 100, width: 400, height: 300 },
      display: { width: 1000, height: 700 },
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
      canShare: true,
      ...handlers,
      ...props,
    },
  })
  return { view, actions, handlers, style: () => style, user: userEvent.setup() }
}

/** Where a bar is, by name. */
const barStyle = (name: string) => screen.getByRole('toolbar', { name }).parentElement!.style
const button = (name: string) => screen.getByRole('button', { name })
/** A tool, color or size: one choice of a toggle group. */
const choice = (name: string) => screen.getByLabelText(name)
const item = async (name: string) =>
  (await screen.findByText(name)).closest('[role=menuitemcheckbox]')!
const tooltip = (name: string) =>
  screen.findByText(name, { selector: '[data-slot=tooltip-content]' })

/** Presses `target`; returns whether the press's default (focusing) was prevented. */
function mousedown(target: Element) {
  const e = new MouseEvent('mousedown', { bubbles: true, cancelable: true })
  target.dispatchEvent(e)
  return e.defaultPrevented
}

describe('Toolbar', () => {
  it('shows the tools in order, Select first', () => {
    setup()
    const names = [...screen.getByLabelText('Tools').children].map((b) =>
      b.getAttribute('aria-label'),
    )
    expect(names).toEqual([
      'Select',
      'Pen',
      'Line',
      'Arrow',
      'Rectangle',
      'Ellipse',
      'Highlighter',
      'Text',
      'Blur',
    ])
  })

  it('shows Select on without a drawing tool, and picks it to drop the tool', async () => {
    const { handlers, user, view } = setup()
    expect(choice('Select')).toHaveAttribute('data-state', 'on')
    await view.rerender({ tool: 'pen' })
    expect(choice('Select')).toHaveAttribute('data-state', 'off')
    await user.click(choice('Select'))
    expect(handlers.ontool).toHaveBeenLastCalledWith(null)
    // Pressing Select again keeps it.
    await view.rerender({ tool: null })
    await user.click(choice('Select'))
    expect(handlers.ontool).toHaveBeenLastCalledWith(null)
  })

  it('puts the rail right of the selection and the action bar below it', () => {
    setup()
    // jsdom lays nothing out, so the bars have no size.
    expect(barStyle('Annotation tools')).toMatchObject({ left: '512px', top: '100px' })
    expect(barStyle('Capture actions')).toMatchObject({ left: '500px', top: '412px' })
  })

  it('enables the tools that draw and shows the current one on', () => {
    setup({ tool: 'rect' })
    expect(choice('Rectangle')).toHaveAttribute('data-state', 'on')
    expect(choice('Arrow')).toHaveAttribute('data-state', 'off')
    expect(choice('Arrow')).toBeEnabled()
    expect(choice('Pen')).toBeEnabled()
    expect(choice('Blur')).toBeEnabled()
    expect(choice('Text')).toBeEnabled()
  })

  it('shows every tool as unavailable when drawing could not load', () => {
    setup({ drawable: false })
    expect(choice('Rectangle')).toBeDisabled()
    expect(choice('Arrow')).toBeDisabled()
    expect(choice('Select')).toBeEnabled()
  })

  it('picks a tool, and drops it when pressed again', async () => {
    const { handlers, user, view } = setup()
    await user.click(choice('Arrow'))
    expect(handlers.ontool).toHaveBeenLastCalledWith('arrow')
    await view.rerender({ tool: 'arrow' })
    await user.click(choice('Arrow'))
    expect(handlers.ontool).toHaveBeenLastCalledWith(null)
  })

  it('shows each tool and its key in the tooltip', async () => {
    const { user } = setup()
    /** The key in the tooltip of the tool `name`, shown as `label`. */
    const key = async (name: string, label: string) => {
      await user.hover(choice(name))
      const shown = (await tooltip(label)).querySelector('kbd')?.textContent
      await user.unhover(choice(name))
      return shown
    }
    expect(await key('Select', 'Select and move')).toBe('V')
    expect(await key('Rectangle', 'Rectangle')).toBe('R')
    expect(await key('Ellipse', 'Ellipse')).toBe('O')
    expect(await key('Blur', 'Blur to hide info')).toBe('B')
  })

  it('shows the keys of the other buttons in their tooltips', async () => {
    const { user } = setup({ canUndo: true })
    await user.hover(button('Undo'))
    expect((await tooltip('Undo')).querySelector('kbd')).toHaveTextContent('⌘Z')
    await user.hover(button('Redo'))
    expect((await tooltip('Redo')).querySelector('kbd')).toHaveTextContent('⇧⌘Z')
    await user.hover(button('Save'))
    expect((await tooltip('Save')).querySelector('kbd')).toHaveTextContent('⌘S')
    await user.hover(button('Cancel'))
    expect((await tooltip('Cancel')).querySelector('kbd')).toHaveTextContent('Esc')
    await user.hover(button('Clear all'))
    expect((await tooltip('Clear all')).querySelector('kbd')).toBeNull()
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
    const swatches = [...(await screen.findByLabelText('Color')).children]
    expect(swatches.map((s) => s.getAttribute('aria-label'))).toEqual(COLORS.map((c) => c.name))
    await user.click(choice('Blue'))
    expect(style().color).toBe('#3B82F6')
    await user.click(choice('Thick'))
    expect(style()).toEqual({ color: '#3B82F6', size: 8 })
    // Pressing the current one again does not leave none.
    await user.click(choice('Thick'))
    await user.click(choice('Blue'))
    expect(style()).toEqual({ color: '#3B82F6', size: 8 })
    expect(screen.getByTestId('current-color').style.background).not.toBe('')
  })

  it('picks a color by hex, as #RGB or #RRGGBB', async () => {
    const { style, user } = setup()
    await user.click(button('Color and size'))
    const hex = await screen.findByLabelText<HTMLInputElement>('Hex')
    expect(hex.value).toBe(DEFAULT_STYLE.color)
    // Typing changes nothing until Enter, so a color is one change.
    await user.clear(hex)
    await user.type(hex, '#0f0')
    expect(hex.value).toBe('#0f0')
    expect(style()).toEqual(DEFAULT_STYLE)
    await user.keyboard('{Enter}')
    expect(style()).toEqual({ ...DEFAULT_STYLE, color: '#00FF00' })
    expect(hex.value).toBe('#00FF00')
    // Something that is not a color is dropped.
    await user.clear(hex)
    await user.type(hex, '#0f08{Enter}')
    expect(style().color).toBe('#00FF00')
    expect(hex.value).toBe('#00FF00')
    // Leaving the field takes the color too.
    await user.clear(hex)
    await user.type(hex, '123456')
    await user.tab()
    expect(style().color).toBe('#123456')
  })

  it('drops a hex color still being typed when a swatch is picked', async () => {
    const { style, user } = setup()
    await user.click(button('Color and size'))
    const hex = await screen.findByLabelText<HTMLInputElement>('Hex')
    await user.clear(hex)
    await user.type(hex, '123456')
    await fireEvent.click(choice('Blue'))
    expect(hex.value).toBe('#3B82F6')
    hex.blur()
    expect(style().color).toBe('#3B82F6')
  })

  it('copies, saves and cancels', async () => {
    const { actions, user } = setup()
    await user.click(button('Copy'))
    await user.click(button('Save'))
    await user.click(button('Cancel'))
    expect(actions.copy).toHaveBeenCalledOnce()
    expect(actions.save).toHaveBeenCalledOnce()
    expect(actions.cancel).toHaveBeenCalledOnce()
  })

  it('shares under the Share button, between ⋯ and Save', async () => {
    const { actions, user } = setup()
    const share = button('Share')
    const names = [...screen.getByRole('toolbar', { name: 'Capture actions' }).children]
      .map((b) => b.getAttribute('aria-label') ?? b.textContent?.trim())
      .filter(Boolean)
    expect(names).toEqual(['More', 'Share', 'Save', 'Copy', 'Cancel'])
    // Made up at run time, so only the button's real rect can match it.
    const at = { x: Math.random() * 500, y: Math.random() * 500, width: 80, height: 36 }
    vi.spyOn(share, 'getBoundingClientRect').mockReturnValue(DOMRect.fromRect(at) as DOMRect)
    await user.hover(share)
    expect(await tooltip('Share')).toBeInTheDocument()
    await user.click(share)
    expect(actions.share).toHaveBeenCalledExactlyOnceWith(at)
  })

  it('has no Share button without a share sheet', () => {
    setup({ canShare: false })
    expect(button('Save')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Share' })).toBeNull()
  })

  it('shows the platform key on Copy', async () => {
    setup({ mac: false })
    expect(button('Copy')).toHaveTextContent('Ctrl+C')
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

  it('shows text options only while asked to, and changes them', async () => {
    const ontext = vi.fn<(t: Partial<TextOptions>) => void>()
    const { user, view } = setup()
    expect(screen.queryByLabelText('Text size')).not.toBeInTheDocument()
    await view.rerender({ text: { size: 'M', background: false }, ontext })
    expect(choice('Medium text')).toHaveAttribute('data-state', 'on')
    // Pressing the current size again keeps it.
    await user.click(choice('Medium text'))
    expect(ontext).not.toHaveBeenCalled()
    await user.click(choice('Large text'))
    expect(ontext).toHaveBeenLastCalledWith({ size: 'L' })
    await user.click(button('Background'))
    expect(ontext).toHaveBeenLastCalledWith({ background: true })
  })

  it('shows no size for resized text', () => {
    setup({ text: { size: null, background: false } })
    expect(choice('Medium text')).toHaveAttribute('data-state', 'off')
    expect(choice('Large text')).toHaveAttribute('data-state', 'off')
  })

  it('keeps focus where it was while asked to, also in the color menu', async () => {
    const field = document.createElement('textarea')
    document.body.append(field)
    const { user, view } = setup({ text: { size: 'M', background: false } })
    expect(mousedown(button('Background'))).toBe(false)
    await view.rerender({ keepFocus: true })
    expect(mousedown(button('Background'))).toBe(true)
    expect(mousedown(button('Undo'))).toBe(true)
    field.focus()
    await user.click(button('Color and size'))
    const swatch = await screen.findByLabelText('Blue')
    expect(field).toHaveFocus()
    expect(mousedown(swatch)).toBe(true)
    const bar = view.component as unknown as { closeMenus: () => boolean }
    bar.closeMenus()
    await waitFor(() => expect(screen.queryByLabelText('Blue')).toBeNull())
    expect(field).toHaveFocus()
    // The ⋯ menu too.
    await user.click(button('More'))
    expect(mousedown(await item('Magnifier'))).toBe(true)
    expect(field).toHaveFocus()
    field.remove()
  })

  it('has default text handlers', async () => {
    const { user } = setup({ text: { size: 'S', background: true } })
    await user.click(choice('Large text'))
    expect(button('Background')).toHaveAttribute('aria-pressed', 'true')
  })
})
