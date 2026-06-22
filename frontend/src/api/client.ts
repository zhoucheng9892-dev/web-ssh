import axios from 'axios'

// Same-origin in production (embedded), proxied to :3000 in dev via vite config.
export const http = axios.create({
  baseURL: '/api',
  withCredentials: true, // send + receive the session cookie
  timeout: 30000,
})

// Surface backend error messages uniformly.
http.interceptors.response.use(
  (res) => res,
  (error) => {
    const status = error.response?.status
    const message =
      error.response?.data?.error || error.message || 'request failed'
    if (status === 401) {
      // Bounce to login; the router guard also handles this reactively.
      window.dispatchEvent(new CustomEvent('auth:unauthorized'))
    }
    return Promise.reject(Object.assign(new Error(message), { status, ...error }))
  },
)

export class ApiError extends Error {
  status?: number
  constructor(message: string, status?: number) {
    super(message)
    this.status = status
  }
}
