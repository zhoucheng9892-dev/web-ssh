import { http } from './client'

export interface Me {
  id: number
  username: string
  is_admin: boolean
}

export interface AuthStatus {
  needs_setup: boolean
}

export interface CaptchaImage {
  image: string // data:image/png;base64,... URL
}

export const authApi = {
  status: () => http.get<AuthStatus>('/auth/status').then((r) => r.data),
  me: () => http.get<Me>('/auth/me').then((r) => r.data),
  captcha: () => http.get<CaptchaImage>('/auth/captcha').then((r) => r.data),
  login: (username: string, password: string, captcha: string) =>
    http.post<Me>('/auth/login', { username, password, captcha }).then((r) => r.data),
  setup: (username: string, password: string) =>
    http.post<Me>('/auth/setup', { username, password }).then((r) => r.data),
  logout: () => http.post('/auth/logout').then((r) => r.data),
  changePassword: (oldPassword: string, newPassword: string) =>
    http.post('/auth/password', { old_password: oldPassword, new_password: newPassword }).then((r) => r.data),
}
