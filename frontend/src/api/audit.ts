import { http } from './client'

export interface AuditLog {
  id: number
  user_id: number
  username: string
  connection_id: number
  conn_name: string | null
  command: string
  created_at: string
}

export interface AuditListResult {
  logs: AuditLog[]
  total: number
}

export interface AuditQuery {
  user_id?: number
  start?: string
  end?: string
  page: number
  page_size: number
}

export const auditApi = {
  list: (q: AuditQuery) =>
    http.get<AuditListResult>('/audit', { params: q }).then((r) => r.data),
}
