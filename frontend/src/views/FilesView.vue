<script setup lang="ts">
import { onActivated, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  filesApi,
  uploadWithProgress,
  type FileEntry,
} from '@/api/files'
import { connectionsApi, type Connection } from '@/api/connections'

/** Status of a single file in the upload queue. */
type UploadStatus = 'pending' | 'uploading' | 'done' | 'error' | 'cancelled'

interface UploadTask {
  id: number
  file: File
  /** 0..100. */
  percent: number
  status: UploadStatus
  /** Bytes uploaded so far. */
  loaded: number
  /** Human-readable speed, refreshed while uploading. */
  speed: string
  /** Last progress timestamp + bytes, for computing speed. */
  lastTickAt: number
  lastTickLoaded: number
  error?: string
  /** Abort handle for the in-flight upload. */
  abort?: () => void
}

let taskSeq = 0
const uploadTasks = ref<UploadTask[]>([])
let uploading = false

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

/** Queue files picked from the input and start the serial uploader. */
function onFilePicked(ev: Event) {
  const input = ev.target as HTMLInputElement
  if (!input.files || input.files.length === 0 || connectionId.value === null) return
  for (const file of Array.from(input.files)) {
    uploadTasks.value.push({
      id: ++taskSeq,
      file,
      percent: 0,
      status: 'pending',
      loaded: 0,
      speed: '',
      lastTickAt: 0,
      lastTickLoaded: 0,
    })
  }
  input.value = ''
  void runUploadQueue()
}

/** Upload queued files one at a time to keep SFTP happy and progress clear. */
async function runUploadQueue() {
  if (uploading) return
  uploading = true
  try {
    while (true) {
      const task = uploadTasks.value.find((t) => t.status === 'pending')
      if (!task || connectionId.value === null) break
      task.status = 'uploading'
      task.lastTickAt = Date.now()
      task.lastTickLoaded = 0
      const { promise, abort } = uploadWithProgress(
        connectionId.value,
        cwd.value,
        task.file,
        (p) => updateProgress(task, p),
      )
      task.abort = abort
      try {
        await promise
        task.status = 'done'
        task.percent = 100
        task.speed = ''
      } catch (e: any) {
        task.status = e.message === '已取消' ? 'cancelled' : 'error'
        task.error = e.message
        ElMessage.error(`${task.file.name}: ${e.message}`)
      }
    }
    // Refresh the listing once the queue drains (any completions happened).
    if (uploadTasks.value.some((t) => t.status === 'done')) {
      load()
    }
  } finally {
    uploading = false
  }
}

/** Update a task's progress bar and compute the transfer speed. */
function updateProgress(task: UploadTask, p: { loaded: number; total: number; ratio: number }) {
  const now = Date.now()
  task.loaded = p.loaded
  task.percent = p.total > 0 ? Math.min(100, Math.round(p.ratio * 100)) : 0
  const dt = now - task.lastTickAt
  if (dt >= 500 && task.lastTickAt > 0) {
    const dBytes = p.loaded - task.lastTickLoaded
    const bps = (dBytes * 1000) / dt
    task.speed = `${fmtSize(bps)}/s`
    task.lastTickAt = now
    task.lastTickLoaded = p.loaded
  }
}

/** Cancel an in-flight upload or remove a finished/failed task from the list. */
function cancelTask(task: UploadTask) {
  if (task.status === 'uploading') {
    task.abort?.()
    return
  }
  uploadTasks.value = uploadTasks.value.filter((t) => t.id !== task.id)
}

/** Clear all finished/failed/cancelled tasks from the list. */
function clearFinished() {
  uploadTasks.value = uploadTasks.value.filter(
    (t) => t.status === 'pending' || t.status === 'uploading',
  )
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

    <div v-if="uploadTasks.length" class="upload-queue">
      <div class="queue-head">
        <span>上传队列（{{ uploadTasks.length }}）</span>
        <el-button size="small" text @click="clearFinished">清除已完成</el-button>
      </div>
      <div v-for="task in uploadTasks" :key="task.id" class="queue-item">
        <div class="queue-info">
          <span class="queue-name" :title="task.file.name">{{ task.file.name }}</span>
          <span class="queue-meta">
            {{ fmtSize(task.file.size) }}
            <template v-if="task.status === 'uploading' && task.speed">· {{ task.speed }}</template>
            <template v-if="task.status === 'done'">· 完成</template>
            <template v-if="task.status === 'error'">· {{ task.error }}</template>
            <template v-if="task.status === 'cancelled'">· 已取消</template>
            <template v-if="task.status === 'pending'">· 等待中</template>
          </span>
        </div>
        <el-progress
          :percentage="task.percent"
          :status="task.status === 'done' ? 'success' : task.status === 'error' ? 'exception' : undefined"
          :stroke-width="8"
          :show-text="true"
          class="queue-bar"
        />
        <el-button
          size="small"
          text
          :type="task.status === 'uploading' ? 'danger' : undefined"
          @click="cancelTask(task)"
        >
          {{ task.status === 'uploading' ? '取消' : '移除' }}
        </el-button>
      </div>
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
.upload-queue {
  padding: 10px 20px 14px;
  border-bottom: 1px solid var(--border);
  background: var(--panel);
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.queue-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: var(--muted);
  font-size: 13px;
}
.queue-item {
  display: grid;
  grid-template-columns: 1fr auto;
  grid-template-rows: auto auto;
  gap: 4px 12px;
  align-items: center;
}
.queue-info {
  min-width: 0;
  display: flex;
  gap: 8px;
  align-items: baseline;
  font-size: 13px;
}
.queue-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.queue-meta {
  color: var(--muted);
  font-size: 12px;
  white-space: nowrap;
}
.queue-bar {
  width: 100%;
}
/* Place the cancel/remove button in the top-right cell. */
.queue-item > .el-button {
  grid-row: 1;
  grid-column: 2;
}
/* Progress bar spans the full width under the info row. */
.queue-item > .el-progress {
  grid-row: 2;
  grid-column: 1 / -1;
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
