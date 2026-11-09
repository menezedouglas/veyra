import { createApp, h, nextTick, type App } from 'vue'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
const native = vi.hoisted(() => ({ openOnce: vi.fn() }))
vi.mock('@/lib/ipc', () => ({ api: native }))
import OneTimeViewer from './OneTimeViewer.vue'

let app: App | undefined
let container: HTMLDivElement
let closed = vi.fn<() => void>()
async function settle() {
  for (let i = 0; i < 6; i++) { await Promise.resolve(); await nextTick() }
}
async function mount(expiresIn = 300000) {
  container = document.createElement('div'); document.body.append(container)
  closed = vi.fn<() => void>()
  app = createApp({ render: () => h(OneTimeViewer, { messageId: 'synthetic-id', expiresAt: performance.now() + expiresIn, onClose: closed }) })
  app.mount(container); await settle()
}
function confirmButton() {
  const button = [...document.querySelectorAll('button')].find(node => node.textContent?.includes('Consumir e abrir'))
  if (!button) throw new Error('Missing local confirmation')
  return button
}
beforeEach(() => { native.openOnce.mockReset(); vi.spyOn(document, 'hasFocus').mockReturnValue(true) })
afterEach(async () => { app?.unmount(); container?.remove(); await settle(); vi.useRealTimers(); vi.restoreAllMocks() })

it('não pede o corpo antes da confirmação, invoca uma vez e renderiza texto escapado', async () => {
  const text = '<img src=x onerror="globalThis.syntheticXss=true"> texto sintético'
  native.openOnce.mockResolvedValue({ body: text, displayForMs: 30000 })
  await mount()
  expect(native.openOnce).not.toHaveBeenCalled()
  const button = confirmButton()
  button.click(); button.click()
  await settle()
  expect(native.openOnce).toHaveBeenCalledTimes(1)
  expect(document.querySelector('pre')?.textContent).toBe(text)
  expect(document.querySelector('pre img')).toBeNull()
  window.dispatchEvent(new Event('blur')); await settle()
  expect(document.querySelector('pre')).toBeNull()
  window.dispatchEvent(new Event('focus')); await settle()
  expect(document.querySelector('pre')).toBeNull()
  expect(closed).toHaveBeenCalled()
})
it('descarta uma resposta que chega depois da perda de foco', async () => {
  let release!: (value: { body: string; displayForMs: number }) => void
  native.openOnce.mockReturnValue(new Promise(resolve => { release = resolve }))
  await mount()
  confirmButton().click(); await settle()
  window.dispatchEvent(new Event('blur'))
  release({ body: 'SINTETICO-NAO-EXIBIR', displayForMs: 30000 })
  await settle()
  expect(document.body.textContent).not.toContain('SINTETICO-NAO-EXIBIR')
  expect(closed).toHaveBeenCalled()
})
it('limita a exibição ao prazo original da mensagem', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date', 'performance'] })
  native.openOnce.mockResolvedValue({ body: 'SINTETICO-CURTO', displayForMs: 30000 })
  await mount(2000)
  confirmButton().click(); await settle()
  expect(document.querySelector('pre')?.textContent).toBe('SINTETICO-CURTO')
  vi.advanceTimersByTime(2100); await settle()
  expect(document.querySelector('pre')).toBeNull()
  expect(closed).toHaveBeenCalled()
})

it('não adiciona a latência IPC ao orçamento de exibição do cofre', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date', 'performance'] })
  let release!: (value: { body: string; displayForMs: number }) => void
  native.openOnce.mockReturnValue(new Promise(resolve => { release = resolve }))
  await mount(60000)
  confirmButton().click(); await settle()
  vi.advanceTimersByTime(2500)
  release({ body: 'SINTETICO-ORCAMENTO-EXPIRADO', displayForMs: 1000 })
  await settle()
  expect(document.querySelector('pre')).toBeNull()
  expect(document.body.textContent).not.toContain('SINTETICO-ORCAMENTO-EXPIRADO')
  expect(closed).toHaveBeenCalled()
})

it.each([
  { label: 'avanço de 20 minutos durante suspensão', wallChangeMs: 20 * 60 * 1000 },
  { label: 'relógio civil retrocedendo cinco segundos', wallChangeMs: -5000 },
])('descarta o corpo com performance parado e $label', async ({ wallChangeMs }) => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
  const monotonic = 1000
  let wall = Date.now()
  vi.spyOn(performance, 'now').mockReturnValue(monotonic)
  vi.spyOn(Date, 'now').mockImplementation(() => wall)
  native.openOnce.mockResolvedValue({ body: 'SINTETICO-RELOGIO-INCERTO', displayForMs: 30000 })
  await mount(60000)
  confirmButton().click(); await settle()
  expect(document.querySelector('pre')?.textContent).toBe('SINTETICO-RELOGIO-INCERTO')
  wall += wallChangeMs
  vi.advanceTimersByTime(100); await settle()
  expect(performance.now()).toBe(monotonic)
  expect(document.querySelector('pre')).toBeNull()
  expect(document.body.textContent).not.toContain('SINTETICO-RELOGIO-INCERTO')
  expect(closed).toHaveBeenCalled()
})
