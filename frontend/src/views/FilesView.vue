<script setup lang="ts">
import { onActivated, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  filesApi,
  type FileEntry,
} from '@/api/files'
import { connectionsApi, type Connection } from '@/api/connections'

const connections = ref<Connection[]>([])
const connectionId = ref<number | null>(null)
const entries = ref<FileEntry[]>([])
const cwd = ref<string>('.')
const breadcrumb = ref<string[]>([])
const loading = ref(false)

async function loadConnections() {
  connections.value = await connectionsApi.list()
}

async function load() {
  if (connectionId.value === null) return
  loading.value = true
  try {
    const res = await filesApi.list(connectionId.value, cwd.value)
    entries.value = res.entries
    // Build a breadcrumb from the resolved absolute path.
    const parts = res.path.split('/').filter(Boolean)
    breadcrumb.value = parts.length ? parts : ['.']
  } catch (e: any) {
    ElMessage.error(e.message)
  } finally {
    loading.value = false
  }
}

function onConnectionChange() {
  cwd.value = '.'
  breadcrumb.value = ['.']
  load()
}

function enter(entry: FileEntry) {
  if (!entry.is_dir) return
  if (cwd.value === '.' || cwd.value === '') {
    cwd.value = entry.name
  } else if (cwd.value === '/') {
    cwd.value = '/' + entry.name
  } else {
    cwd.value = cwd.value.replace(/\/$/, '') + '/' + entry.name
  }
  load()
}

function goTo(index: number) {
  // index into breadcrumb; rebuild path from root.
  const parts = breadcrumb.value.slice(0, index + 1)
  if (parts.length === 1 && parts[0] === '.') {
    cwd.value = '.'
  } else {
    cwd.value = '/' + parts.join('/')
  }
  load()
}

function download(entry: FileEntry) {
  if (entry.is_dir || connectionId.value === null) return
  const full =
    cwd.value === '.' ? entry.name : cwd.value.replace(/\/$/, '') + '/' + entry.name
  // Trigger a download via a temporary anchor (cookie sent automatically).
  const a = document.createElement('a')
  a.href = filesApi.downloadUrl(connectionId.value, full)
  a.download = entry.name
  document.body.appendChild(a)
  a.click()
  a.remove()
}

async function remove(entry: FileEntry) {
  if (connectionId.value === null) return
  try {
    await ElMessageBox.confirm(
      `删除「${entry.name}」${entry.is_dir ? '（目录需为空）' : ''}？`,
      '确认',
      { type: 'warning' },
    )
  } catch {
    return
  }
  const full =
    cwd.value === '.' ? entry.name : cwd.value.replace(/\/$/, '') + '/' + entry.name
  try {
    await filesApi.remove(connectionId.value, full)
    ElMessage.success('已删除')
    load()
  } catch (e: any) {
    ElMessage.error(e.message)
  }
}

async function mkdir() {
  if (connectionId.value === null) return
  try {
    const { value } = await ElMessageBox.prompt('新目录名称', '新建目录', {
      confirmButtonText: '创建',
      cancelButtonText: '取消',
    })
    if (!value) return
    const full =
      cwd.value === '.' ? value : cwd.value.replace(/\/$/, '') + '/' + value
    await filesApi.mkdir(connectionId.value, full)
    ElMessage.success('已创建')
    load()
  } catch {
    /* cancelled */
  }
}

function onUpload(ev: any) {
  const files: File[] = Array.from(ev.file?.fileList ? [] : [ev.file]).filter(
    Boolean,
  ) as File[]
  // el-upload passes { file } in http-request override; use raw input instead.
}

async function onFilePicked(ev: Event) {
  const input = ev.target as HTMLInputElement
  if (!input.files || input.files.length === 0 || connectionId.value === null) return
  try {
    await filesApi.upload(connectionId.value, cwd.value, Array.from(input.files))
    ElMessage.success(`已上传 ${input.files.length} 个文件`)
    load()
  } catch (e: any) {
    ElMessage.error(e.message)
  } finally {
    input.value = ''
  }
}

