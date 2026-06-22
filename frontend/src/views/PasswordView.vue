<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { authApi } from '@/api/auth'

const oldPassword = ref('')
const newPassword = ref('')
const confirm = ref('')
const loading = ref(false)

async function submit() {
  if (newPassword.value.length < 6) {
    ElMessage.warning('新密码至少 6 位')
    return
  }
  if (newPassword.value !== confirm.value) {
    ElMessage.warning('两次新密码不一致')
    return
  }
  loading.value = true
  try {
    await authApi.changePassword(oldPassword.value, newPassword.value)
    ElMessage.success('密码已修改')
    oldPassword.value = ''
    newPassword.value = ''
    confirm.value = ''
  } catch (e: any) {
    ElMessage.error(e.message || '修改失败')
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="password-view">
    <div class="card">
      <h2>修改密码</h2>
      <el-form label-width="90px" label-position="left" @submit.prevent="submit">
        <el-form-item label="原密码">
          <el-input v-model="oldPassword" type="password" show-password size="large" />
        </el-form-item>
        <el-form-item label="新密码">
          <el-input
            v-model="newPassword"
            type="password"
            show-password
            size="large"
            placeholder="至少 6 位"
          />
        </el-form-item>
        <el-form-item label="确认新密码">
          <el-input
            v-model="confirm"
            type="password"
            show-password
            size="large"
            @keyup.enter="submit"
          />
        </el-form-item>
        <el-form-item>
          <el-button type="primary" :loading="loading" @click="submit">保存</el-button>
        </el-form-item>
      </el-form>
    </div>
  </div>
</template>

<style scoped>
.password-view {
  height: 100%;
  padding: 24px;
  background: var(--bg);
  overflow: auto;
}
.card {
  max-width: 460px;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 28px 32px;
}
h2 {
  margin: 0 0 20px;
  font-size: 16px;
}
</style>
