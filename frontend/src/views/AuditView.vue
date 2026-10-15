<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { auditApi, type AuditLog } from '@/api/audit'
import { usersApi, type UserInfo } from '@/api/users'

const logs = ref<AuditLog[]>([])
const loading = ref(false)
const total = ref(0)

// Filters
const users = ref<UserInfo[]>([])
const filterUserId = ref<number | undefined>(undefined)
const dateRange = ref<[string, string] | null>(null)
const currentPage = ref(1)
const pageSize = ref(50)

async function load() {
  loading.value = true
  try {
    const res = await auditApi.list({
      user_id: filterUserId.value,
      start: dateRange.value?.[0],
      end: dateRange.value?.[1],
      page: currentPage.value,
      page_size: pageSize.value,
    })
    logs.value = res.logs
    total.value = res.total
  } catch (e: any) {
    ElMessage.error(e.message)
  } finally {
    loading.value = false
  }
}

function onSearch() {
  currentPage.value = 1
  load()
}

function onPageChange(page: number) {
  currentPage.value = page
  load()
}

function onPageSizeChange(size: number) {
  pageSize.value = size
  currentPage.value = 1
  load()
}

onMounted(async () => {
  try {
    users.value = await usersApi.list()
  } catch {
    /* ignore */
  }
  load()
})
</script>

<template>
  <div class="audit-view">
    <header class="toolbar">
      <h2>审计日志</h2>
      <div class="filters">
        <el-select
          v-model="filterUserId"
          placeholder="全部用户"
          clearable
          style="width: 160px"
        >
          <el-option
            v-for="u in users"
            :key="u.id"
            :label="u.username"
            :value="u.id"
          />
        </el-select>
        <el-date-picker
          v-model="dateRange"
          type="datetimerange"
          range-separator="至"
          start-placeholder="开始时间"
          end-placeholder="结束时间"
          format="YYYY-MM-DD HH:mm:ss"
          value-format="YYYY-MM-DD HH:mm:ss"
          style="width: 380px"
        />
        <el-button type="primary" @click="onSearch">查询</el-button>
      </div>
    </header>

    <div class="table-wrap">
      <el-table v-loading="loading" :data="logs" stripe>
        <el-table-column prop="created_at" label="时间" width="180" />
        <el-table-column prop="username" label="用户" width="120" />
        <el-table-column prop="conn_name" label="连接" width="160" />
        <el-table-column prop="command" label="命令" min-width="400">
          <template #default="{ row }">
            <code class="cmd">{{ row.command }}</code>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <div class="pagination">
      <el-pagination
        :current-page="currentPage"
        :page-size="pageSize"
        :total="total"
        :page-sizes="[20, 50, 100, 200]"
        layout="total, sizes, prev, pager, next"
        @current-change="onPageChange"
        @size-change="onPageSizeChange"
      />
    </div>
  </div>
</template>

<style scoped>
.audit-view {
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
  flex-wrap: wrap;
  gap: 12px;
}
.filters {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
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
.cmd {
  font-family: 'Menlo', 'Consolas', monospace;
  font-size: 13px;
  color: var(--accent);
  word-break: break-all;
}
.pagination {
  padding: 8px 20px;
  border-top: 1px solid var(--border);
  display: flex;
  justify-content: flex-end;
}
</style>
