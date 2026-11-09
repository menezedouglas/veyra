<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef } from 'vue'
import { Moon, Sun, Power, ArrowRight, Monitor, CircleAlert, Fingerprint, RefreshCw, Inbox, UsersRound, Ticket, Network, Settings2, Plus, Eye, ShieldCheck, ChevronRight, Radio } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import VeyraLogo from '@/components/VeyraLogo.vue'
import PrivacyStatus from '@/components/PrivacyStatus.vue'
import ConnectivityDiagnostics from '@/components/ConnectivityDiagnostics.vue'
import PeerIdentityCard from '@/components/PeerIdentityCard.vue'
import FingerprintVerification from '@/components/FingerprintVerification.vue'
import EphemeralMessageCard from '@/components/EphemeralMessageCard.vue'
import OneTimeViewer from '@/components/OneTimeViewer.vue'
import MessageComposer from '@/components/MessageComposer.vue'
import InvitePanel from '@/components/InvitePanel.vue'
import VeyraDialog from '@/components/ui/dialog/VeyraDialog.vue'
import { api, type SessionStatus, type PendingMessage } from '@/lib/ipc'
import { validateSignalingEndpoint } from '@/lib/validation'
import { useAppearance } from '@/stores/appearance'

type Tab = 'pending' | 'peers' | 'invites' | 'connections' | 'settings'
type PendingView = PendingMessage & { expiresAt: number }
const tabs = [
  { id: 'pending' as Tab, label: 'Mensagens pendentes', icon: Inbox },
  { id: 'peers' as Tab, label: 'Contatos da sessão', icon: UsersRound },
  { id: 'invites' as Tab, label: 'Convites', icon: Ticket },
  { id: 'connections' as Tab, label: 'Conexões', icon: Network },
  { id: 'settings' as Tab, label: 'Configurações', icon: Settings2 },
]
const descriptions: Record<Tab, string> = {
  pending: 'Somente mensagens não consumidas. Nenhum histórico é mantido.',
  peers: 'Identidades válidas apenas nesta execução. Compare antes de enviar.',
  invites: 'Pareamento temporário, sem cadastro nem diretório público.',
  connections: 'Estado informado pelo núcleo e rota selecionada pelo transporte.',
  settings: 'Preferências em RAM e configuração do operador desta sessão.',
}
const appearance = useAppearance()
const desktop = api.isDesktop()
const tab = ref<Tab>('pending')
const status = shallowRef<SessionStatus | null>(null)
const pending = shallowRef<PendingView[]>([])
const busy = ref(false)
const error = ref('')
const notice = ref('')
const endpoint = shallowRef('')
const composerOpen = ref(false)
const verificationPeerId = ref<string | null>(null)
const revokePeerId = ref<string | null>(null)
const opening = shallowRef<PendingView | null>(null)
const discarding = ref<string | null>(null)
const locallyClaimed = new Set<string>()
const sessionExists = computed(() => !!status.value?.runtimeId)
const networkReady = computed(() => desktop && sessionExists.value && status.value?.phase === 'active' && status.value.infrastructureConfigured)
const peers = computed(() => status.value?.peers ?? [])
const verificationPeer = computed(() => peers.value.find(peer => peer.id === verificationPeerId.value))
const revokePeer = computed(() => peers.value.find(peer => peer.id === revokePeerId.value))
const canCompose = computed(() => networkReady.value && peers.value.some(peer => peer.verified))
const endpointError = computed(() => validateSignalingEndpoint(endpoint.value))
const title = computed(() => tabs.find(item => item.id === tab.value)?.label)
let refreshing = false
let generation = 0
let alive = true
let poll: ReturnType<typeof setInterval> | undefined
let monoCheckpoint = performance.now()
let wallCheckpoint = Date.now()

