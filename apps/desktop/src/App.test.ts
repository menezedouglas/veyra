import { createApp, nextTick, type App as VueApp } from 'vue'
import { createPinia } from 'pinia'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import type { Invite, PeerSummary, PendingMessage, SessionStatus } from './lib/ipc'
const native = vi.hoisted(() => ({
  isDesktop: () => true,
  status: vi.fn(), startSession: vi.fn(), endSession: vi.fn(), listPending: vi.fn(), joinInvite: vi.fn(), createInvite: vi.fn(), revokePeer: vi.fn(),
}))
vi.mock('./lib/ipc', () => ({ api: native }))
import App from './App.vue'

const inactive: SessionStatus = {
  phase: 'inactive', runtimeId: null, fingerprint: null,
  infrastructureConfigured: false, reason: 'Conexão direta sem servidor.',
  connectionSetup: { configured: true, source: 'direct', reason: 'Conexão direta sem servidor. Redes com NAT restritivo podem impedir a conexão.' },
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
  native.status.mockReset().mockResolvedValue(inactive)
  native.endSession.mockResolvedValue(undefined)
  native.listPending.mockResolvedValue([])
  native.startSession.mockReset()
  native.joinInvite.mockReset()
  native.createInvite.mockReset()
  native.revokePeer.mockReset().mockResolvedValue(undefined)
})
afterEach(() => { app?.unmount(); container?.remove(); vi.useRealTimers(); vi.clearAllMocks() })

const savedContact: PeerSummary = {
  id: 'CONTATO-SALVO', label: 'Contato sincronizado', fingerprint: 'FINGERPRINT-SALVO',
  verified: false, route: 'disconnected', verificationCode: null,
}

it('exibe contatos restaurados sem iniciar uma sessão e oferece novo pareamento', async () => {
  native.status.mockResolvedValue({ ...inactive, peers: [savedContact] })
  await mount()
  button('Contatos').click()
  await settle()
  expect(container.querySelector('h1')?.textContent).toBe('Contatos')
  expect(container.textContent).toContain('Contato sincronizado')
  expect(container.textContent).toContain('FINGERPRINT-SALVO')
  expect(container.textContent).toContain('Contato salvo e desconectado')
  expect(container.textContent).not.toContain('Identidade verificada')
  expect(container.textContent).not.toContain('Verificar identidade')
  expect(native.startSession).not.toHaveBeenCalled()
  expect(native.listPending).not.toHaveBeenCalled()
  button('Reconectar por convite').click()
  await settle()
  expect(container.querySelector('h1')?.textContent).toBe('Convites')
  expect(container.querySelector<HTMLTextAreaElement>('#join-invitation')?.disabled).toBe(false)
})

it('informa uma falha ao carregar os contatos mesmo sem sessão ativa', async () => {
  native.status.mockResolvedValue({ ...inactive, contactsError: 'Não foi possível carregar os contatos salvos.' })
  await mount()
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Não foi possível carregar os contatos salvos.')
  button('Contatos').click()
  await settle()
  expect(container.querySelector('[role="alert"]')?.textContent).toContain('Não foi possível carregar os contatos salvos.')
})

it('preserva os contatos devolvidos pelo núcleo ao encerrar a sessão e reabrir a interface', async () => {
  native.status.mockResolvedValue({
    ...inactive, phase: 'active', runtimeId: 'RUNTIME-SINTETICO', memoryProtection: 'protected',
    peers: [{ ...savedContact, route: 'direct', verified: true, verificationCode: 'ABCD-EF01-2345-6789' }],
  })
  await mount()
  native.status.mockResolvedValue({ ...inactive, peers: [savedContact] })
  button('Sair e descartar sessão').click()
  await settle()
  button('Contatos').click()
  await settle()
  expect(native.endSession).toHaveBeenCalledTimes(1)
  expect(container.textContent).toContain('Contato sincronizado')
  expect(container.textContent).toContain('Contato salvo e desconectado')
  expect(container.textContent).not.toContain('RUNTIME-SINTETICO')
  app!.unmount()
  container.remove()
  await mount()
  button('Contatos').click()
  await settle()
  expect(container.textContent).toContain('Contato sincronizado')
  expect(container.textContent).not.toContain('Identidade verificada')
})

