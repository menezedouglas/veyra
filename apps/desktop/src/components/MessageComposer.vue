<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import { Send, CircleAlert, CheckCheck, UsersRound } from '@lucide/vue'
import { api, type Delivery, type PeerSummary } from '@/lib/ipc'
import { validateText, MAX_TEXT_BYTES } from '@/lib/validation'
import { Button } from '@/components/ui/button'
import VeyraDialog from './ui/dialog/VeyraDialog.vue'
import ConnectionRouteBadge from './ConnectionRouteBadge.vue'
const props = defineProps<{ peers: PeerSummary[]; initialPeerId?: string; deliveryUpdates?: Delivery[] }>()
const emit = defineEmits<{ close: []; sent: [] }>()
const text = shallowRef('')
const selected = ref<string[]>(props.initialPeerId ? [props.initialPeerId] : [])
const busy = ref(false)
const error = ref('')
const deliveries = shallowRef<Delivery[]>([])
const updates = shallowRef<{ value: Delivery; expiresAt: number }[]>([])
let resultsExpireAt = 0
let interval: ReturnType<typeof setInterval> | undefined
const currentDeliveries = computed(() => deliveries.value.map(delivery => {
  if (!delivery.messageId) return delivery
  return updates.value.find(update => update.value.messageId === delivery.messageId && update.value.peerId === delivery.peerId)?.value ?? delivery
}))
watch(() => props.deliveryUpdates, values => {
  const now = performance.now()
  const next = updates.value.filter(update => update.expiresAt > now)
  for (const value of values ?? []) {
    if (!value.messageId) continue
    const index = next.findIndex(update => update.value.messageId === value.messageId && update.value.peerId === value.peerId)
    const expiresAt = index >= 0 ? next[index]!.expiresAt : now + 300000
    if (index >= 0) next.splice(index, 1)
    next.push({ value, expiresAt })
  }
  updates.value = next.slice(-128)
}, { immediate: true })
const verifiedPeers = computed(() => props.peers.filter(peer => peer.verified))
const bytes = computed(() => new TextEncoder().encode(text.value).length)
const validation = computed(() => validateText(text.value))
const labels: Record<Delivery['state'], string> = {
  sealed: 'Cifrada · aguardando conexão', sent: 'Enviada · aguardando confirmação',
  acknowledged: 'Recebida em RAM', 'delivery-unconfirmed': 'Entrega não confirmada', rejected: 'Envio recusado',
}
async function send() {
  if (busy.value || validation.value || !selected.value.length || selected.value.length > 16) return
  if (selected.value.some(id => !verifiedPeers.value.some(peer => peer.id === id))) { error.value = 'Selecione somente identidades verificadas.'; return }
  busy.value = true
  error.value = ''
  deliveries.value = []
  // invoke serializes the indispensable local transfer. Clear our composition
  // reference immediately; there is no autosave, retry of plaintext or recovery.
  const result = api.sendMessage([...selected.value], text.value)
  text.value = ''
  try { deliveries.value = await result; resultsExpireAt = performance.now() + 300000; emit('sent') }
  catch { error.value = 'O núcleo recusou o envio ou a confirmação não pôde ser obtida. O texto foi descartado; redija novamente se necessário.' }
  finally { busy.value = false }
}
onMounted(() => {
  interval = setInterval(() => {
    const now = performance.now()
    updates.value = updates.value.filter(update => update.expiresAt > now)
    if (resultsExpireAt > 0 && now >= resultsExpireAt) deliveries.value = []
  }, 1000)
})
onUnmounted(() => { text.value = ''; selected.value = []; deliveries.value = []; updates.value = []; if (interval) clearInterval(interval) })
</script>

<template>
  <VeyraDialog :open="true" title="Nova mensagem" description="Texto efêmero para até 16 destinatários verificados. Cada pessoa recebe um envelope independente." wide @close="emit('close')">
    <form @submit.prevent="send">
      <fieldset :disabled="busy"><legend class="mb-3 flex items-center gap-2 text-sm font-semibold"><UsersRound class="size-4 text-muted-foreground" aria-hidden="true" />Destinatários</legend><div class="max-h-44 space-y-2 overflow-y-auto"><label v-for="peer in verifiedPeers" :key="peer.id" class="flex cursor-pointer items-center gap-3 rounded-md border border-border p-3"><input v-model="selected" type="checkbox" :value="peer.id" class="size-4 accent-primary" :disabled="busy || (!selected.includes(peer.id) && selected.length >= 16)" /><span class="min-w-0 flex-1 truncate text-sm">{{ peer.label || 'Contato da sessão' }}</span><ConnectionRouteBadge :route="peer.route" /></label><p v-if="!verifiedPeers.length" class="text-sm text-muted-foreground">Verifique uma identidade antes de enviar.</p></div></fieldset>
      <label for="message-text" class="mb-2 mt-6 block text-sm font-semibold">Mensagem</label><textarea id="message-text" v-model="text" rows="6" class="w-full resize-y leading-relaxed" placeholder="Escreva uma mensagem para esta sessão…" autocomplete="off" autocapitalize="off" :spellcheck="false" :disabled="busy" aria-describedby="message-limit message-policy" />
      <div id="message-limit" class="mt-2 flex items-center justify-between gap-3 text-xs"><span :class="bytes > MAX_TEXT_BYTES ? 'text-destructive' : 'text-muted-foreground'">{{ bytes }} / {{ MAX_TEXT_BYTES }} bytes UTF-8</span><span class="text-muted-foreground">{{ selected.length }} selecionado(s)</span></div>
      <p id="message-policy" class="mt-4 text-xs leading-relaxed text-muted-foreground">Até cinco minutos, limitados à validade da oferta descartável. Sem oferta válida, o envio é recusado. “Recebida em RAM” não significa leitura nem armazenamento durável. O texto não pode ser recuperado após o envio.</p>
      <p v-if="error" role="alert" class="mt-4 flex gap-2 text-sm leading-relaxed text-destructive"><CircleAlert class="mt-0.5 size-4 shrink-0" aria-hidden="true" />{{ error }}</p>
      <ul v-if="currentDeliveries.length" class="mt-4 space-y-2 rounded-md border border-border p-3" aria-label="Resultado do envio"><li v-for="delivery in currentDeliveries" :key="delivery.messageId || delivery.peerId" class="text-sm"><span class="font-medium">{{ peers.find(peer => peer.id === delivery.peerId)?.label || 'Destinatário' }}</span><span class="ml-2" :class="delivery.state === 'rejected' || delivery.state === 'delivery-unconfirmed' ? 'text-warning' : 'text-success'">{{ labels[delivery.state] }}</span><p v-if="delivery.reason" class="mt-1 text-xs text-muted-foreground">{{ delivery.reason }}</p></li></ul>
      <div class="mt-6 flex items-center justify-end gap-3"><Button type="button" variant="secondary" @click="emit('close')">Fechar</Button><Button type="submit" :disabled="busy || !!validation || !selected.length || selected.length > 16"><CheckCheck v-if="deliveries.length" class="size-4" aria-hidden="true" /><Send v-else class="size-4" aria-hidden="true" />{{ busy ? 'Cifrando e enviando…' : 'Cifrar e enviar' }}</Button></div>
    </form>
  </VeyraDialog>
</template>
