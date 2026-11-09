import { createApp, h, nextTick, shallowRef, type App } from 'vue'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import type { Delivery, PeerSummary } from '@/lib/ipc'
const native = vi.hoisted(() => ({ sendMessage: vi.fn() }))
vi.mock('@/lib/ipc', () => ({ api: native }))
import MessageComposer from './MessageComposer.vue'

const peers: PeerSummary[] = [
  { id: 'verified', label: 'Fixture verificada', fingerprint: 'SYNTHETIC-FP-1', verified: true, route: 'direct' },
  { id: 'unverified', label: 'Fixture não verificada', fingerprint: 'SYNTHETIC-FP-2', verified: false, route: 'unknown' },
]
const incoming = shallowRef<Delivery[]>([])
let app: App | undefined
let container: HTMLDivElement
async function settle() { for (let i = 0; i < 5; i++) { await Promise.resolve(); await nextTick() } }
async function mount(initialPeerId = 'verified') {
  container = document.createElement('div'); document.body.append(container)
  app = createApp({ render: () => h(MessageComposer, { peers, initialPeerId, deliveryUpdates: incoming.value }) })
  app.mount(container); await settle()
}
async function type(text: string) {
  const textarea = document.querySelector('textarea')!
  textarea.value = text
  textarea.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
  return textarea
}
function submit() { document.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })) }
beforeEach(() => { native.sendMessage.mockReset(); incoming.value = [] })
afterEach(async () => { app?.unmount(); container?.remove(); await settle() })

it('limpa o rascunho imediatamente na transferência IPC, inclusive após recusa', async () => {
  let reject!: (reason: Error) => void
  native.sendMessage.mockReturnValue(new Promise<Delivery[]>((_, no) => { reject = no }))
  await mount()
  const textarea = await type('SINTETICO-SOMENTE-COMPOSITOR')
  submit(); await settle()
  expect(native.sendMessage).toHaveBeenCalledWith(['verified'], 'SINTETICO-SOMENTE-COMPOSITOR')
  expect(textarea.value).toBe('')
  reject(new Error('Synthetic rejection')); await settle()
  expect(textarea.value).toBe('')
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('O texto foi descartado')
})
it('recusa destinatário não verificado antes da chamada nativa', async () => {
  await mount('unverified')
  await type('SINTETICO')
  submit(); await settle()
  expect(native.sendMessage).not.toHaveBeenCalled()
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('identidades verificadas')
})
it('aplica o limite UTF-8 antes do IPC', async () => {
  await mount()
  await type('á'.repeat(2049))
  submit(); await settle()
  expect(native.sendMessage).not.toHaveBeenCalled()
})

it('correlaciona confirmação por messageId e peerId, sem atribuir ACK antigo ao novo envio', async () => {
  native.sendMessage.mockResolvedValue([{ messageId: 'current-envelope', peerId: 'verified', state: 'sent' }])
  await mount(); await type('SINTETICO'); submit(); await settle()
  incoming.value = [{ messageId: 'previous-envelope', peerId: 'verified', state: 'acknowledged' }]
  await settle()
  expect(document.querySelector('[aria-label="Resultado do envio"]')?.textContent).not.toContain('Recebida em RAM')
  incoming.value = [{ messageId: 'current-envelope', peerId: 'verified', state: 'acknowledged' }]
  await settle()
  expect(document.querySelector('[aria-label="Resultado do envio"]')?.textContent).toContain('Recebida em RAM')
})
it('preserva confirmação que chega antes da resposta local do envio', async () => {
  let release!: (value: Delivery[]) => void
  native.sendMessage.mockReturnValue(new Promise<Delivery[]>(resolve => { release = resolve }))
  await mount(); await type('SINTETICO'); submit(); await settle()
  incoming.value = [{ messageId: 'racing-envelope', peerId: 'verified', state: 'acknowledged' }]
  await settle()
  release([{ messageId: 'racing-envelope', peerId: 'verified', state: 'sent' }]); await settle()
  expect(document.querySelector('[aria-label="Resultado do envio"]')?.textContent).toContain('Recebida em RAM')
})