it('remove um contato salvo sem sessão somente após confirmação do núcleo', async () => {
  native.status.mockResolvedValue({ ...inactive, peers: [savedContact] })
  let finishRemoval!: () => void
  native.revokePeer.mockReturnValue(new Promise<void>(resolve => { finishRemoval = resolve }))
  await mount()
  button('Contatos').click()
  await settle()
  button('Remover').click()
  await settle()
  const dialog = document.querySelector('[role="dialog"]')!
  expect(dialog.textContent).toContain('Remover contato salvo?')
  expect(dialog.textContent).toContain('removido permanentemente')
  const confirm = [...dialog.querySelectorAll('button')].find(node => node.textContent?.includes('Remover e descartar'))!
  confirm.click()
  await settle()
  expect(native.revokePeer).toHaveBeenCalledWith(savedContact.id)
  expect(container.textContent).toContain('Contato sincronizado')
  native.status.mockResolvedValue(inactive)
  finishRemoval()
  for (let i = 0; i < 5; i++) await settle()
  expect(container.textContent).not.toContain('Contato sincronizado')
  expect(container.textContent).toContain('Nenhum contato salvo')
  expect(native.startSession).not.toHaveBeenCalled()
})

it('mantém a confirmação de remoção offline aberta durante o polling enquanto o contato existir', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date', 'performance'] })
  native.status.mockResolvedValue({ ...inactive, peers: [savedContact] })
  await mount()
  button('Contatos').click()
  await settle()
  button('Remover').click()
  await settle()
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain('Remover contato salvo?')
  await vi.advanceTimersByTimeAsync(2200)
  expect(native.status.mock.calls.length).toBeGreaterThanOrEqual(3)
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain('Contato sincronizado')
  expect(native.revokePeer).not.toHaveBeenCalled()
  native.status.mockResolvedValue(inactive)
  await vi.advanceTimersByTimeAsync(1200)
  expect(document.querySelector('[role="dialog"]')).toBeNull()
})

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

it('usa a configuração do aplicativo sem pedir um endpoint no fluxo principal', async () => {
  native.status.mockResolvedValue({ ...inactive, connectionSetup: { configured: true, source: 'application' } })
  native.startSession.mockResolvedValue({ ...inactive, phase: 'active', runtimeId: 'RUNTIME-SINTETICO', infrastructureConfigured: true, connectionSetup: { configured: true, source: 'application' }, memoryProtection: 'protected' })
  await mount()
  expect(container.querySelector('#signaling-endpoint')).toBeNull()
  expect(container.textContent).not.toContain('Configurar operador')
  button('Conexões').click()
  await settle()
  const configuration = [...container.querySelectorAll('dt')].find(node => node.textContent === 'Configuração de conexão')!
  expect(configuration.nextElementSibling?.textContent).toBe('Configurada')
  expect(container.textContent).toContain('Nenhum transporte estabelecido.')
  button('Convites').click()
  await settle()
  expect(button('Criar convite').disabled).toBe(true)
  button('Mensagens pendentes').click()
  await settle()
  button('Iniciar sessão').click()
  await settle()
  expect(native.startSession).toHaveBeenCalledWith(false, undefined)
})

it('mantém a substituição manual opcional na configuração avançada', async () => {
  native.startSession.mockResolvedValue({ ...inactive, phase: 'active', runtimeId: 'RUNTIME-SINTETICO', memoryProtection: 'protected' })
  await mount()
  button('Configurações').click()
  await settle()
  const advanced = container.querySelector('details')!
  const input = container.querySelector<HTMLInputElement>('#signaling-endpoint')!
  expect(advanced.open).toBe(false)
  expect(advanced.contains(input)).toBe(true)
  expect(input.placeholder).toBe('Conexão direta sem servidor')
  expect(input.maxLength).toBe(512)
  advanced.open = true
  input.value = 'wss://synthetic.invalid/v1/rendezvous'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
  button('Iniciar sessão').click()
  await settle()
  expect(native.startSession).toHaveBeenCalledWith(false, 'wss://synthetic.invalid/v1/rendezvous')
})

