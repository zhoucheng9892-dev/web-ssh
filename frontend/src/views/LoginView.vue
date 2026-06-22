<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { useAuthStore } from '@/stores/auth'
import { authApi } from '@/api/auth'

const auth = useAuthStore()
const router = useRouter()

const username = ref('')
const password = ref('')
const captcha = ref('')
const captchaImg = ref('')
const loading = ref(false)

async function refreshCaptcha() {
  try {
    const res = await authApi.captcha()
    captchaImg.value = res.image
  } catch (e: any) {
    // captcha endpoint failure shouldn't block the form
  }
}

async function submit() {
  if (!username.value || !password.value) {
    ElMessage.warning('请输入用户名和密码')
    return
  }
  if (!captcha.value) {
    ElMessage.warning('请输入验证码')
    return
  }
  loading.value = true
  try {
    await auth.login(username.value, password.value, captcha.value)
    router.push({ name: 'terminals' })
  } catch (e: any) {
    ElMessage.error(e.message || '登录失败')
    refreshCaptcha()
    captcha.value = ''
  } finally {
    loading.value = false
  }
}

onMounted(refreshCaptcha)
</script>

<template>
  <div class="auth-page">
    <form class="auth-card" @submit.prevent="submit">
      <h1>Web SSH</h1>
      <p class="subtitle">登录到终端控制台</p>
      <el-input
        v-model="username"
        placeholder="用户名"
        size="large"
        autocomplete="username"
      />
      <el-input
        v-model="password"
        type="password"
        placeholder="密码"
        size="large"
        show-password
        autocomplete="current-password"
        @keyup.enter="submit"
      />
      <div class="captcha-row">
        <el-input
          v-model="captcha"
          placeholder="验证码"
          size="large"
          autocomplete="off"
          @keyup.enter="submit"
        />
        <img
          v-if="captchaImg"
          :src="captchaImg"
          class="captcha-img"
          title="点击刷新"
          @click="refreshCaptcha"
        />
      </div>
      <el-button
        type="primary"
        size="large"
        :loading="loading"
        native-type="submit"
        @click="submit"
      >
        登录
      </el-button>
    </form>
  </div>
</template>

<style scoped>
.auth-page {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, #0d1117 0%, #161b22 100%);
}
.auth-card {
  width: 360px;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 36px 32px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
h1 {
  margin: 0;
  text-align: center;
  color: var(--accent);
  font-size: 28px;
}
.subtitle {
  margin: 0 0 8px;
  text-align: center;
  color: var(--muted);
  font-size: 13px;
}
.captcha-row {
  display: flex;
  gap: 10px;
  align-items: center;
}
.captcha-img {
  height: 40px;
  width: 110px;
  border-radius: 4px;
  cursor: pointer;
  flex-shrink: 0;
  border: 1px solid var(--border);
}
</style>
