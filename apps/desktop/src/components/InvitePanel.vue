<script setup lang="ts">
import { computed, onUnmounted, ref, shallowRef } from 'vue'
import { ArrowRight, Copy, TicketPlus, CircleAlert, Link2 } from '@lucide/vue'
import { api, type Invite } from '@/lib/ipc'
import { Button } from '@/components/ui/button'
import ExpiryIndicator from './ExpiryIndicator.vue'
defineProps<{ enabled: boolean }>()
const emit = defineEmits<{ joined: []; refresh: [] }>()
const generated = shallowRef<(Invite & { expiresAt: number }) | null>(null)
const invitation = shallowRef('')
const busy = ref(false)
const error = ref('')
const notice = ref('')
let alive = true
const canJoin = computed(() => invitation.value.trim().length > 0 && invitation.value.length <= 4096)
async function create() {
  if (busy.value) return
  busy.value = true; error.value = ''; notice.value = ''
  const startedAt = performance.now()
  try {
    const value = await api.createInvite()
    if (alive) generated.value = { ...value, expiresAt: startedAt + Math.max(0, Math.min(value.expiresInMs, 300000)) }
  } catch { error.value = 'Não foi possível criar um convite. Confira o operador e o estado da sessão.' }
  finally { busy.value = false }
}
async function join() {
  if (!canJoin.value || busy.value) return
  busy.value = true; error.value = ''; notice.value = ''
  const result = api.joinInvite(invitation.value.trim(), '')
  invitation.value = ''
  try { await result; if (alive) { notice.value = 'Pareamento iniciado. Aguarde a conexão e verifique a identidade.'; emit('joined'); emit('refresh') } }
  catch { if (alive) error.value = 'O convite foi recusado ou a conexão não pôde ser iniciada. Confira validade, operador e identidade esperada.' }
  finally { busy.value = false }
}
async function copyInvite() {
  if (!generated.value) return
  try { await navigator.clipboard.writeText(generated.value.token); notice.value = 'Convite copiado por sua solicitação. O histórico externo do clipboard pode conservar uma cópia.' }
  catch { notice.value = 'Não foi possível copiar automaticamente. Selecione o convite e use o atalho de cópia, se desejar.' }
}
function expire() { generated.value = null; notice.value = 'O convite exibido expirou.' }
onUnmounted(() => { alive = false; invitation.value = ''; generated.value = null })
</script>

<template>
  <div class="grid items-start gap-6 xl:grid-cols-2">
    <section class="rounded-lg border border-border bg-card p-6"><div class="mb-5 flex size-11 items-center justify-center rounded-lg bg-primary/10"><TicketPlus class="size-5 text-primary" aria-hidden="true" /></div><h2 class="text-lg font-semibold">Convidar uma pessoa</h2><p class="mt-2 text-sm leading-relaxed text-muted-foreground">Gere um convite temporário e compartilhe por um canal de confiança. Ele é consumido após um pareamento bem-sucedido.</p><template v-if="generated"><div class="mt-5 flex flex-wrap items-center justify-between gap-2"><span class="text-xs font-semibold">Convite temporário</span><ExpiryIndicator :expires-at="generated.expiresAt" @expired="expire" /></div><textarea :value="generated.token" readonly rows="5" class="mt-3 w-full resize-none break-all font-mono text-xs leading-relaxed" aria-label="Convite temporário para compartilhar" autocomplete="off" :spellcheck="false" /><p class="mt-3 text-xs leading-relaxed text-warning">Copiar coloca o convite no clipboard do sistema, cujo histórico ou sincronização pode preservar cópias fora do Veyra.</p><Button variant="secondary" class="mt-4" @click="copyInvite"><Copy class="size-4" aria-hidden="true" />Copiar convite</Button></template><Button v-else class="mt-6" :disabled="!enabled || busy" @click="create"><TicketPlus class="size-4" aria-hidden="true" />{{ busy ? 'Criando…' : 'Criar convite' }}</Button><p class="mt-5 text-xs leading-relaxed text-muted-foreground">Convites compartilhados continuam sujeitos ao prazo original e ao encerramento da sessão. Não são links de navegador.</p></section>
    <section class="rounded-lg border border-border bg-card p-6"><div class="mb-5 flex size-11 items-center justify-center rounded-lg bg-secondary"><Link2 class="size-5 text-muted-foreground" aria-hidden="true" /></div><h2 class="text-lg font-semibold">Entrar com convite</h2><p class="mt-2 text-sm leading-relaxed text-muted-foreground">Cole o código recebido. O serviço de encontro não comprova a identidade humana da outra pessoa.</p><form class="mt-5 space-y-4" @submit.prevent="join"><div><label for="join-invitation" class="mb-2 block text-sm font-medium">Convite</label><textarea id="join-invitation" v-model="invitation" rows="4" class="w-full resize-none font-mono text-xs leading-relaxed" placeholder="Cole o convite temporário…" autocomplete="off" autocapitalize="off" :spellcheck="false" :disabled="!enabled || busy" maxlength="4096" /></div><Button type="submit" :disabled="!enabled || !canJoin || busy">{{ busy ? 'Conectando…' : 'Iniciar pareamento' }}<ArrowRight class="size-4" aria-hidden="true" /></Button></form></section>
  </div>
  <p v-if="!enabled" class="mt-5 text-sm text-warning">Inicie uma sessão com infraestrutura WSS configurada para usar convites.</p><p v-if="error" role="alert" class="mt-5 flex gap-2 text-sm text-destructive"><CircleAlert class="size-4 shrink-0" aria-hidden="true" />{{ error }}</p><p v-if="notice" role="status" class="mt-5 text-sm text-muted-foreground">{{ notice }}</p>
</template>