it('explica o pareamento sem servidor e os limites de alcance dos endereços', async () => {
  await mount()
  expect(container.textContent).toContain('Conecte diretamente, sem servidor')
  expect(container.textContent).not.toContain('informe o endpoint')
  expect(container.textContent).not.toContain('Configurar operador')
  button('Entrar com convite').click()
  await settle()
  expect(container.textContent).toContain('Conectar sem servidor')
  expect(container.textContent).toContain('NAT restritivo')
  expect(container.textContent).toContain('devolve a resposta gerada')
  expect(button('Criar convite').disabled).toBe(true)
  expect(container.querySelector<HTMLTextAreaElement>('#join-invitation')?.disabled).toBe(false)
  expect(native.startSession).not.toHaveBeenCalled()
})

it('mantém a resposta para devolver na aba de convites, sem simular conexão', async () => {
  const response: Invite = { token: 'RESPOSTA-SINTETICA', expiresInMs: 60000 }
  native.joinInvite.mockResolvedValue(response)
  await mount()
  button('Entrar com convite').click()
  await settle()
  const input = container.querySelector<HTMLTextAreaElement>('#join-invitation')!
  expect(input.maxLength).toBe(16000)
  input.value = 'CONVITE-SINTETICO'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
  native.status.mockResolvedValue({ ...inactive, phase: 'active', runtimeId: 'RUNTIME-DIRETO', infrastructureConfigured: true, memoryProtection: 'protected' })
  button('Iniciar pareamento').click()
  await settle()
  expect(container.querySelector('h1')?.textContent).toBe('Convites')
  expect(container.querySelector<HTMLTextAreaElement>('[aria-label="Resposta temporária para compartilhar"]')?.value).toBe('RESPOSTA-SINTETICA')
  expect(container.textContent).toContain('Devolva esta resposta')
  expect(container.textContent).not.toContain('Pareamento iniciado.')
  button('Contatos').click()
  await settle()
  expect(container.textContent).toContain('Nenhum contato salvo')
  button('Convites').click()
  await settle()
  expect(container.querySelector<HTMLTextAreaElement>('[aria-label="Resposta temporária para compartilhar"]')?.value).toBe('RESPOSTA-SINTETICA')
  native.status.mockResolvedValue(inactive)
  button('Sair e descartar sessão').click()
  await settle()
  expect(container.querySelector('[aria-label="Resposta temporária para compartilhar"]')).toBeNull()
})

it('não renova a resposta ao trocar de aba e permite concluir colando uma resposta', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date', 'performance'] })
  native.joinInvite.mockResolvedValueOnce({ token: 'RESPOSTA-CURTA', expiresInMs: 5000 }).mockResolvedValueOnce(null)
  await mount()
  button('Entrar com convite').click()
  await settle()
  let input = container.querySelector<HTMLTextAreaElement>('#join-invitation')!
  input.value = 'OFERTA'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
  native.status.mockResolvedValue({ ...inactive, phase: 'active', runtimeId: 'RUNTIME-DIRETO', infrastructureConfigured: true, memoryProtection: 'protected' })
  button('Iniciar pareamento').click()
  await settle()
  button('Configurações').click()
  await settle()
  await vi.advanceTimersByTimeAsync(6000)
  button('Convites').click()
  await settle()
  expect(container.querySelector('[aria-label="Resposta temporária para compartilhar"]')).toBeNull()
  input = container.querySelector<HTMLTextAreaElement>('#join-invitation')!
  input.value = 'RESPOSTA-RECEBIDA'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
  button('Iniciar pareamento').click()
  await settle()
  expect(container.querySelector('h1')?.textContent).toBe('Contatos')
  expect(container.textContent).toContain('Aguarde a conexão e verifique a identidade')
})

it('recebe convite sem sessão e aguarda o estado nativo antes de abrir contatos', async () => {
  let finishJoin!: () => void
  let finishStatus!: (value: SessionStatus) => void
  native.joinInvite.mockReturnValue(new Promise<void>(resolve => { finishJoin = resolve }))
  await mount()
  button('Entrar com convite').click()
  await settle()
  const input = container.querySelector<HTMLTextAreaElement>('#join-invitation')!
  input.value = 'CONVITE-SINTETICO'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
  button('Iniciar pareamento').click()
  await settle()
  expect(native.joinInvite).toHaveBeenCalledWith('CONVITE-SINTETICO', '')
  expect(input.value).toBe('')
  expect(native.startSession).not.toHaveBeenCalled()
  native.status.mockReturnValue(new Promise<SessionStatus>(resolve => { finishStatus = resolve }))
  finishJoin()
  await settle()
  expect(container.querySelector('h1')?.textContent).toBe('Convites')
  finishStatus({ ...inactive, phase: 'active', runtimeId: 'RUNTIME-CONVITE', infrastructureConfigured: true, connectionSetup: { configured: true, source: 'invitation' }, memoryProtection: 'protected' })
  await settle()
  expect(container.querySelector('h1')?.textContent).toBe('Contatos')
  expect(container.textContent).toContain('Identidade ativa nesta execução')
})

