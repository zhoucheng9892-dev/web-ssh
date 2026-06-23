import { http } from './client'

/** Probe result from `/api/terminal/probe`. */
export interface ProbeResult {
  ok: true
  host: string
  port: number
  username: string
}

export const terminalApi = {
  /**
   * Pre-flight SSH connectivity check: dials + authenticates, then closes
   * without opening a shell. Failures come back as normal HTTP errors (readable
   * via the axios interceptor), unlike the WebSocket upgrade path where the
   * browser can never read the error body.
   */
  probe: (connectionId: number) =>
    http.get<ProbeResult>('/terminal/probe', { params: { connection_id: connectionId } }).then((r) => r.data),
}
