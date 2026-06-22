import { http } from './client'

export type AuthType = 'password' | 'key'

export interface Connection {
  id: number
  user_id: number
  name: string
  host: string
  port: number
  username: string
  auth_type: string
  last_used_at: string | null
  created_at: string
}

export interface ConnectionInput {
  name: string
  host: string
  port?: number
  username: string
  auth_type: AuthType
  /** password or PEM private key. Omit on update to keep the stored value. */
  secret?: string
}

export interface ConnectionDetail {
  id: number
  name: string
  host: string
  port: number
  username: string
  auth_type: string
}

export const connectionsApi = {
  list: () => http.get<Connection[]>('/connections').then((r) => r.data),
  detail: (id: number) =>
    http.get<ConnectionDetail>(`/connections/${id}`).then((r) => r.data),
  create: (input: ConnectionInput) =>
    http.post<{ id: number }>('/connections', input).then((r) => r.data),
  update: (id: number, input: ConnectionInput) =>
    http.put(`/connections/${id}`, input).then((r) => r.data),
  remove: (id: number) =>
    http.delete(`/connections/${id}`).then((r) => r.data),
}
