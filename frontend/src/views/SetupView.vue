<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const router = useRouter()

const username = ref('admin')
const password = ref('')
const confirm = ref('')
const loading = ref(false)

async function submit() {
  if (password.value.length < 6) {
    ElMessage.warning('密码至少 6 位')
    return
  }
  if (password.value !== confirm.value) {
    ElMessage.warning('两次密码不一致')
    return
  }
  loading.value = true
  try {
    await auth.setup(username.value, password.value)
    router.push({ name: 'terminals' })
  } catch (e: any) {
    ElMessage.error(e.message || '初始化失败')
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="auth-page">
    <form class="auth-card" @submit.prevent="submit">
      <h1>初始化</h1>
      <p class="subtitle">创建管理员账号以开始使用 Web SSH</p>
      <el-input v-model="username" placeholder="管理员用户名" size="large" />
      <el-input
        v-model="password"
        type="password"
        placeholder="密码（至少 6 位）"
        size="large"
        show-password
      />
      <el-input
        v-model="confirm"
        type="password"
        placeholder="确认密码"
        size="large"
        show-password
        @keyup.enter="submit"
      />
      <el-button
        type="primary"
        size="large"
        :loading="loading"
        native-type="submit"
        @click="submit"
      >
        创建账号
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
  font-size: 24px;
}
.subtitle {
  margin: 0 0 8px;
  text-align: center;
  color: var(--muted);
  font-size: 13px;
}
</style>
