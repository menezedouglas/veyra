<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef } from 'vue'
import { CircleAlert, Eye, EyeOff } from '@lucide/vue'
import { api } from '@/lib/ipc'
import { formatRemaining } from '@/lib/validation'
import { Button } from '@/components/ui/button'
import VeyraDialog from './ui/dialog/VeyraDialog.vue'
const props = defineProps<{ messageId: string; expiresAt: number }>()
const emit = defineEmits<{ close: []; consumed: [] }>()
const phase = ref<'confirm' | 'opening' | 'displaying' | 'destroyed'>('confirm')
const body = shallowRef<string | null>(null)
const error = ref('')
const now = ref(performance.now())
let displayDeadline = 0
let alive = true
let interrupted = false
let interval: ReturnType<typeof setInterval> | undefined
let clockMonoBaseline = performance.now()
let clockWallBaseline = Date.now()
const remaining = computed(() => Math.max(0, displayDeadline - now.value))
function clockDiscontinuous(mono = performance.now(), wall = Date.now()) {
  // Wall time only invalidates an uncertain budget; it never grants more time.
  return Math.abs((wall - clockWallBaseline) - (mono - clockMonoBaseline)) > 1000
}
function destroy() {
  interrupted = true
  body.value = null
  if (phase.value !== 'confirm') phase.value = 'destroyed'
  emit('close')
}
function onVisibility() { if (document.visibilityState !== 'visible') destroy() }
async function open() {
  const startedAt = performance.now()
  const startedWall = Date.now()
  if (phase.value !== 'confirm' || props.expiresAt <= startedAt) return
  if (clockDiscontinuous(startedAt, startedWall)) { destroy(); return }
  clockMonoBaseline = startedAt
  clockWallBaseline = startedWall
  phase.value = 'opening'
  // This first local deadline never increases, including during IPC latency.
  displayDeadline = Math.min(props.expiresAt, startedAt + 30000)
  emit('consumed')
  try {
    const response = await api.openOnce(props.messageId)
    displayDeadline = Math.min(displayDeadline, startedAt + Math.max(0, response.displayForMs))
    if (!alive || interrupted || document.visibilityState !== 'visible' || !document.hasFocus() || clockDiscontinuous() || performance.now() >= displayDeadline) return
    body.value = response.body
    phase.value = 'displaying'
  } catch {
    if (alive) { error.value = 'Não foi possível abrir. A autorização pode já ter sido consumida; não há recuperação.'; phase.value = 'destroyed' }
  } finally {
    if (alive && phase.value === 'opening') destroy()
  }
}
function tick() {
  now.value = performance.now()
  if (clockDiscontinuous(now.value) || now.value >= props.expiresAt || (displayDeadline > 0 && now.value >= displayDeadline)) destroy()
}
onMounted(() => {
  window.addEventListener('blur', destroy)
  document.addEventListener('visibilitychange', onVisibility)
  interval = setInterval(tick, 100)
})
onUnmounted(() => {
  alive = false
  interrupted = true
  body.value = null
  if (interval) clearInterval(interval)
  window.removeEventListener('blur', destroy)
  document.removeEventListener('visibilitychange', onVisibility)
})
</script>

<template>
  <VeyraDialog :open="true" :title="phase === 'displaying' ? 'Leitura única' : 'Abrir uma vez?'" description="A abertura consome a autorização. O conteúdo não poderá ser aberto novamente neste cliente." wide @close="destroy">
    <template v-if="phase === 'confirm'"><div class="flex gap-3 rounded-lg border border-warning/35 bg-warning/5 p-4 text-sm leading-relaxed"><EyeOff class="mt-0.5 size-5 shrink-0 text-warning" aria-hidden="true" /><p>Ao continuar, a chave será consumida antes de liberar o texto. Se ocorrer uma falha, a mensagem poderá ser perdida. O conteúdo será ocultado ao perder foco ou em até 30 segundos.</p></div><div class="mt-6 flex justify-end gap-3"><Button variant="secondary" @click="destroy">Cancelar</Button><Button @click="open"><Eye class="size-4" aria-hidden="true" />Consumir e abrir</Button></div></template>
    <p v-else-if="phase === 'opening'" role="status" class="text-sm text-muted-foreground">Autorizando abertura no cofre…</p>
    <template v-else-if="phase === 'displaying'"><div class="mb-4 flex items-center justify-between gap-3 text-xs text-warning"><span>Ocultada ao perder foco</span><span>Fecha em {{ formatRemaining(remaining) }}</span></div><pre class="max-h-[45vh] overflow-y-auto whitespace-pre-wrap break-words rounded-lg border border-border bg-background p-5 font-sans text-base leading-relaxed" aria-label="Conteúdo da mensagem">{{ body }}</pre><Button class="mt-5 w-full" variant="secondary" @click="destroy"><EyeOff class="size-4" aria-hidden="true" />Fechar e descartar visualização</Button></template>
    <p v-else role="alert" class="flex gap-2 text-sm leading-relaxed text-destructive"><CircleAlert class="size-5 shrink-0" aria-hidden="true" />{{ error || 'A visualização foi encerrada.' }}</p>
  </VeyraDialog>
</template>
