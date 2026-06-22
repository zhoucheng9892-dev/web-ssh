<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox, type FormInstance } from 'element-plus'
import { usersApi, type UserInfo } from '@/api/users'

const list = ref<UserInfo[]>([])
const loading = ref(false)

// Create dialog
const createVisible = ref(false)
const createForm = reactive({ username: '', password: '', is_admin: false })

// Reset password dialog
const resetVisible = ref(false)
const resetTarget = ref<UserInfo | null>(null)
const resetPassword = ref('')

async function load() {
  loading.value = true
  try {
    list.value = await usersApi.list()
  } catch (e: any) {
    ElMessage.error(e.message)
  } finally {
    loading.value = false
  }
}

function openCreate() {
  createForm.username = ''
  createForm.password = ''
  createForm.is_admin = false
  createVisible.value = true
}

async function doCreate() {
  if (createForm.username.trim().length < 3) {
    ElMessage.warning('用户名至少 3 位')
    return
  }
  if (createForm.password.length < 6) {
    ElMessage.warning('密码至少 6 位')
    return
  }
  try {
    await usersApi.create(createForm.username.trim(), createForm.password, createForm.is_admin)
    ElMessage.success('已创建')
    createVisible.value = false
    load()
  } catch (e: any) {
    ElMessage.error(e.message)
  }
}

async function freeze(u: UserInfo) {
  try {
    await ElMessageBox.confirm(`冻结「${u.username}」？该用户将被强制下线且无法登录。`, '确认', {
      type: 'warning',
    })
  } catch {
    return
  }
  try {
    await usersApi.freeze(u.id)
    ElMessage.success('已冻结')
    load()
  } catch (e: any) {
    ElMessage.error(e.message)
  }
}

async function unfreeze(u: UserInfo) {
  try {
    await usersApi.unfreeze(u.id)
    ElMessage.success('已解冻')
    load()
  } catch (e: any) {
    ElMessage.error(e.message)
  }
}

async function toggleAdmin(u: UserInfo) {
  const action = u.is_admin ? '取消管理员' : '设为管理员'
  try {
    await ElMessageBox.confirm(`${action}「${u.username}」？`, '确认', { type: 'warning' })
  } catch {
    return
  }
  try {
    await usersApi.update(u.id, !u.is_admin)
    ElMessage.success('已更新')
    load()
  } catch (e: any) {
    ElMessage.error(e.message)
  }
}

async function remove(u: UserInfo) {
  try {
    await ElMessageBox.confirm(`删除用户「${u.username}」？此操作不可恢复。`, '确认', {
      type: 'warning',
    })
  } catch {
    return
  }
  try {
    await usersApi.remove(u.id)
    ElMessage.success('已删除')
    load()
  } catch (e: any) {
    ElMessage.error(e.message)
  }
}

function openReset(u: UserInfo) {
  resetTarget.value = u
  resetPassword.value = ''
  resetVisible.value = true
}

async function doReset() {
  if (!resetTarget.value) return
  if (resetPassword.value.length < 6) {
    ElMessage.warning('密码至少 6 位')
    return
  }
  try {
    await usersApi.resetPassword(resetTarget.value.id, resetPassword.value)
    ElMessage.success('密码已重置')
    resetVisible.value = false
  } catch (e: any) {
    ElMessage.error(e.message)
  }
}

onMounted(load)
</script>

<template>
  <div class="users-view">
    <header class="toolbar">
      <h2>用户管理</h2>
      <el-button type="primary" @click="openCreate">+ 新建用户</el-button>
    </header>

    <div class="table-wrap">
      <el-table v-loading="loading" :data="list" stripe>
        <el-table-column prop="id" label="ID" width="60" />
        <el-table-column prop="username" label="用户名" min-width="140" />
        <el-table-column label="角色" width="100">
          <template #default="{ row }">
            <el-tag v-if="row.is_admin" type="warning" size="small">管理员</el-tag>
            <el-tag v-else type="info" size="small">普通</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="100">
          <template #default="{ row }">
            <el-tag v-if="row.is_disabled" type="danger" size="small">已冻结</el-tag>
            <el-tag v-else type="success" size="small">正常</el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="created_at" label="创建时间" min-width="170" />
        <el-table-column label="操作" width="320" fixed="right">
          <template #default="{ row }">
            <el-button size="small" text @click="openReset(row)">重置密码</el-button>
            <el-button size="small" text @click="toggleAdmin(row)">
              {{ row.is_admin ? '取消管理员' : '设为管理员' }}
            </el-button>
            <el-button
              v-if="!row.is_disabled"
              size="small"
              text
              type="warning"
              @click="freeze(row)"
            >冻结</el-button>
            <el-button v-else size="small" text type="success" @click="unfreeze(row)">解冻</el-button>
            <el-button size="small" text type="danger" @click="remove(row)">删除</el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <!-- Create dialog -->
    <el-dialog v-model="createVisible" title="新建用户" width="440px">
      <el-form label-width="80px" label-position="left">
        <el-form-item label="用户名">
          <el-input v-model="createForm.username" placeholder="3-32 字符" />
        </el-form-item>
        <el-form-item label="密码">
          <el-input v-model="createForm.password" type="password" show-password placeholder="至少 6 位" />
        </el-form-item>
        <el-form-item label="管理员">
          <el-switch v-model="createForm.is_admin" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="createVisible = false">取消</el-button>
        <el-button type="primary" @click="doCreate">创建</el-button>
      </template>
    </el-dialog>

    <!-- Reset password dialog -->
    <el-dialog v-model="resetVisible" :title="`重置「${resetTarget?.username}」的密码`" width="440px">
      <el-input v-model="resetPassword" type="password" show-password placeholder="新密码（至少 6 位）" />
      <template #footer>
        <el-button @click="resetVisible = false">取消</el-button>
        <el-button type="primary" @click="doReset">重置</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.users-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
}
.toolbar {
  padding: 14px 20px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--border);
}
h2 {
  margin: 0;
  font-size: 16px;
}
.table-wrap {
  flex: 1;
  overflow: auto;
  padding: 16px 20px;
}
</style>
