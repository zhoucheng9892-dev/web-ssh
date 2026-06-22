import { defineStore } from 'pinia'
import { ref } from 'vue'
import { authApi, type Me } from '@/api/auth'

export const useAuthStore = defineStore('auth', () => {
  const user = ref<Me | null>(null)
  const needsSetup = ref(false)
  const loading = ref(true)

  /** Probe status + current user on app start. */
  async function init() {
    loading.value = true
    try {
      const status = await authApi.status()
      needsSetup.value = status.needs_setup
      if (!status.needs_setup) {
        try {
          user.value = await authApi.me()
        } catch {
          user.value = null
        }
      }
    } finally {
      loading.value = false
    }
  }

  async function login(username: string, password: string, captcha: string) {
    user.value = await authApi.login(username, password, captcha)
    needsSetup.value = false
  }

  async function setup(username: string, password: string) {
    user.value = await authApi.setup(username, password)
    needsSetup.value = false
  }

  async function logout() {
    await authApi.logout()
    user.value = null
  }

  return { user, needsSetup, loading, init, login, setup, logout }
})
