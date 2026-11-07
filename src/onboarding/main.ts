import { mount } from 'svelte'
import '../app.css'
import Onboarding from './Onboarding.svelte'

const onboarding = mount(Onboarding, {
  target: document.getElementById('onboarding')!,
})

export default onboarding