function clearSensitiveUI() {
  composerOpen.value = false
  opening.value = null
  verificationPeerId.value = null
  revokePeerId.value = null
}
function applyStatus(value: SessionStatus) {
  if (!value.runtimeId || (status.value?.runtimeId && value.runtimeId !== status.value.runtimeId)) {
    clearSensitiveUI(); pending.value = []; locallyClaimed.clear()
  }
  status.value = value
  if (value.phase === 'blocked' || value.memoryProtection === 'unavailable') clearSensitiveUI()
  if (opening.value && !value.peers.some(peer => peer.id === opening.value?.peerId && peer.verified)) opening.value = null
}
async function refresh() {
  if (!desktop || busy.value || refreshing || !alive) return
  refreshing = true
  const expectedGeneration = generation
  try {
    const value = await api.status()
    if (!alive || expectedGeneration !== generation) return
    applyStatus(value)
    if (value.runtimeId) {
      const requestStartedAt = performance.now()
      const messages = await api.listPending()
      if (!alive || expectedGeneration !== generation) return
      if (messages.length > 128) throw new Error('Pending metadata limit exceeded')
      const now = performance.now()
      const activeIds = new Set(messages.map(message => message.id))
      for (const id of locallyClaimed) if (!activeIds.has(id)) locallyClaimed.delete(id)
      pending.value = messages.filter(message => !locallyClaimed.has(message.id)).map(message => {
        const previous = pending.value.find(item => item.id === message.id)
        const reportedDeadline = requestStartedAt + Math.max(0, Math.min(message.expiresInMs, 300000))
        return { ...message, expiresAt: Math.min(previous?.expiresAt ?? reportedDeadline, reportedDeadline) }
      }).filter(message => message.expiresAt > now)
    } else pending.value = []
  } catch {
    if (alive && expectedGeneration === generation) { error.value = 'O núcleo não respondeu. Não é possível confirmar o estado da sessão.'; if (status.value) status.value = { ...status.value, phase: 'blocked' }; clearSensitiveUI(); pending.value = [] }
  } finally { refreshing = false }
}
async function start() {
  if (endpointError.value || busy.value) return
  generation++; busy.value = true; error.value = ''; notice.value = ''
  try { applyStatus(await api.startSession(false, endpoint.value.trim() || undefined)) }
  catch { error.value = 'Não foi possível iniciar uma sessão. Confira a proteção de memória, o cofre e o operador configurado.' }
  finally { busy.value = false }
}
async function end() {
  generation++; busy.value = true; error.value = ''; notice.value = ''; clearSensitiveUI()
  try { await api.endSession(); applyStatus(await api.status()); pending.value = []; locallyClaimed.clear() }
  catch { error.value = 'Não foi possível confirmar o encerramento. Feche o aplicativo para encerrar os componentes locais.' }
  finally { busy.value = false }
}
async function discard(message: PendingView) {
  if (discarding.value) return
  discarding.value = message.id
  try { await api.discardMessage(message.id); pending.value = pending.value.filter(item => item.id !== message.id) }
  catch { error.value = 'Não foi possível confirmar o descarte. O prazo original permanece em vigor.' }
  finally { discarding.value = null }
}
function openMessage(message: PendingView) {
  if (status.value?.phase !== 'active' || status.value.memoryProtection !== 'protected') { error.value = 'O núcleo não autorizou operações sensíveis nesta sessão.'; return }
  if (locallyClaimed.size >= 128) { error.value = 'Aguarde a atualização das pendências antes de abrir outra mensagem.'; return }
  opening.value = message
}
function consumeLocal(message: PendingView) {
  // This set is bounded by the core's pending limit. An id is removed when the
  // next authoritative listing no longer contains it, without retaining history.
  locallyClaimed.add(message.id)
  pending.value = pending.value.filter(item => item.id !== message.id)
}
async function revoke() {
  const peer = revokePeer.value
  if (!peer || busy.value) return
  generation++; busy.value = true
  try {
    await api.revokePeer(peer.id)
    pending.value = pending.value.filter(message => message.peerId !== peer.id)
    revokePeerId.value = null
    notice.value = 'Contato revogado e pendências locais descartadas. Cópias remotas não estão sob controle do Veyra.'
  } catch { error.value = 'Não foi possível confirmar a revogação. Confira o estado do núcleo.' }
  finally { busy.value = false; void refresh() }
}
function tick() {
  const mono = performance.now(), wall = Date.now()
  if (Math.abs((wall - wallCheckpoint) - (mono - monoCheckpoint)) > 1500) {
    opening.value = null
    for (const message of pending.value) { locallyClaimed.add(message.id); void api.discardMessage(message.id).catch(() => {}) }
    pending.value = []
    notice.value = 'Mudança de relógio ou retomada detectada. Pendências foram ocultadas e o descarte foi solicitado ao núcleo por precaução.'
  }
  monoCheckpoint = mono; wallCheckpoint = wall
  void refresh()
}
onMounted(() => { void refresh(); if (desktop) poll = setInterval(tick, 1000) })
onUnmounted(() => { alive = false; if (poll) clearInterval(poll); clearSensitiveUI(); pending.value = []; status.value = null; endpoint.value = ''; locallyClaimed.clear() })
</script>

