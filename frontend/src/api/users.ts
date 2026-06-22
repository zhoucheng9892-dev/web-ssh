import { http } from './client'

export interface UserInfo {
  id: number
  username: string
  is_admin: boolean
  is_disabled: boolean
  created_at: string
}

export const usersApi = {
  list: () => http.get<UserInfo[]>('/users').then((r) => r.data),
  create: (username: string, password: string, isAdmin: boolean) =>
    http.post<{ id: number }>('/users', { username, password, is_admin: isAdmin }).then((r) => r.data),
  remove: (id: number) => http.delete(`/users/${id}`).then((r) => r.data),
  update: (id: number, isAdmin: boolean) =>
    http.put(`/users/${id}`, { is_admin: isAdmin }).then((r) => r.data),
  resetPassword: (id: number, password: string) =>
    http.post(`/users/${id}/password`, { password }).then((r) => r.data),
  freeze: (id: number) => http.post(`/users/${id}/freeze`).then((r) => r.data),
  unfreeze: (id: number) => http.post(`/users/${id}/unfreeze`).then((r) => r.data),
}