it('permite receber convite com identidade local sem infraestrutura configurada', async () => {
  native.status.mockResolvedValue({ ...inactive, phase: 'active', runtimeId: 'RUNTIME-SINTETICO', memoryProtection: 'protected' })
  await mount()
  button('Convites').click()
  await settle()
  expect(button('Criar convite').disabled).toBe(true)
  expect(container.querySelector<HTMLTextAreaElement>('#join-invitation')?.disabled).toBe(false)
})

it('mantém o pareamento no aplicativo ao trocar de aba e libera as ações ao concluir', async () => {
  let finishJoin!: () => void
  native.status.mockResolvedValue({ ...inactive, phase: 'active', runtimeId: 'RUNTIME-SINTETICO', memoryProtection: 'protected' })
  native.joinInvite.mockReturnValue(new Promise<void>(resolve => { finishJoin = resolve }))
  await mount()
  button('Convites').click()
  await settle()
  const input = container.querySelector<HTMLTextAreaElement>('#join-invitation')!
  input.value = 'CONVITE-SINTETICO'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
  button('Iniciar pareamento').click()
  await settle()
  button('Configurações').click()
  await settle()
  expect(button('Sair e descartar sessão').disabled).toBe(true)
  button('Sair e descartar sessão').click()
  expect(native.endSession).not.toHaveBeenCalled()
  button('Convites').click()
  await settle()
  expect(button('Conectando…').disabled).toBe(true)
  expect(container.querySelector<HTMLTextAreaElement>('#join-invitation')?.value).toBe('')
  finishJoin()
  await settle()
  expect(container.querySelector('h1')?.textContent).toBe('Contatos')
  expect(button('Sair e descartar sessão').disabled).toBe(false)
})

it('ignora a resposta de pareamento depois de desmontar o aplicativo', async () => {
  let finishJoin!: () => void
  native.joinInvite.mockReturnValue(new Promise<void>(resolve => { finishJoin = resolve }))
  await mount()
  button('Entrar com convite').click()
  await settle()
  const input = container.querySelector<HTMLTextAreaElement>('#join-invitation')!
  input.value = 'CONVITE-SINTETICO'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
  button('Iniciar pareamento').click()
  await settle()
  app!.unmount()
  app = undefined
  native.status.mockClear()
  finishJoin()
  await settle()
  expect(native.status).not.toHaveBeenCalled()
})

it('mostra a recusa do IPC confirmada pelo núcleo ao iniciar a sessão', async () => {
  const reason = 'Não foi possível iniciar o cofre seguro; a sessão foi recusada.'
  native.startSession.mockRejectedValue(new Error('DETALHE-NATIVO-NAO-EXIBIR'))
  await mount()
  native.status.mockResolvedValue({ ...inactive, phase: 'blocked', reason })
  button('Iniciar sessão').click()
  await settle()
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(reason)
  expect(container.textContent).not.toContain('DETALHE-NATIVO-NAO-EXIBIR')
  expect(container.textContent).not.toContain('Identidade ativa nesta execução')
})

it('mostra a recusa do núcleo ao entrar com convite sem revelar o erro recebido', async () => {
  const reason = 'Não foi possível iniciar o cofre seguro; a sessão foi recusada.'
  native.joinInvite.mockRejectedValue(new Error('CONVITE-NAO-EXIBIR'))
  await mount()
  button('Entrar com convite').click()
  await settle()
  const input = container.querySelector<HTMLTextAreaElement>('#join-invitation')!
  input.value = 'CONVITE-SINTETICO'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
  native.status.mockResolvedValue({ ...inactive, phase: 'blocked', reason })
  button('Iniciar pareamento').click()
  await settle()
  expect(container.textContent).toContain(reason)
  expect(container.textContent).not.toContain('CONVITE-NAO-EXIBIR')
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
