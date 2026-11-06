import { mount } from 'svelte'
import '../app.css'
import Permission from './Permission.svelte'

const permission = mount(Permission, {
  target: document.getElementById('permission')!,
})

export default permission
