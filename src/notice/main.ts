import { mount } from 'svelte'
import '../app.css'
import Notice from './Notice.svelte'

const notice = mount(Notice, {
  target: document.getElementById('notice')!,
})

export default notice
