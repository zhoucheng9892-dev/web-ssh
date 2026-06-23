import { http } from './client'
import { sha256Hex } from '@/utils/hash'

export interface UserInfo {
  id: number
  username: string
  is_admin: boolean
  is_disabled: boolean
  created_at: string
}

export const usersApi = {
  list: () => http.get<UserInfo[]>('/users').then((r) => r.data),
  // Passwords are SHA-256 hashed on the client (backend applies Argon2id on top).
  create: (username: string, password: string, isAdmin: boolean) =>
    sha256Hex(password).then((hp) =>
      http.post<{ id: number }>('/users', { username, password: hp, is_admin: isAdmin }).then((r) => r.data),
    ),
  remove: (id: number) => http.delete(`/users/${id}`).then((r) => r.data),
  update: (id: number, isAdmin: boolean) =>
    http.put(`/users/${id}`, { is_admin: isAdmin }).then((r) => r.data),
  resetPassword: (id: number, password: string) =>
    sha256Hex(password).then((hp) =>
      http.post(`/users/${id}/password`, { password: hp }).then((r) => r.data),
    ),
  freeze: (id: number) => http.post(`/users/${id}/freeze`).then((r) => r.data),
  unfreeze: (id: number) => http.post(`/users/${id}/unfreeze`).then((r) => r.data),
}