function fmtSize(n: number) {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`
}

function fmtDate(s: string | null) {
  if (!s) return '—'
  return new Date(s).toLocaleString()
}

onMounted(async () => {
  await loadConnections()
  if (connections.value.length) {
    connectionId.value = connections.value[0].id
    onConnectionChange()
  }
})

// KeepAlive: refresh the connection list when returning to this view so newly
// added connections appear, without disrupting an already-selected connection.
onActivated(async () => {
  const prev = connectionId.value
  await loadConnections()
  if (connectionId.value === null && connections.value.length) {
    connectionId.value = connections.value[0].id
    onConnectionChange()
  } else if (prev !== null && !connections.value.some((c) => c.id === prev)) {
    // previously selected connection was deleted; fall back to first.
    connectionId.value = connections.value[0]?.id ?? null
    onConnectionChange()
  }
})
</script>

<template>
  <div class="files-view">
    <header class="toolbar">
      <el-select
        v-model="connectionId"
        placeholder="选择连接"
        style="width: 240px"
        @change="onConnectionChange"
      >
        <el-option
          v-for="c in connections"
          :key="c.id"
          :label="c.name"
          :value="c.id"
        />
      </el-select>
      <el-button-group>
        <el-button :icon="'ArrowUp'" :disabled="!connectionId" @click="goTo(Math.max(0, breadcrumb.length - 2))">上级</el-button>
        <el-button :disabled="!connectionId" @click="load">刷新</el-button>
        <el-button :disabled="!connectionId" @click="mkdir">新建目录</el-button>
      </el-button-group>
      <label class="upload">
        <span>上传</span>
        <input type="file" multiple @change="onFilePicked" :disabled="!connectionId" />
      </label>
    </header>

    <div class="breadcrumb">
      <span class="crumb" @click="goTo(-1)"><a>根</a></span>
      <template v-for="(p, i) in breadcrumb" :key="i">
        <span class="sep">/</span>
        <span class="crumb" @click="goTo(i)"><a>{{ p }}</a></span>
      </template>
    </div>

    <div class="table-wrap">
      <el-table
        v-loading="loading"
        :data="entries"
        stripe
        empty-text="目录为空或未选择连接"
        @row-dblclick="(row: FileEntry) => row.is_dir && enter(row)"
      >
        <el-table-column label="名称" min-width="280">
          <template #default="{ row }">
            <span class="name" :class="{ dir: row.is_dir }" @click="row.is_dir ? enter(row) : download(row)">
              {{ row.is_dir ? '📁' : '📄' }} {{ row.name }}
            </span>
          </template>
        </el-table-column>
        <el-table-column label="大小" width="120">
          <template #default="{ row }">{{ row.is_dir ? '—' : fmtSize(row.size) }}</template>
        </el-table-column>
        <el-table-column label="修改时间" width="200">
          <template #default="{ row }">{{ fmtDate(row.modified) }}</template>
        </el-table-column>
        <el-table-column label="操作" width="140" fixed="right">
          <template #default="{ row }">
            <el-button v-if="!row.is_dir" size="small" text @click="download(row)">下载</el-button>
            <el-button size="small" text type="danger" @click="remove(row)">删除</el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>
  </div>
</template>

<style scoped>
.files-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
}
.toolbar {
  padding: 14px 20px;
  display: flex;
  gap: 12px;
  align-items: center;
  border-bottom: 1px solid var(--border);
}
.upload {
  position: relative;
  display: inline-flex;
  align-items: center;
  padding: 0 14px;
  height: 32px;
  background: var(--accent);
  color: #fff;
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
}
.upload input {
  position: absolute;
  inset: 0;
  opacity: 0;
  cursor: pointer;
}
.breadcrumb {
  padding: 10px 20px;
  display: flex;
  gap: 4px;
  align-items: center;
  color: var(--muted);
  font-size: 13px;
  border-bottom: 1px solid var(--border);
}
.crumb {
  cursor: pointer;
}
.crumb a {
  color: var(--accent);
}
.sep {
  opacity: 0.5;
}
.table-wrap {
  flex: 1;
  overflow: auto;
  padding: 12px 20px;
}
.name {
  cursor: pointer;
}
.name.dir {
  color: var(--accent);
}
.name:hover {
  text-decoration: underline;
}
</style>
