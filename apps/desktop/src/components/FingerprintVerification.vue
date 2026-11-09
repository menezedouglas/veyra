<script setup lang="ts">
import { ref } from 'vue'
import { ShieldCheck, CircleAlert } from '@lucide/vue'
import type { PeerSummary } from '@/lib/ipc'
import { api } from '@/lib/ipc'
import { Button } from '@/components/ui/button'
import VeyraDialog from './ui/dialog/VeyraDialog.vue'
const props = defineProps<{ peer: PeerSummary }>()
const emit = defineEmits<{ close: []; verified: [] }>()
const compared = ref(false)
const busy = ref(false)
const error = ref('')
async function approve() {
  if (!compared.value || !props.peer.verificationCode || busy.value) return
  busy.value = true
  try { await api.verifyPeer(props.peer.id, props.peer.verificationCode); emit('verified'); emit('close') }
  catch { error.value = 'A verificação foi recusada pelo núcleo. Confira a conexão e a identidade atual.' }
  finally { busy.value = false }
}
</script>

<template>
  <VeyraDialog :open="true" title="Verificar identidade" description="Compare a impressão digital completa e o código com a outra pessoa por um canal independente antes de aprovar." @close="emit('close')">
    <p class="font-semibold">{{ peer.label || 'Contato da sessão' }}</p><p class="mt-2 break-all rounded-md border border-border bg-background p-3 font-mono text-xs leading-relaxed">{{ peer.fingerprint }}</p>
    <div class="my-5 rounded-lg border border-primary/30 bg-primary/5 p-5"><p class="text-xs font-medium text-muted-foreground">Código desta conexão</p><p class="mt-3 break-all font-mono text-lg font-semibold tracking-wider">{{ peer.verificationCode || 'Aguardando handshake autenticado' }}</p></div>
    <p class="text-xs leading-relaxed text-muted-foreground">O nome é um rótulo local. Um documento autossinado confirma a consistência das chaves; a comparação identifica a pessoa. Reiniciar o núcleo exige novo pareamento.</p>
    <template v-if="!peer.verified"><label class="mt-5 flex cursor-pointer items-start gap-3 text-sm leading-relaxed"><input v-model="compared" type="checkbox" class="mt-1 size-4 accent-primary" :disabled="!peer.verificationCode || busy" /><span>Comparei o código e a impressão digital completa com a outra pessoa.</span></label><p v-if="error" class="mt-4 flex gap-2 text-sm text-destructive" role="alert"><CircleAlert class="size-4 shrink-0" aria-hidden="true" />{{ error }}</p><Button class="mt-6 w-full" :disabled="!compared || !peer.verificationCode || busy" @click="approve"><ShieldCheck class="size-4" aria-hidden="true" />{{ busy ? 'Verificando…' : 'Aprovar identidade' }}</Button></template>
    <p v-else class="mt-5 flex items-center gap-2 text-sm text-success"><ShieldCheck class="size-4" aria-hidden="true" />Identidade aprovada nesta execução.</p>
  </VeyraDialog>
</template>
