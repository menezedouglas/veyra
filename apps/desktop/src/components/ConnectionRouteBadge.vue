<script setup lang="ts">
import { ArrowLeftRight, Cable, CircleHelp, Unplug } from '@lucide/vue'
import { computed } from 'vue'
import type { Route } from '@/lib/ipc'
const props = defineProps<{ route: Route }>()
const routeInfo = computed(() => ({
  direct: { label: 'P2P direto', icon: Cable, style: 'text-route-direct' },
  relay: { label: 'via relay', icon: ArrowLeftRight, style: 'text-route-relay' },
  disconnected: { label: 'Sem conexão', icon: Unplug, style: 'text-muted-foreground' },
  unknown: { label: 'Rota não verificada', icon: CircleHelp, style: 'text-muted-foreground' },
})[props.route])
</script>

<template>
  <span class="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-xs font-medium" :class="routeInfo.style"><component :is="routeInfo.icon" class="size-3.5" aria-hidden="true" />{{ routeInfo.label }}</span>
</template>
