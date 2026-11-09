<script setup lang="ts">
import { Eye, Trash2, ShieldCheck, ShieldQuestion } from '@lucide/vue'
import type { PendingMessage, PeerSummary } from '@/lib/ipc'
import { Button } from '@/components/ui/button'
import ExpiryIndicator from './ExpiryIndicator.vue'
defineProps<{ message: PendingMessage & { expiresAt: number }; peer?: PeerSummary; busy?: boolean }>()
defineEmits<{ open: []; discard: []; expired: [] }>()
</script>

<template>
  <article class="rounded-lg border border-border bg-card p-5">
    <div class="flex flex-wrap items-center justify-between gap-3"><div class="flex items-center gap-3"><div class="flex size-10 items-center justify-center rounded-lg border border-primary/25 bg-primary/5"><Eye class="size-5 text-primary" aria-hidden="true" /></div><div><h3 class="font-semibold">{{ peer?.label || 'Contato desta sessão' }}</h3><p class="mt-1 text-xs text-muted-foreground">Mensagem cifrada · leitura única</p></div></div><ExpiryIndicator :expires-at="message.expiresAt" @expired="$emit('expired')" /></div>
    <div class="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4"><span class="flex items-center gap-1.5 text-xs" :class="peer?.verified ? 'text-success' : 'text-warning'"><ShieldCheck v-if="peer?.verified" class="size-3.5" aria-hidden="true" /><ShieldQuestion v-else class="size-3.5" aria-hidden="true" />{{ peer?.verified ? 'Remetente verificado' : 'Remetente não verificado' }}</span><div class="flex gap-2"><Button variant="ghost" size="sm" :disabled="busy" @click="$emit('discard')"><Trash2 class="size-3.5" aria-hidden="true" />Descartar</Button><Button variant="secondary" size="sm" :disabled="busy || !peer?.verified" @click="$emit('open')"><Eye class="size-3.5" aria-hidden="true" />Abrir uma vez</Button></div></div>
  </article>
</template>
