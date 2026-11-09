<script setup lang="ts">
import { Clock3 } from '@lucide/vue'
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { formatRemaining } from '@/lib/validation'
const props = defineProps<{ expiresAt: number }>()
const emit = defineEmits<{ expired: [] }>()
const now = ref(performance.now())
let interval: ReturnType<typeof setInterval> | undefined
let notified = false
const remaining = computed(() => Math.max(0, props.expiresAt - now.value))
function tick() {
  now.value = performance.now()
  if (remaining.value <= 0 && !notified) { notified = true; emit('expired') }
}
onMounted(() => { tick(); interval = setInterval(tick, 500) })
onUnmounted(() => { if (interval) clearInterval(interval) })
</script>

<template>
  <span class="inline-flex items-center gap-1.5 text-xs" :class="remaining < 30000 ? 'text-warning' : 'text-muted-foreground'"><Clock3 class="size-3.5" aria-hidden="true" />{{ remaining > 0 ? `Expira em ${formatRemaining(remaining)}` : 'Expirou' }}</span>
</template>
