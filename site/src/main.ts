import './style.css'
import { inView } from 'motion'
import { animate } from 'motion/mini'
import { setUpOs } from './platform'

setUpOs(document, navigator, () => localStorage)

const video = document.querySelector('video')

if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
  // No movement at all: the demo waits for the visitor to press play.
  if (video) {
    video.pause()
    video.removeAttribute('autoplay')
    video.controls = true
  }
} else {
  // The hero enters in CSS. Sections below the fold rise in as they scroll into view; any already
  // on screen stay as they are, so nothing blinks out.
  for (const section of document.querySelectorAll<HTMLElement>('[data-reveal]')) {
    if (section.getBoundingClientRect().top < innerHeight) continue
    section.style.opacity = '0'
    inView(
      section,
      () => {
        animate(
          section,
          { opacity: [0, 1], transform: ['translateY(12px)', 'none'] },
          { duration: 0.6, ease: [0.22, 1, 0.36, 1] },
        )
      },
      { amount: 0.15 },
    )
  }
}
