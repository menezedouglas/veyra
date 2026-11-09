<script setup lang="ts">
import { X } from '@lucide/vue'
import { DialogRoot, DialogPortal, DialogOverlay, DialogContent, DialogTitle, DialogDescription, DialogClose } from 'reka-ui'
import { Button } from '@/components/ui/button'
defineProps<{ open: boolean; title: string; description: string; wide?: boolean }>()
const emit = defineEmits<{ close: [] }>()
</script>

<template>
  <DialogRoot :open="open" @update:open="value => { if (!value) emit('close') }">
    <DialogPortal>
      <DialogOverlay class="fixed inset-0 z-40 bg-black/65" />
      <DialogContent class="fixed left-1/2 top-1/2 z-50 max-h-[88vh] w-[calc(100%-2rem)] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-xl border border-border bg-card p-6 shadow-xl focus:outline-none sm:p-8" :class="wide ? 'max-w-2xl' : 'max-w-lg'">
        <div class="mb-6 pr-8"><DialogTitle class="text-xl font-semibold tracking-tight">{{ title }}</DialogTitle><DialogDescription class="mt-2 text-sm leading-relaxed text-muted-foreground">{{ description }}</DialogDescription></div>
        <DialogClose as-child><Button variant="ghost" size="icon" class="absolute right-3 top-3" aria-label="Fechar"><X class="size-4" aria-hidden="true" /></Button></DialogClose>
        <slot />
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
