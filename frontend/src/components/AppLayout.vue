<script setup lang="ts">
import { computed } from 'vue'
import { useRouter, RouterView } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const router = useRouter()

const active = computed(() => router.currentRoute.value.name as string)

async function logout() {
  await auth.logout()
  router.push({ name: 'login' })
}
</script>

<template>
  <div class="layout">
    <aside class="sidebar">
      <div class="brand">
        <span class="logo">▸_</span> Web SSH
      </div>
      <nav>
        <RouterLink to="/" :class="{ active: active === 'terminals' }">
          <span class="icon">▶</span> 终端
        </RouterLink>
        <RouterLink to="/connections" :class="{ active: active === 'connections' }">
          <span class="icon">⚙</span> 连接
        </RouterLink>
        <RouterLink to="/files" :class="{ active: active === 'files' }">
          <span class="icon">📁</span> 文件
        </RouterLink>
        <RouterLink to="/password" :class="{ active: active === 'password' }">
          <span class="icon">🔑</span> 修改密码
        </RouterLink>
        <RouterLink
          v-if="auth.user?.is_admin"
          to="/users"
          :class="{ active: active === 'users' }"
        >
          <span class="icon">👥</span> 用户管理
        </RouterLink>
      </nav>
      <div class="user">
        <div class="name">{{ auth.user?.username }}</div>
        <el-button size="small" text @click="logout">退出</el-button>
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
}
.sidebar {
  width: 200px;
  background: var(--panel);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
}
.brand {
  padding: 18px 20px;
  font-size: 16px;
  font-weight: 600;
  color: var(--accent);
  border-bottom: 1px solid var(--border);
}
.logo {
  font-family: monospace;
  margin-right: 4px;
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
}
.user {
  padding: 12px 16px;
  border-top: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: space-between;
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
