// Which section of a scrolling page the sidebar marks as current.

/** A section's vertical extent, in the scroll container's content coordinates. */
export type Span = { id: string; top: number; bottom: number }

export type View = {
  /** The visible part of the content. */
  top: number
  bottom: number
  /** Sections whose top has passed this line count as reached. */
  line: number
  /** Scrolled to the end: the last sections may never reach the line. */
  atBottom: boolean
}

/**
 * The section in view: the clicked one while any of it is visible (a short section near the
 * end can't scroll to the top), else the last reached, or the last one at the bottom.
 */
export function sectionInView(sections: Span[], view: View, clicked?: string): string | undefined {
  const pick = sections.find((s) => s.id === clicked)
  if (pick && pick.top < view.bottom && pick.bottom > view.top) return pick.id
  if (view.atBottom) return sections.at(-1)?.id
  return (sections.findLast((s) => s.top <= view.line) ?? sections[0])?.id
}
