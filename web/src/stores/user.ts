import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useUserStore = defineStore('user', () => {
  const token = ref(localStorage.getItem('token') || '')
  const username = ref(localStorage.getItem('username') || '')
  const theme = ref(localStorage.getItem('theme') || 'default')

  function setLogin(t: string, u: string) {
    token.value = t
    username.value = u
    localStorage.setItem('token', t)
    localStorage.setItem('username', u)
  }

  function logout() {
    token.value = ''
    username.value = ''
    localStorage.removeItem('token')
    localStorage.removeItem('username')
  }

  function setTheme(t: string) {
    theme.value = t
    localStorage.setItem('theme', t)
    document.documentElement.setAttribute('data-theme', t === 'default' ? '' : t)
  }

  // Init theme on load
  if (theme.value && theme.value !== 'default') {
    document.documentElement.setAttribute('data-theme', theme.value)
  }

  return { token, username, theme, setLogin, logout, setTheme }
})