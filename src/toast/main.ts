import { mount } from 'svelte'
import '../app.css'
import Toasts from './Toasts.svelte'

const toasts = mount(Toasts, {
  target: document.getElementById('toast')!,
})

export default toasts
