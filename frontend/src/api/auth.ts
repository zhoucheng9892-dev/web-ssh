import { http } from './client'
import { sha256Hex } from '@/utils/hash'

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
  // Passwords are SHA-256 hashed on the client; the backend applies Argon2id
  // on top. Plaintext never leaves the browser.
  login: (username: string, password: string, captcha: string) =>
    sha256Hex(password).then((hp) =>
      http.post<Me>('/auth/login', { username, password: hp, captcha }).then((r) => r.data),
    ),
  setup: (username: string, password: string) =>
    sha256Hex(password).then((hp) =>
      http.post<Me>('/auth/setup', { username, password: hp }).then((r) => r.data),
    ),
  logout: () => http.post('/auth/logout').then((r) => r.data),
  changePassword: (oldPassword: string, newPassword: string) =>
    Promise.all([sha256Hex(oldPassword), sha256Hex(newPassword)]).then(([oldHp, newHp]) =>
      http
        .post('/auth/password', { old_password: oldHp, new_password: newHp })
        .then((r) => r.data),
    ),
}
