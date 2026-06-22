<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox, type FormInstance, type FormRules } from 'element-plus'
import {
  connectionsApi,
  type Connection,
  type ConnectionInput,
} from '@/api/connections'

const list = ref<Connection[]>([])
const loading = ref(false)
const dialogVisible = ref(false)
const editingId = ref<number | null>(null)
const formRef = ref<FormInstance>()

const form = reactive<ConnectionInput>({
  name: '',
  host: '',
  port: 22,
  username: '',
  auth_type: 'password',
  secret: '',
})

const rules: FormRules = {
  name: [{ required: true, message: '请输入名称', trigger: 'blur' }],
  host: [{ required: true, message: '请输入主机', trigger: 'blur' }],
  username: [{ required: true, message: '请输入用户名', trigger: 'blur' }],
}

async function load() {
  loading.value = true
  try {
    list.value = await connectionsApi.list()
  } catch (e: any) {
    ElMessage.error(e.message)
  } finally {
    loading.value = false
  }
}

function openCreate() {
  editingId.value = null
  Object.assign(form, {
    name: '',
    host: '',
    port: 22,
    username: '',
    auth_type: 'password',
    secret: '',
  })
  dialogVisible.value = true
}

async function openEdit(c: Connection) {
  editingId.value = c.id
  const detail = await connectionsApi.detail(c.id)
  Object.assign(form, {
    name: detail.name,
    host: detail.host,
    port: detail.port,
    username: detail.username,
    auth_type: detail.auth_type as 'password' | 'key',
    secret: '', // never echo the secret; leave blank to keep existing
  })
  dialogVisible.value = true
}

async function submit() {
  if (!formRef.value) return
  await formRef.value.validate(async (valid) => {
    if (!valid) return
    try {
      if (editingId.value === null) {
        if (!form.secret) {
          ElMessage.warning('请输入密码或私钥')
          return
        }
        await connectionsApi.create({ ...form })
        ElMessage.success('已创建')
      } else {
        // On edit, drop empty secret so the backend keeps the stored value.
        const payload = { ...form }
        if (!payload.secret) delete payload.secret
        await connectionsApi.update(editingId.value, payload)
        ElMessage.success('已保存')
      }
      dialogVisible.value = false
      load()
    } catch (e: any) {
      ElMessage.error(e.message)
    }
  })
}

async function remove(c: Connection) {
  try {
    await ElMessageBox.confirm(`删除连接「${c.name}」？`, '确认', {
      type: 'warning',
    })
  } catch {
    return
  }
  try {
    await connectionsApi.remove(c.id)
    ElMessage.success('已删除')
    load()
  } catch (e: any) {
    ElMessage.error(e.message)
  }
}

onMounted(load)
</script>

<template>
  <div class="connections-view">
    <header class="toolbar">
      <h2>SSH 连接</h2>
      <el-button type="primary" @click="openCreate">+ 新建连接</el-button>
    </header>

    <div class="table-wrap">
      <el-table
        v-loading="loading"
        :data="list"
        stripe
        empty-text="还没有连接，点击右上角新建"
      >
        <el-table-column prop="name" label="名称" min-width="120" />
        <el-table-column label="目标" min-width="220">
          <template #default="{ row }">
            {{ row.username }}@{{ row.host }}:{{ row.port }}
          </template>
        </el-table-column>
        <el-table-column prop="auth_type" label="认证" width="90">
          <template #default="{ row }">
            <el-tag size="small" :type="row.auth_type === 'key' ? 'success' : 'info'">
              {{ row.auth_type === 'key' ? '私钥' : '密码' }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="last_used_at" label="上次使用" width="170">
          <template #default="{ row }">
            {{ row.last_used_at || '—' }}
          </template>
        </el-table-column>
        <el-table-column label="操作" width="160" fixed="right">
          <template #default="{ row }">
            <el-button size="small" text @click="openEdit(row)">编辑</el-button>
            <el-button size="small" text type="danger" @click="remove(row)">删除</el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <el-dialog
      v-model="dialogVisible"
      :title="editingId === null ? '新建连接' : '编辑连接'"
      width="520px"
    >
      <el-form
        ref="formRef"
        :model="form"
        :rules="rules"
        label-width="90px"
        label-position="left"
      >
        <el-form-item label="名称" prop="name">
          <el-input v-model="form.name" placeholder="例如：生产服务器" />
        </el-form-item>
        <el-form-item label="主机" prop="host">
          <el-input v-model="form.host" placeholder="hostname or IP" />
        </el-form-item>
        <el-form-item label="端口">
          <el-input-number v-model="form.port" :min="1" :max="65535" />
        </el-form-item>
        <el-form-item label="用户名" prop="username">
          <el-input v-model="form.username" placeholder="SSH 登录用户名" />
        </el-form-item>
        <el-form-item label="认证方式">
          <el-radio-group v-model="form.auth_type">
            <el-radio value="password">密码</el-radio>
            <el-radio value="key">私钥</el-radio>
          </el-radio-group>
        </el-form-item>
        <el-form-item :label="form.auth_type === 'key' ? '私钥' : '密码'">
          <el-input
            v-if="form.auth_type === 'password'"
            v-model="form.secret"
            type="password"
            show-password
            :placeholder="editingId === null ? '' : '留空则不修改'"
          />
          <el-input
            v-else
            v-model="form.secret"
            type="textarea"
            :rows="6"
            placeholder="-----BEGIN OPENSSH PRIVATE KEY-----&#10;...&#10;-----END OPENSSH PRIVATE KEY-----&#10;（留空则不修改）"
          />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" @click="submit">保存</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.connections-view {
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
