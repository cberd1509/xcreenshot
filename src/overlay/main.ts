import { mount } from 'svelte'
import '../app.css'
import Overlay from './Overlay.svelte'

const overlay = mount(Overlay, {
  target: document.getElementById('overlay')!,
})

export default overlay
