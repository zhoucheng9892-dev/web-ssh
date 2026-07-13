<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter, RouterView } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const router = useRouter()

const active = computed(() => router.currentRoute.value.name as string)

async function logout() {
  await auth.logout()
  router.push({ name: 'login' })
}

// Sidebar collapse state. false = expanded (200px), true = collapsed (56px
// icon-only strip). When collapsed, hovering the sidebar slides it out as an
// overlay without pushing the main content.
const collapsed = ref(false)
const hovering = ref(false)

/** Whether the sidebar is currently shown at full width (pinned or hovered). */
const expanded = computed(() => !collapsed.value || hovering.value)

function toggleSidebar() {
  collapsed.value = !collapsed.value
}

// Only fire on pin toggle (click), NOT on hover. This lets the terminal refit
// its width when the sidebar permanently collapses/expands, while hover
// slide-out (which doesn't change `collapsed`) leaves the terminal untouched.
watch(collapsed, () => {
  window.dispatchEvent(new CustomEvent('sidebar:toggle'))
})
</script>

<template>
  <div class="layout">
    <aside
      class="sidebar"
      :class="{ collapsed, expanded }"
      @mouseenter="collapsed && (hovering = true)"
      @mouseleave="hovering = false"
    >
      <div class="brand" @click="toggleSidebar" title="点击收起/展开菜单">
        <span class="brand-icon">{{ collapsed && !hovering ? '☰' : '▸_' }}</span>
        <span v-if="expanded" class="brand-text">Web SSH</span>
      </div>
      <nav>
        <RouterLink to="/" :class="{ active: active === 'terminals' }" :title="!expanded ? '终端' : undefined">
          <span class="icon">▶</span>
          <span v-if="expanded" class="label">终端</span>
        </RouterLink>
        <RouterLink to="/connections" :class="{ active: active === 'connections' }" :title="!expanded ? '连接' : undefined">
          <span class="icon">⚙</span>
          <span v-if="expanded" class="label">连接</span>
        </RouterLink>
        <RouterLink to="/files" :class="{ active: active === 'files' }" :title="!expanded ? '文件' : undefined">
          <span class="icon">📁</span>
          <span v-if="expanded" class="label">文件</span>
        </RouterLink>
        <RouterLink to="/password" :class="{ active: active === 'password' }" :title="!expanded ? '修改密码' : undefined">
          <span class="icon">🔑</span>
          <span v-if="expanded" class="label">修改密码</span>
        </RouterLink>
        <RouterLink
          v-if="auth.user?.is_admin"
          to="/users"
          :class="{ active: active === 'users' }"
          :title="!expanded ? '用户管理' : undefined"
        >
          <span class="icon">👥</span>
          <span v-if="expanded" class="label">用户管理</span>
        </RouterLink>
      </nav>
      <div class="user" v-if="expanded">
        <div class="name">{{ auth.user?.username }}</div>
        <el-button size="small" text @click="logout">退出</el-button>
      </div>
      <div class="user user-collapsed" v-else>
        <span class="icon" :title="auth.user?.username">👤</span>
      </div>
    </aside>
    <main class="content">
      <RouterView v-slot="{ Component }">
        <!-- Keep views alive across nav switches so terminal sessions (and
             their WebSockets) survive visiting the Files/Connections pages. -->
        <KeepAlive>
          <component :is="Component" />
        </KeepAlive>
      </RouterView>
    </main>
  </div>
</template>

<style scoped>
.layout {
  display: flex;
  height: 100%;
  position: relative;
}

.sidebar {
  width: 200px;
  background: var(--panel);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  transition: width 0.2s ease;
  /* Prevent text wrapping during the width transition. */
  overflow: hidden;
}

/* Collapsed (not hovered): narrow icon strip. */
.sidebar.collapsed:not(.expanded) {
  width: 56px;
}

/* Collapsed + hovered: slide out to full width as an overlay without pushing
   the main content. */
.sidebar.collapsed.expanded {
  width: 200px;
  position: absolute;
  top: 0;
  left: 0;
  bottom: 0;
  z-index: 100;
  box-shadow: 4px 0 12px rgba(0, 0, 0, 0.4);
}

.brand {
  padding: 18px 20px;
  font-size: 16px;
  font-weight: 600;
  color: var(--accent);
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  user-select: none;
  white-space: nowrap;
}

.sidebar.collapsed:not(.expanded) .brand {
  padding: 18px 0;
  justify-content: center;
}

.brand-icon {
  font-family: monospace;
}

.brand-text {
  margin-left: 2px;
}

nav {
  flex: 1;
  padding: 12px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

nav a {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 6px;
  color: var(--text);
  font-size: 14px;
  white-space: nowrap;
}

.sidebar.collapsed:not(.expanded) nav a {
  justify-content: center;
  padding: 10px 0;
}

nav a:hover {
  background: rgba(255, 255, 255, 0.05);
}

nav a.active {
  background: rgba(79, 158, 255, 0.15);
  color: var(--accent);
}

.icon {
  width: 18px;
  text-align: center;
  font-size: 12px;
  flex-shrink: 0;
}

.label {
  overflow: hidden;
  text-overflow: ellipsis;
}

.user {
  padding: 12px 16px;
  border-top: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: space-between;
  white-space: nowrap;
}

.user-collapsed {
  justify-content: center;
}

.name {
  color: var(--muted);
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
</style>
