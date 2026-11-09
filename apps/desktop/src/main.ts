import { createApp } from 'vue'
import { createPinia } from 'pinia'
import '@fontsource/inter/latin-400.css'
import '@fontsource/inter/latin-500.css'
import '@fontsource/inter/latin-600.css'
import './styles/tokens.css'
import App from './App.vue'

const app = createApp(App).use(createPinia())
let failed = false
// Vue's default error reporter can include component state. Do not forward it
// to console, telemetry or a global event. Clear the view on an uncaught error.
app.config.errorHandler = () => {
  if (failed) return
  failed = true
  try { app.unmount() } catch { /* Keep the failure path free of application data. */ }
  const root = document.getElementById('app')
  if (root) root.textContent = 'Falha na interface. Feche o Veyra para descartar a sessão.'
}
app.mount('#app')
