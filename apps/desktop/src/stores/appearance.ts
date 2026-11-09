import { defineStore } from 'pinia'
import { ref } from 'vue'

// Somente preferências visuais efêmeras. Esta store não recebe dados da sessão.
export const useAppearance = defineStore('appearance', () => {
  const theme = ref<'dark' | 'light'>('dark')
  function toggleTheme() {
    theme.value = theme.value === 'dark' ? 'light' : 'dark'
    document.documentElement.dataset.theme = theme.value
  }
  return { theme, toggleTheme }
})