<template>
  <div class="min-h-screen bg-background text-foreground lg:flex">
    <aside class="border-b border-border bg-card lg:fixed lg:inset-y-0 lg:left-0 lg:flex lg:w-64 lg:flex-col lg:border-b-0 lg:border-r">
      <div class="flex h-20 items-center justify-between px-6"><VeyraLogo /><span class="rounded border border-border px-1.5 py-0.5 font-mono text-[9px] tracking-wide text-muted-foreground">EXPERIMENTAL</span></div>
      <nav aria-label="Navegação principal" class="flex gap-1 overflow-x-auto px-3 pb-3 lg:mt-4 lg:flex-col lg:px-4">
        <button v-for="item in tabs" :key="item.id" type="button" class="flex min-h-11 shrink-0 items-center gap-3 rounded-md px-3 text-left text-sm transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" :class="tab === item.id ? 'bg-primary/10 font-semibold text-primary' : 'text-muted-foreground hover:bg-secondary hover:text-foreground'" :aria-current="tab === item.id ? 'page' : undefined" @click="tab = item.id"><component :is="item.icon" class="size-[18px] shrink-0" aria-hidden="true" /><span>{{ item.label }}</span><span v-if="item.id === 'pending' && pending.length" class="ml-auto rounded bg-primary/15 px-1.5 py-0.5 text-[10px]">{{ pending.length }}</span></button>
      </nav>
      <div class="mt-auto hidden p-5 lg:block"><div class="rounded-lg border border-border bg-background p-4"><div class="flex items-center gap-2 text-xs font-semibold"><Fingerprint class="size-4 text-primary" aria-hidden="true" />Identidade temporária</div><p class="mt-3 break-all font-mono text-[10px] leading-relaxed text-muted-foreground">{{ status?.fingerprint || 'Gerada ao iniciar uma sessão' }}</p><p class="mt-3 text-[11px] leading-relaxed text-muted-foreground">Sem contas. Sem histórico. Somente esta execução.</p></div><p class="mt-4 text-center text-[10px] text-muted-foreground">Veyra 0.1.0 · Apache-2.0</p></div>
    </aside>

    <div class="min-w-0 flex-1 lg:ml-64">
      <header class="border-b border-border"><div class="mx-auto flex h-16 max-w-6xl items-center justify-between gap-3 px-6 sm:px-8"><span class="inline-flex items-center gap-2 text-xs text-muted-foreground"><span class="size-1.5 rounded-full" :class="sessionExists && status?.phase === 'active' ? 'bg-success' : 'bg-warning'" aria-hidden="true" />{{ sessionExists ? (status?.phase === 'active' ? 'Identidade ativa nesta execução' : 'Estado da sessão não confirmado') : 'Sessão não iniciada' }}</span><div class="flex items-center gap-2"><Button variant="ghost" size="icon" :aria-label="appearance.theme === 'dark' ? 'Usar tema claro' : 'Usar tema escuro'" @click="appearance.toggleTheme"><Sun v-if="appearance.theme === 'dark'" class="size-[18px]" aria-hidden="true" /><Moon v-else class="size-[18px]" aria-hidden="true" /></Button><Button v-if="sessionExists" variant="ghost" size="sm" :disabled="busy" @click="end"><Power class="size-3.5" aria-hidden="true" /><span class="hidden sm:inline">Sair e descartar sessão</span><span class="sm:hidden">Sair</span></Button></div></div></header>
      <main class="mx-auto max-w-6xl px-6 py-8 sm:px-8 sm:py-10">
        <div class="mb-7 flex flex-wrap items-start justify-between gap-4"><div><p class="mb-2 flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-[.15em] text-muted-foreground">Veyra<ChevronRight class="size-3" aria-hidden="true" />Sessão efêmera</p><h1 class="text-2xl font-semibold tracking-tight">{{ title }}</h1><p class="mt-2 max-w-2xl text-sm leading-relaxed text-muted-foreground">{{ descriptions[tab] }}</p></div><Button v-if="tab === 'pending' && sessionExists" :disabled="!canCompose" @click="composerOpen = true"><Plus class="size-4" aria-hidden="true" />Nova mensagem</Button></div>
        <div v-if="!desktop" class="mb-6 flex gap-3 rounded-lg border border-warning/35 bg-warning/5 p-4 text-sm" role="status"><Monitor class="mt-0.5 size-5 shrink-0 text-warning" aria-hidden="true" /><div><p class="font-semibold">Aplicativo desktop necessário</p><p class="mt-1 leading-relaxed text-muted-foreground">O navegador não tem acesso ao núcleo Rust. Inicie Veyra no Windows ou Linux para criar uma sessão. Esta página não estabelece conexões nem gera credenciais.</p></div></div>
        <p v-if="error" role="alert" class="mb-5 flex gap-2 rounded-lg border border-destructive/35 bg-destructive/5 p-4 text-sm leading-relaxed text-destructive"><CircleAlert class="mt-0.5 size-4 shrink-0" aria-hidden="true" />{{ error }}</p>
        <p v-if="notice" role="status" class="mb-5 rounded-lg border border-border bg-card p-4 text-sm leading-relaxed text-muted-foreground">{{ notice }}</p>
        <div v-if="sessionExists && status?.reason" class="mb-6 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-warning/30 bg-warning/5 p-4"><p class="flex max-w-3xl gap-2 text-xs leading-relaxed text-warning"><CircleAlert class="mt-0.5 size-4 shrink-0" aria-hidden="true" />{{ status.reason }}</p><Button v-if="!status.infrastructureConfigured" variant="ghost" size="sm" @click="tab = 'settings'">Configurar operador<ArrowRight class="size-3.5" aria-hidden="true" /></Button></div>

        <section v-if="!sessionExists && tab !== 'settings' && tab !== 'connections'" class="rounded-xl border border-border bg-card p-7 sm:p-10"><div class="mb-6 flex size-14 items-center justify-center rounded-xl border border-primary/25 bg-primary/5"><Fingerprint class="size-7 text-primary" :stroke-width="1.5" aria-hidden="true" /></div><h2 class="text-xl font-semibold">Uma nova sessão, uma nova identidade.</h2><p class="mt-3 max-w-xl text-sm leading-relaxed text-muted-foreground">Mensagens privadas, diretas e efêmeras. A identidade é gerada no núcleo Rust e permanece em RAM. Não há conta, cadastro ou recuperação de sessão.</p><div class="mt-6 flex flex-wrap gap-3"><Button :disabled="!desktop || busy || !!endpointError" @click="start">{{ busy ? 'Iniciando…' : 'Iniciar sessão' }}<ArrowRight class="size-4" aria-hidden="true" /></Button><Button variant="secondary" @click="tab = 'settings'">Configurar operador</Button></div><p class="mt-5 max-w-xl text-xs leading-relaxed text-muted-foreground">Sem operador configurado, apenas a identidade local fica disponível. Para parear, informe o endpoint WSS fornecido pelo operador. Fechar esta execução descarta a sessão.</p></section>

        <template v-else-if="tab === 'pending'">
          <div v-if="pending.length" class="space-y-4"><EphemeralMessageCard v-for="message in pending" :key="message.id" :message="message" :peer="peers.find(peer => peer.id === message.peerId)" :busy="discarding === message.id" @open="openMessage(message)" @discard="discard(message)" @expired="pending = pending.filter(item => item.id !== message.id)" /></div>
          <section v-else class="flex min-h-80 flex-col items-center justify-center rounded-xl border border-border bg-card px-6 py-12 text-center"><div class="mb-5 flex size-16 items-center justify-center rounded-xl border border-border bg-background"><Inbox class="size-7 text-muted-foreground" :stroke-width="1.5" aria-hidden="true" /></div><h2 class="text-lg font-semibold">Nenhuma mensagem pendente</h2><p class="mt-3 max-w-md text-sm leading-relaxed text-muted-foreground">Mensagens aceitas ficam cifradas em RAM até a abertura ou expiração. Após consumo, o cartão desaparece.</p><Button class="mt-6" variant="secondary" @click="tab = peers.length ? 'peers' : 'invites'">{{ peers.length ? 'Ver contatos da sessão' : 'Convidar uma pessoa' }}<ArrowRight class="size-4" aria-hidden="true" /></Button></section>
          <div class="mt-6 flex items-start gap-2 text-xs leading-relaxed text-muted-foreground"><Eye class="mt-0.5 size-4 shrink-0" aria-hidden="true" /><p>Uma abertura, até 30 segundos de exibição. Perder foco oculta o conteúdo. Aceitação em RAM não garante abertura: encerrar ou perder o cofre descarta a mensagem.</p></div>
        </template>

        <template v-else-if="tab === 'peers'"><div v-if="peers.length" class="grid gap-4 xl:grid-cols-2"><PeerIdentityCard v-for="peer in peers" :key="peer.id" :peer="peer" :busy="busy" @verify="verificationPeerId = $event.id" @revoke="revokePeerId = $event.id" /></div><section v-else class="rounded-xl border border-border bg-card px-6 py-12 text-center"><UsersRound class="mx-auto mb-5 size-9 text-muted-foreground" :stroke-width="1.5" aria-hidden="true" /><h2 class="text-lg font-semibold">Nenhum contato nesta sessão</h2><p class="mx-auto mt-3 max-w-md text-sm leading-relaxed text-muted-foreground">Crie ou receba um convite. Após a conexão, compare o código antes de aprovar a identidade.</p><Button class="mt-6" variant="secondary" @click="tab = 'invites'">Abrir convites<ArrowRight class="size-4" aria-hidden="true" /></Button></section><p class="mt-5 text-xs leading-relaxed text-muted-foreground">A identidade muda ao reiniciar o núcleo. Nomes são rótulos locais e não comprovam quem está do outro lado.</p></template>
        <InvitePanel v-else-if="tab === 'invites'" :enabled="networkReady" @joined="tab = 'peers'" @refresh="refresh" />
        <div v-else-if="tab === 'connections'" class="grid items-start gap-6 xl:grid-cols-2"><ConnectivityDiagnostics :status="status" :desktop="desktop" /><PrivacyStatus :memory-protection="status?.memoryProtection ?? 'not-checked'" /><div class="xl:col-span-2"><Button variant="secondary" :disabled="!desktop || busy" @click="refresh"><RefreshCw class="size-4" aria-hidden="true" />Atualizar diagnóstico</Button></div></div>

        <div v-else-if="tab === 'settings'" class="space-y-6">
          <section class="rounded-lg border border-border bg-card p-6"><h2 class="flex items-center gap-2 text-base font-semibold"><Radio class="size-4 text-primary" aria-hidden="true" />Operador de conectividade</h2><p class="mt-2 max-w-2xl text-sm leading-relaxed text-muted-foreground">Informe o endpoint real fornecido pelo operador. Sinalização e TURN podem observar metadados de rede; as chaves ponta a ponta permanecem nos dispositivos.</p><label for="signaling-endpoint" class="mb-2 mt-5 block text-sm font-medium">Endpoint WSS <span class="text-xs font-normal text-muted-foreground">(opcional para iniciar somente identidade local)</span></label><input id="signaling-endpoint" v-model="endpoint" type="url" class="w-full max-w-2xl font-mono text-xs" placeholder="Endereço do operador, com caminho /v1/rendezvous" autocomplete="off" autocapitalize="off" :spellcheck="false" :disabled="sessionExists || busy" maxlength="2048" aria-describedby="endpoint-help" /><p id="endpoint-help" class="mt-2 text-xs leading-relaxed" :class="endpointError ? 'text-destructive' : 'text-muted-foreground'">{{ endpointError || 'Somente wss://, certificado válido e caminho /v1/rendezvous. Sem tokens na URL. Nenhum serviço público é configurado automaticamente.' }}</p><p v-if="sessionExists" class="mt-4 text-xs text-warning">Para trocar de operador, encerre esta sessão e inicie outra identidade.</p><Button v-else class="mt-5" :disabled="!desktop || busy || !!endpointError" @click="start">{{ busy ? 'Iniciando…' : 'Iniciar sessão' }}<ArrowRight class="size-4" aria-hidden="true" /></Button><p class="mt-5 border-t border-border pt-4 text-xs leading-relaxed text-muted-foreground">Credenciais temporárias TURN dependem do operador. Não informe segredos operacionais do relay no cliente. A disponibilidade e os caminhos aceitos dependem da infraestrutura configurada.</p></section>
          <section class="rounded-lg border border-border bg-card p-6"><h2 class="text-base font-semibold">Aparência e ciclo da sessão</h2><div class="mt-5 flex flex-wrap items-center justify-between gap-3"><div><p class="text-sm font-medium">Tema {{ appearance.theme === 'dark' ? 'escuro' : 'claro' }}</p><p class="mt-1 text-xs text-muted-foreground">Preferência somente em RAM, sem fonte externa.</p></div><Button variant="secondary" @click="appearance.toggleTheme">{{ appearance.theme === 'dark' ? 'Usar tema claro' : 'Usar tema escuro' }}</Button></div><div class="mt-5 border-t border-border pt-5"><p class="text-sm font-medium">Recepção em segundo plano</p><p class="mt-1 text-xs leading-relaxed text-muted-foreground">Indisponível nesta versão. Fechar o aplicativo encerra os componentes locais; o autostart permanece desativado.</p></div></section>
          <section v-if="sessionExists" class="rounded-lg border border-border bg-card p-6"><h2 class="flex items-center gap-2 text-base font-semibold"><ShieldCheck class="size-4 text-primary" aria-hidden="true" />Identidade desta execução</h2><dl class="mt-5 space-y-4"><div><dt class="mb-1 text-xs text-muted-foreground">Impressão digital</dt><dd class="break-all font-mono text-xs leading-relaxed">{{ status?.fingerprint }}</dd></div><div><dt class="mb-1 text-xs text-muted-foreground">Identificador da execução</dt><dd class="break-all font-mono text-xs">{{ status?.runtimeId }}</dd></div></dl><Button class="mt-6" variant="destructive" :disabled="busy" @click="end"><Power class="size-4" aria-hidden="true" />Sair e descartar sessão</Button></section>
          <PrivacyStatus :memory-protection="status?.memoryProtection ?? 'not-checked'" />
        </div>
        <p class="mt-8 border-t border-border pt-5 text-[11px] leading-relaxed text-muted-foreground">Privado por padrão. Efêmero por design. Protocolo experimental, sem revisão independente. Veyra não oferece anonimato, conexão universal ou apagamento absoluto.</p>
      </main>
    </div>
    <FingerprintVerification v-if="verificationPeer" :key="verificationPeer.id" :peer="verificationPeer" @close="verificationPeerId = null" @verified="refresh" />
    <MessageComposer v-if="composerOpen && sessionExists" :peers="peers" :delivery-updates="status?.deliveries ?? []" @close="composerOpen = false" @sent="refresh" />
    <OneTimeViewer v-if="opening && sessionExists" :key="opening.id" :message-id="opening.id" :expires-at="opening.expiresAt" @close="opening = null" @consumed="opening && consumeLocal(opening)" />
    <VeyraDialog v-if="revokePeer" :open="true" title="Revogar contato?" description="Novas operações serão impedidas e as pendências locais desse contato serão descartadas. Cópias remotas não podem ser apagadas por esta ação." @close="revokePeerId = null"><p class="font-semibold">{{ revokePeer.label || 'Contato da sessão' }}</p><div class="mt-6 flex justify-end gap-3"><Button variant="secondary" @click="revokePeerId = null">Cancelar</Button><Button variant="destructive" :disabled="busy" @click="revoke">Revogar e descartar</Button></div></VeyraDialog>
  </div>
</template>
