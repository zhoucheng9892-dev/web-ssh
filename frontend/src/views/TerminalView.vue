<script setup lang="ts">
import { nextTick, onActivated, onBeforeUnmount, onDeactivated, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { connectionsApi, type Connection } from '@/api/connections'
import { terminalApi } from '@/api/terminal'
import {
  attachSession,
  closeSession,
  detachSession,
  openSession,
  refit,
  type TerminalSession,
} from '@/composables/useTerminal'

const connections = ref<Connection[]>([])
// Sessions live across page switches: the WebSocket stays open; only the xterm
// renderer is detached/re-attached as this view is hidden/shown.
const sessions = ref<TerminalSession[]>([])
const activeId = ref<string | null>(null)
/** Map session id -> its pane div element (only populated while visible). */
const paneRefs = ref<Record<string, HTMLDivElement>>({})
/** Connection ids currently being probed; disables the picker while in flight. */
const probing = ref(false)

async function loadConnections() {
  connections.value = await connectionsApi.list()
}

async function openTerminal(c: Connection) {
  // Pre-flight: dial + authenticate via a normal HTTP request so the real
  // failure reason (DNS / algorithm negotiation / auth / timeout …) is surfaced
  // through the axios interceptor. The WebSocket upgrade path can't deliver
  // this — the browser hides the HTTP error body on a rejected handshake.
  if (probing.value) return
  probing.value = true
  try {
    await terminalApi.probe(c.id)
  } catch (e: any) {
    ElMessage.error(e.message || '连接失败')
    probing.value = false
    return
  }
  probing.value = false

  // Allow multiple independent sessions to the same connection. Disambiguate
  // the tab title with a counter (e.g. "myhost", "myhost (2)").
  const sameConn = sessions.value.filter((s) => s.connectionId === c.id).length
  const title = sameConn === 0 ? c.name : `${c.name} (${sameConn + 1})`
  const session = openSession(c.id, title)
  sessions.value.push(session)
  activeId.value = session.id
  await nextTick()
  const pane = paneRefs.value[session.id]
  if (pane) attachSession(session, pane)
}

function selectTab(id: string) {
  activeId.value = id
  nextTick(() => {
    const s = sessions.value.find((x) => x.id === id)
    if (s) refit(s)
  })
}

async function closeTab(id: string) {
  const s = sessions.value.find((x) => x.id === id)
  if (!s) return
  if (!s.closed) {
    try {
      await ElMessageBox.confirm(`关闭与「${s.title}」的连接？`, '确认', {
        type: 'warning',
      })
    } catch {
      return
    }
  }
  closeSession(s)
  sessions.value = sessions.value.filter((x) => x.id !== id)
  delete paneRefs.value[id]
  if (activeId.value === id) {
    activeId.value = sessions.value[sessions.value.length - 1]?.id ?? null
  }
}

function setPaneRef(id: string, el: Element | any) {
  if (el instanceof HTMLDivElement) paneRefs.value[id] = el
}

function onResize() {
  const s = sessions.value.find((x) => x.id === activeId.value)
  if (s) refit(s)
}

function isActive(id: string) {
  return activeId.value === id
}

/** Re-attach all sessions' xterm renderers when the view becomes visible. */
function reattachAll() {
  for (const s of sessions.value) {
    const pane = paneRefs.value[s.id]
    if (pane && !s.closed) attachSession(s, pane)
  }
  const active = sessions.value.find((x) => x.id === activeId.value)
  if (active) refit(active)
}

loadConnections()
window.addEventListener('resize', onResize)

// Listen for the sidebar pin toggle (not hover). AppLayout dispatches this
// custom event only when `collapsed` actually changes via click — hover slide-out
// doesn't fire it, so the terminal doesn't resize during hover.
window.addEventListener('sidebar:toggle', onResize)

// <KeepAlive> lifecycle: detach host divs when hidden, re-parent them back
// when shown. paneRefs persist because KeepAlive keeps the DOM alive — the
// mappings stay valid, only the host's parent changes.
// Also refresh the connection list so newly-added connections (from the
// Connections page) appear in the dropdown without a manual refresh.
onActivated(() => {
  loadConnections()
  nextTick(reattachAll)
})
onDeactivated(() => {
  for (const s of sessions.value) detachSession(s)
})

// Hard unmount (e.g. logout): close everything.
onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
  window.removeEventListener('sidebar:toggle', onResize)
  for (const s of sessions.value) closeSession(s)
})
</script>

<template>
  <div class="terminal-view">
    <header class="toolbar">
      <el-select
        placeholder="选择连接以打开终端"
        style="width: 260px"
        @change="(id: any) => {
          const c = connections.find((x) => x.id === id)
          if (c) openTerminal(c)
        }"
      >
        <el-option
          v-for="c in connections"
          :key="c.id"
          :label="`${c.name} (${c.username}@${c.host}:${c.port})`"
          :value="c.id"
        />
      </el-select>
      <el-button text @click="loadConnections">刷新</el-button>
    </header>

    <div class="tabs" v-if="sessions.length">
      <div
        v-for="s in sessions"
        :key="s.id"
        class="tab"
        :class="{ active: isActive(s.id), closed: s.closed }"
        @click="selectTab(s.id)"
      >
        <span class="dot" :class="{ live: !s.closed }"></span>
        <span class="title">{{ s.title }}</span>
        <span class="close" @click.stop="closeTab(s.id)">×</span>
      </div>
    </div>

    <div class="pane-area">
      <div
        v-for="s in sessions"
        :key="s.id"
        v-show="isActive(s.id)"
        class="pane"
        :ref="(el) => setPaneRef(s.id, el)"
      ></div>
      <div v-if="!sessions.length" class="empty">
        <p>还没有打开的终端会话。</p>
        <p class="hint">从上方选择一个连接开始，或在「连接」页新建连接。</p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.terminal-view {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.toolbar {
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
  display: flex;
  gap: 10px;
  align-items: center;
}
.tabs {
  display: flex;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  overflow-x: auto;
}
.tab {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 14px;
  border-right: 1px solid var(--border);
  cursor: pointer;
  color: var(--muted);
  font-size: 13px;
  white-space: nowrap;
}
.tab.active {
  background: var(--bg);
  color: var(--text);
  border-bottom: 2px solid var(--accent);
}
.tab.closed .title {
  text-decoration: line-through;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #6e7681;
}
.dot.live {
  background: #3fb950;
}
.close {
  padding: 0 4px;
  border-radius: 3px;
}
.close:hover {
  background: rgba(255, 255, 255, 0.1);
}
.pane-area {
  flex: 1;
  position: relative;
  min-height: 0;
  background: #0d1117;
}
.pane {
  position: absolute;
  inset: 0;
}
.empty {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--muted);
  gap: 8px;
}
.hint {
  font-size: 13px;
  opacity: 0.7;
}
</style>
