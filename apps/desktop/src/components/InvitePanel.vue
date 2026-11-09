<script setup lang="ts">
import { computed, onUnmounted, ref, shallowRef } from 'vue'
import { ArrowRight, Copy, TicketPlus, CircleAlert, Link2 } from '@lucide/vue'
import { api, type Invite } from '@/lib/ipc'
import { Button } from '@/components/ui/button'
import ExpiryIndicator from './ExpiryIndicator.vue'
const props = defineProps<{ enabled: boolean; joinEnabled: boolean; joining: boolean; response?: (Invite & { expiresAt: number }) | null }>()
const emit = defineEmits<{ join: [invite: string]; responseExpired: [] }>()
const generated = shallowRef<(Invite & { expiresAt: number }) | null>(null)
const invitation = shallowRef('')
const busy = ref(false)
const error = ref('')
const notice = ref('')
let alive = true
const canJoin = computed(() => invitation.value.trim().length > 0 && invitation.value.length <= 16000)
async function create() {
  if (!props.enabled || busy.value) return
  busy.value = true; error.value = ''; notice.value = ''
  const startedAt = performance.now()
  try {
    const value = await api.createInvite()
    if (alive) generated.value = { ...value, expiresAt: startedAt + Math.max(0, Math.min(value.expiresInMs, 300000)) }
  } catch { error.value = 'Não foi possível criar um convite. Confira o diagnóstico de conexão.' }
  finally { busy.value = false }
}
function join() {
  if (!props.joinEnabled || !canJoin.value || busy.value) return
  error.value = ''; notice.value = ''
  const invite = invitation.value.trim()
  invitation.value = ''
  emit('join', invite)
}
async function copyInvite(token?: string) {
  if (!token) return
  try { await navigator.clipboard.writeText(token); notice.value = 'Código copiado por sua solicitação. O histórico externo do clipboard pode conservar uma cópia.' }
  catch { notice.value = 'Não foi possível copiar automaticamente. Selecione o convite e use o atalho de cópia, se desejar.' }
}
function expire() { generated.value = null; notice.value = 'O convite exibido expirou.' }
onUnmounted(() => { alive = false; invitation.value = ''; generated.value = null })
</script>

<template>
  <section class="mb-6 rounded-lg border border-border bg-card p-6" aria-label="Como conectar sem servidor">
    <h2 class="text-base font-semibold">Conectar sem servidor</h2>
    <ol class="mt-3 list-inside list-decimal space-y-2 text-sm text-muted-foreground"><li>Uma pessoa cria um convite e o envia à outra por um canal de confiança.</li><li>A outra pessoa cola o convite aqui e devolve a resposta gerada.</li><li>Quem criou o convite cola a resposta aqui. Após conectar, ambos comparam e aprovam o código da conexão.</li></ol>
    <p class="mt-4 text-xs leading-relaxed text-warning">A conexão usa endereços diretamente acessíveis, como os da mesma rede local. Sem servidor STUN ou TURN, NAT restritivo, isolamento de Wi-Fi e firewalls podem impedir a conexão.</p>
  </section>
  <section v-if="response" class="mb-6 rounded-lg border border-primary/40 bg-primary/5 p-6" aria-label="Resposta para devolver">
    <div class="flex flex-wrap items-center justify-between gap-3"><h2 class="text-lg font-semibold">Devolva esta resposta</h2><ExpiryIndicator :expires-at="response.expiresAt" @expired="emit('responseExpired')" /></div>
    <p class="mt-2 text-sm leading-relaxed text-muted-foreground">Envie este código à pessoa que criou o convite. Ela deve colá-lo em “Receber convite ou resposta” para concluir o pareamento.</p>
    <textarea :value="response.token" readonly rows="5" class="mt-4 w-full resize-none break-all font-mono text-xs leading-relaxed" aria-label="Resposta temporária para compartilhar" autocomplete="off" :spellcheck="false" />
    <Button variant="secondary" class="mt-4" @click="copyInvite(response.token)"><Copy class="size-4" aria-hidden="true" />Copiar resposta</Button>
    <p class="mt-3 text-xs leading-relaxed text-warning">O clipboard e seu histórico podem conservar uma cópia fora do Veyra.</p>
  </section>
  <div class="grid items-start gap-6 xl:grid-cols-2">
    <section class="rounded-lg border border-border bg-card p-6"><div class="mb-5 flex size-11 items-center justify-center rounded-lg bg-primary/10"><TicketPlus class="size-5 text-primary" aria-hidden="true" /></div><h2 class="text-lg font-semibold">Convidar uma pessoa</h2><p class="mt-2 text-sm leading-relaxed text-muted-foreground">Gere um convite temporário, compartilhe e aguarde o código de resposta. Mantenha o aplicativo aberto durante a troca.</p><template v-if="generated"><div class="mt-5 flex flex-wrap items-center justify-between gap-2"><span class="text-xs font-semibold">Convite temporário</span><ExpiryIndicator :expires-at="generated.expiresAt" @expired="expire" /></div><textarea :value="generated.token" readonly rows="5" class="mt-3 w-full resize-none break-all font-mono text-xs leading-relaxed" aria-label="Convite temporário para compartilhar" autocomplete="off" :spellcheck="false" /><p class="mt-3 text-xs leading-relaxed text-warning">Copiar coloca o convite no clipboard do sistema, cujo histórico ou sincronização pode preservar cópias fora do Veyra.</p><Button variant="secondary" class="mt-4" @click="copyInvite(generated.token)"><Copy class="size-4" aria-hidden="true" />Copiar convite</Button></template><Button v-else class="mt-6" :disabled="!enabled || busy" @click="create"><TicketPlus class="size-4" aria-hidden="true" />{{ busy ? 'Criando…' : 'Criar convite' }}</Button><p class="mt-5 text-xs leading-relaxed text-muted-foreground">Convites compartilhados continuam sujeitos ao prazo original e ao encerramento da sessão. Não são links de navegador.</p></section>
    <section class="rounded-lg border border-border bg-card p-6"><div class="mb-5 flex size-11 items-center justify-center rounded-lg bg-secondary"><Link2 class="size-5 text-muted-foreground" aria-hidden="true" /></div><h2 class="text-lg font-semibold">Receber convite ou resposta</h2><p class="mt-2 text-sm leading-relaxed text-muted-foreground">Cole o código recebido. Um convite gera uma resposta para devolver; uma resposta conclui a troca. Depois, compare a identidade da outra pessoa.</p><form class="mt-5 space-y-4" @submit.prevent="join"><div><label for="join-invitation" class="mb-2 block text-sm font-medium">Convite ou resposta</label><textarea id="join-invitation" v-model="invitation" rows="4" class="w-full resize-none font-mono text-xs leading-relaxed" placeholder="Cole o convite ou a resposta temporária…" autocomplete="off" autocapitalize="off" :spellcheck="false" :disabled="!joinEnabled || busy" maxlength="16000" /></div><Button type="submit" :disabled="!joinEnabled || !canJoin || busy">{{ joining ? 'Conectando…' : 'Iniciar pareamento' }}<ArrowRight class="size-4" aria-hidden="true" /></Button></form></section>
  </div>
  <p v-if="!enabled" class="mt-5 text-sm text-warning">Inicie uma sessão para criar convites. Receber um convite inicia a conexão diretamente, sem configurar um servidor.</p><p v-if="error" role="alert" class="mt-5 flex gap-2 text-sm text-destructive"><CircleAlert class="size-4 shrink-0" aria-hidden="true" />{{ error }}</p><p v-if="notice" role="status" class="mt-5 text-sm text-muted-foreground">{{ notice }}</p>
</template>
