import { createApp, h, nextTick, ref } from 'vue'
import { afterEach, expect, it } from 'vitest'
import type { Route } from '@/lib/ipc'
import ConnectionRouteBadge from './ConnectionRouteBadge.vue'

let cleanup = () => {}
afterEach(() => { cleanup() })
function mountRoute(initial: Route) {
  const route = ref<Route>(initial)
  const container = document.createElement('div')
  document.body.append(container)
  const app = createApp({ render: () => h(ConnectionRouteBadge, { route: route.value }) })
  app.mount(container)
  cleanup = () => { app.unmount(); container.remove() }
  return { container, route }
}

it('não converte rota desconhecida em conexão direta', () => {
  const { container } = mountRoute('unknown')
  expect(container.textContent).toBe('Rota não verificada')
  expect(container.textContent).not.toContain('P2P direto')
})
it('distingue relay e conexão direta a partir do valor do núcleo', async () => {
  const { container, route } = mountRoute('relay')
  expect(container.textContent).toBe('via relay')
  route.value = 'direct'
  await nextTick()
  expect(container.textContent).toBe('P2P direto')
})
