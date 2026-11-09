<script setup lang="ts">
import { CircleCheck, CircleMinus, CircleAlert } from '@lucide/vue'
import type { SessionStatus } from '@/lib/ipc'
import ConnectionRouteBadge from './ConnectionRouteBadge.vue'
defineProps<{ status: SessionStatus | null; desktop: boolean }>()
</script>

<template>
  <section class="rounded-lg border border-border bg-card p-5" aria-labelledby="connection-title">
    <h2 id="connection-title" class="mb-5 text-sm font-semibold">Diagnóstico de conexão</h2>
    <dl class="space-y-4 text-sm">
      <div class="flex items-center justify-between gap-3"><dt class="text-muted-foreground">Núcleo Rust</dt><dd class="flex items-center gap-1.5"><CircleCheck v-if="desktop && status" class="size-4 text-success" aria-hidden="true" /><CircleMinus v-else class="size-4 text-warning" aria-hidden="true" />{{ desktop ? (status ? 'Disponível' : 'Aguardando resposta') : 'Indisponível no navegador' }}</dd></div>
      <div class="flex items-center justify-between gap-3"><dt class="text-muted-foreground">Configuração de conexão</dt><dd>{{ status ? (status.connectionSetup?.configured ? 'Configurada' : 'Não configurada') : 'Não verificada' }}</dd></div>
      <div v-if="status?.connectionSetup?.source === 'direct'" class="flex items-center justify-between gap-3"><dt class="text-muted-foreground">Modo</dt><dd>Direto · sem servidor</dd></div>
      <div class="flex items-center justify-between gap-3"><dt class="text-muted-foreground">Identidades verificadas</dt><dd>{{ status?.peers.filter(peer => peer.verified).length ?? 0 }}</dd></div>
    </dl>
    <div class="mt-5 border-t border-border pt-4"><h3 class="mb-3 text-xs font-semibold text-muted-foreground">Rotas por conexão</h3><ul v-if="status?.peers.length" class="space-y-3"><li v-for="peer in status.peers" :key="peer.id" class="flex flex-wrap items-center justify-between gap-2"><span class="min-w-0 truncate text-sm">{{ peer.label || 'Contato da sessão' }}</span><ConnectionRouteBadge :route="peer.route" /></li></ul><p v-else class="text-xs text-muted-foreground">Nenhum transporte estabelecido.</p></div>
    <p v-if="status?.reason || !status" class="mt-5 flex gap-2 border-t border-border pt-4 text-xs leading-relaxed text-warning"><CircleAlert class="size-4 shrink-0" aria-hidden="true" /><span>{{ status?.reason || (desktop ? 'Aguardando diagnóstico do núcleo.' : 'Diagnóstico nativo indisponível no navegador.') }}</span></p>
    <p class="mt-4 text-xs leading-relaxed text-muted-foreground">O modo direto usa troca manual de convite e resposta, sem sinalização, STUN ou TURN externos. Os endereços precisam ser acessíveis entre os participantes; NAT restritivo e firewalls podem impedir a conexão. O transporte confirma a rota após o pareamento.</p>
  </section>
</template>
