<script setup lang="ts">
withDefaults(defineProps<{ symbolOnly?: boolean }>(), { symbolOnly: false })
</script>

<template>
  <span class="inline-flex items-center gap-2.5" aria-label="Veyra">
    <svg viewBox="0 0 96 96" fill="none" class="size-10 shrink-0" aria-hidden="true"><path d="M12 20h18l18 36-9 18L12 20Z" fill="var(--primary)"/><path d="M66 20h18L57 74H39l27-54Z" fill="var(--primary)" opacity=".68"/></svg>
    <span v-if="!symbolOnly" class="text-[27px] leading-none font-semibold tracking-[-1px]">Veyra</span>
  </span>
</template>
