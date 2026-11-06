import './style.css'
import { animate, inView, stagger } from 'motion'
import { setUpOs } from './platform'

setUpOs(document, navigator, () => localStorage)

const ease = [0.22, 1, 0.36, 1] as const
const video = document.querySelector('video')

if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
  // No movement at all: the demo waits for the visitor to press play.
  video?.pause()
  video?.removeAttribute('autoplay')
  if (video) video.controls = true
} else {
  animate(
    '[data-enter]',
    { opacity: [0, 1], y: [8, 0] },
    { duration: 0.6, ease, delay: stagger(0.06) },
  )
  animate('[data-window]', { opacity: [0, 1], y: [16, 0] }, { duration: 0.8, ease, delay: 0.25 })

  for (const section of document.querySelectorAll<HTMLElement>('[data-reveal]')) {
    section.style.opacity = '0'
    inView(
      section,
      () => {
        animate(section, { opacity: [0, 1], y: [12, 0] }, { duration: 0.6, ease })
      },
      { amount: 0.15 },
    )
  }
}
