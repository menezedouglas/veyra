<script setup lang="ts">
import { Fingerprint, ShieldCheck, ShieldQuestion, UserRoundX } from '@lucide/vue'
import type { PeerSummary } from '@/lib/ipc'
import { Button } from '@/components/ui/button'
import ConnectionRouteBadge from './ConnectionRouteBadge.vue'
defineProps<{ peer: PeerSummary; busy?: boolean }>()
defineEmits<{ verify: [peer: PeerSummary]; revoke: [peer: PeerSummary]; reconnect: [peer: PeerSummary] }>()
</script>

<template>
  <article class="rounded-lg border border-border bg-card p-5">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="flex items-center gap-3"><div class="flex size-10 items-center justify-center rounded-lg bg-secondary"><Fingerprint class="size-5 text-muted-foreground" aria-hidden="true" /></div><div><h3 class="font-semibold">{{ peer.label || 'Contato' }}</h3><p class="mt-1 flex items-center gap-1.5 text-xs" :class="peer.verified ? 'text-success' : 'text-warning'"><ShieldCheck v-if="peer.verified" class="size-3.5" aria-hidden="true" /><ShieldQuestion v-else class="size-3.5" aria-hidden="true" />{{ peer.verified ? 'Identidade verificada' : 'Identidade não verificada' }}</p></div></div>
      <ConnectionRouteBadge :route="peer.route" />
    </div>
    <p class="mt-5 text-xs text-muted-foreground">Impressão digital</p><p class="mt-1 break-all font-mono text-xs leading-relaxed">{{ peer.fingerprint }}</p>
    <p v-if="peer.route === 'disconnected'" class="mt-4 text-xs leading-relaxed text-muted-foreground">Contato salvo e desconectado. Troque um novo convite e verifique a identidade para enviar mensagens.</p>
    <div class="mt-5 flex flex-wrap items-center justify-between gap-2 border-t border-border pt-4"><Button v-if="peer.route === 'disconnected'" variant="secondary" size="sm" :disabled="busy" @click="$emit('reconnect', peer)">Reconectar por convite</Button><Button v-else variant="secondary" size="sm" :disabled="busy || !peer.verificationCode" @click="$emit('verify', peer)">{{ peer.verified ? 'Conferir identidade' : 'Verificar identidade' }}</Button><Button variant="ghost" size="sm" :disabled="busy" class="text-destructive" @click="$emit('revoke', peer)"><UserRoundX class="size-3.5" aria-hidden="true" />Remover</Button></div>
  </article>
</template>
