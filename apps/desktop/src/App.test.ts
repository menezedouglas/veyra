import { createApp, nextTick, type App as VueApp } from 'vue'
import { createPinia } from 'pinia'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import type { PendingMessage, SessionStatus } from './lib/ipc'
const native = vi.hoisted(() => ({
  isDesktop: () => true,
  status: vi.fn(), startSession: vi.fn(), endSession: vi.fn(), listPending: vi.fn(),
}))
vi.mock('./lib/ipc', () => ({ api: native }))
import App from './App.vue'

const inactive: SessionStatus = {
  phase: 'inactive', runtimeId: null, fingerprint: null,
  infrastructureConfigured: false, reason: 'Infraestrutura não configurada.',
  peers: [], pendingCount: 0, memoryProtection: 'not-checked',
}
let app: VueApp | undefined
let container: HTMLDivElement
async function settle() { await Promise.resolve(); await nextTick(); await Promise.resolve(); await nextTick() }
async function mount() {
  container = document.createElement('div')
  document.body.append(container)
  app = createApp(App).use(createPinia())
  app.mount(container)
  await settle()
}
function button(label: string): HTMLButtonElement {
  const found = [...container.querySelectorAll('button')].find(node => node.textContent?.includes(label))
  if (!found) throw new Error(`Missing button: ${label}`)
  return found
}
beforeEach(() => {
  native.status.mockResolvedValue(inactive)
  native.endSession.mockResolvedValue(undefined)
  native.listPending.mockResolvedValue([])
  native.startSession.mockReset()
})
afterEach(() => { app?.unmount(); container?.remove(); vi.useRealTimers(); vi.clearAllMocks() })

it('exibe somente a identidade fornecida pelo núcleo e a descarta ao encerrar', async () => {
  await mount()
  expect(container.textContent).not.toContain('RUNTIME-SINTETICO')
  native.startSession.mockResolvedValue({ ...inactive, phase: 'active', runtimeId: 'RUNTIME-SINTETICO', fingerprint: 'FINGERPRINT-SINTETICO', memoryProtection: 'protected' })
  button('Iniciar sessão').click()
  await settle()
  expect(native.startSession).toHaveBeenCalledWith(false, undefined)
  button('Configurações').click()
  await settle()
  expect(container.textContent).toContain('RUNTIME-SINTETICO')
  button('Sair e descartar sessão').click()
  await settle()
  expect(native.endSession).toHaveBeenCalledTimes(1)
  expect(container.textContent).not.toContain('RUNTIME-SINTETICO')
  expect(container.textContent).not.toContain('FINGERPRINT-SINTETICO')
})
it('não simula início de sessão quando o núcleo recusa', async () => {
  native.startSession.mockRejectedValue(new Error('Falha sintética'))
  await mount()
  button('Iniciar sessão').click()
  await settle()
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Não foi possível iniciar')
  expect(container.textContent).not.toContain('Identidade ativa nesta execução')
})

it('não adiciona a latência IPC à validade das pendências', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date', 'performance'] })
  let release!: (value: PendingMessage[]) => void
  native.status.mockResolvedValue({ ...inactive, phase: 'active', runtimeId: 'RUNTIME-SINTETICO', memoryProtection: 'protected' })
  native.listPending.mockReturnValue(new Promise<PendingMessage[]>(resolve => { release = resolve }))
  await mount()
  expect(native.listPending).toHaveBeenCalledTimes(1)
  vi.advanceTimersByTime(2500)
  release([{ id: 'SYNTHETIC-MESSAGE', peerId: 'SYNTHETIC-PEER', expiresInMs: 1000 }])
  await settle()
  expect(container.querySelector('article')).toBeNull()
  expect(container.textContent).toContain('Nenhuma mensagem pendente')
})
