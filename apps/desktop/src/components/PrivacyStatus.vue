<script setup lang="ts">
import { ShieldCheck, ShieldAlert } from '@lucide/vue'
import type { SessionStatus } from '@/lib/ipc'
defineProps<{ memoryProtection: SessionStatus['memoryProtection'] }>()
</script>

<template>
  <section class="rounded-lg border border-border bg-card p-5" aria-labelledby="privacy-title">
    <h2 id="privacy-title" class="mb-4 flex items-center gap-2 text-sm font-semibold"><ShieldCheck class="size-4 text-primary" aria-hidden="true" />Privacidade da sessão</h2>
    <ul class="space-y-3 text-sm text-muted-foreground">
      <li>Identidade e autorizações temporárias por execução.</li>
      <li>Contatos mantidos neste dispositivo entre execuções.</li>
      <li>Sem histórico de mensagens ou recuperação de sessão.</li>
      <li>Aceitação em RAM não garante abertura posterior.</li>
      <li class="flex items-start gap-2 text-warning"><ShieldAlert class="mt-0.5 size-4 shrink-0" aria-hidden="true" /><span>{{ memoryProtection === 'protected' ? 'Proteção nativa dos buffers confirmada pelo núcleo.' : memoryProtection === 'unavailable' ? 'Proteção nativa de memória indisponível. Operações sensíveis bloqueadas.' : 'Proteção nativa de memória ainda não verificada.' }}</span></li>
    </ul>
    <p class="mt-5 border-t border-border pt-4 text-xs leading-relaxed text-muted-foreground">O protocolo é experimental. O sistema operacional e processos privilegiados podem capturar dados. Veyra não oferece anonimato nem apagamento absoluto.</p>
  </section>
</template>
