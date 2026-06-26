import { http } from './client'
import { appPath } from '@/context'

export interface FileEntry {
  name: string
  is_dir: boolean
  size: number
  modified: string | null
}

export interface DirListing {
  path: string
  entries: FileEntry[]
}

function params(connectionId: number, path?: string) {
  return { params: { connection_id: connectionId, path } }
}

export const filesApi = {
  list: (connectionId: number, path?: string) =>
    http
      .get<DirListing>('/files/list', params(connectionId, path))
      .then((r) => r.data),

  mkdir: (connectionId: number, path: string) =>
    http.post('/files/mkdir', null, params(connectionId, path)).then((r) => r.data),

  remove: (connectionId: number, path: string) =>
    http.delete('/files', params(connectionId, path)).then((r) => r.data),

  /** Full download URL (lets the browser stream the file directly). */
  downloadUrl: (connectionId: number, path: string) =>
    `${appPath('api/files/download')}?connection_id=${connectionId}&path=${encodeURIComponent(path)}`,
}

/** Progress callback for a single file upload. `loaded`/`total` are bytes. */
export interface UploadProgress {
  loaded: number
  total: number
  /** 0..1; 0 when total is unknown (chunked transfer encoding). */
  ratio: number
}

/**
 * Upload a single file with real-time progress via XMLHttpRequest.
 *
 * axios can't report reliable upload progress for large bodies and is bound by
 * the client's global 30s timeout, so large uploads use a raw XHR instead — it
 * has native `upload.onprogress` and no timeout by default. The returned object
 * exposes `abort()` so the caller can cancel an in-flight upload from the UI.
 *
 * On error the backend responds with `{ "error": "..." }`; we surface that the
 * same way the axios interceptor does.
 */
export function uploadWithProgress(
  connectionId: number,
  dir: string,
  file: File,
  onProgress: (p: UploadProgress) => void,
): { promise: Promise<string>; abort: () => void } {
  const form = new FormData()
  form.append('files', file, file.name)
  const url = `${appPath('api/files/upload')}?connection_id=${connectionId}&path=${encodeURIComponent(dir)}`

  const xhr = new XMLHttpRequest()
  xhr.open('POST', url)
  xhr.withCredentials = true
  xhr.responseType = 'json'

  xhr.upload.onprogress = (ev) => {
    onProgress({
      loaded: ev.loaded,
      total: ev.total,
      ratio: ev.lengthComputable ? ev.loaded / ev.total : 0,
    })
  }

  const promise = new Promise<string>((resolve, reject) => {
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        const uploaded: string[] | undefined = xhr.response?.uploaded
        resolve(uploaded?.[0] ?? file.name)
      } else {
        const message = xhr.response?.error || `上传失败（HTTP ${xhr.status}）`
        reject(new Error(message))
      }
    }
    xhr.onerror = () => reject(new Error('网络错误，上传失败'))
    xhr.onabort = () => reject(new Error('已取消'))
  })

  xhr.send(form)
  return { promise, abort: () => xhr.abort() }
}

/** Bytes per chunk for chunked uploads. */
const CHUNK_SIZE = 8 * 1024 * 1024

interface UploadStatusResult {
  exists: boolean
  size: number
}

interface ChunkAckResult {
  offset: number
}

/** Key prefix for sessionStorage resume entries. */
const RESUME_PREFIX = 'webssh.upload.'

/**
 * Persist enough state to resume after a page reload (same browser tab only).
 * SessionStorage is cleared when the tab is closed; that's an acceptable
 * trade-off for simplicity.
 */
function saveResumeState(
  connectionId: number,
  dir: string,
  file: File,
  offset: number,
) {
  try {
    sessionStorage.setItem(
      RESUME_PREFIX + file.name,
      JSON.stringify({
        connectionId,
        dir,
        name: file.name,
        size: file.size,
        lastModified: file.lastModified,
        offset,
      }),
    )
  } catch {
    /* quota exceeded – non-critical */
  }
}

function clearResumeState(filename: string) {
  try {
    sessionStorage.removeItem(RESUME_PREFIX + filename)
  } catch {
    /* ignore */
  }
}

/**
 * Upload a single file in fixed-size chunks via `/api/files/upload-chunk`.
 *
 * Each chunk is a tiny HTTP POST (no multipart overhead, no 413 risk from
 * reverse proxies with small body limits). If interrupted, call the function
 * again with the same `file` and it will skip already-written bytes (resume via
 * `/api/files/upload-status`).
 *
 * Returns the same `{ promise, abort }` shape as `uploadWithProgress` so
 * callers can swap between the two transparently.
 */
export function uploadChunked(
  connectionId: number,
  dir: string,
  file: File,
  onProgress: (p: UploadProgress) => void,
): { promise: Promise<string>; abort: () => void } {
  let aborted = false
  let currentAbort: (() => void) | null = null

  async function run(): Promise<string> {
    // 1 — probe remote: is there a partial file?
    const statusUrl = `${appPath('api/files/upload-status')}?connection_id=${connectionId}&path=${encodeURIComponent(dir)}&filename=${encodeURIComponent(file.name)}`
    let status: UploadStatusResult
    try {
      // Use fetch directly instead of axios so a non-2xx response doesn't get
      // swallowed by the global interceptor — we want full control here.
      const res = await fetch(statusUrl, { credentials: 'include' })
      if (!res.ok) {
        const body = await res.json().catch(() => ({}))
        throw new Error(body.error || `HTTP ${res.status}`)
      }
      status = await res.json()
    } catch (e: any) {
      // Surface the error so the caller can decide to retry or fail.
      throw new Error(`查询上传状态失败: ${e.message}`)
    }

    // 2 — resume from last confirmed offset
    let offset = status.exists ? status.size : 0
    if (offset >= file.size) {
      // Already complete — possibly a re-upload of the same file.
      onProgress({ loaded: file.size, total: file.size, ratio: 1 })
      return file.name
    }
    if (offset > 0) {
      onProgress({ loaded: offset, total: file.size, ratio: offset / file.size })
    }

    // 3 — send chunks sequentially
    while (offset < file.size) {
      if (aborted) throw new Error('已取消')

      const end = Math.min(offset + CHUNK_SIZE, file.size)
      const blob = file.slice(offset, end)

      const chunkUrl =
        `${appPath('api/files/upload-chunk')}?connection_id=${connectionId}` +
        `&path=${encodeURIComponent(dir)}` +
        `&filename=${encodeURIComponent(file.name)}` +
        `&offset=${offset}`

      const chunk = chunkRequest(chunkUrl, blob)
      currentAbort = chunk.abort
      const ack = await chunk.promise
      currentAbort = null
      offset = ack.offset
      onProgress({ loaded: offset, total: file.size, ratio: offset / file.size })
      saveResumeState(connectionId, dir, file, offset)
    }

    clearResumeState(file.name)
    return file.name
  }

  return {
    promise: run(),
    abort: () => {
      aborted = true
      currentAbort?.()
    },
  }
}

/** Send one chunk via XHR, return the confirmed new offset and an abort handle. */
function chunkRequest(url: string, blob: Blob): { promise: Promise<ChunkAckResult>; abort: () => void } {
  const xhr = new XMLHttpRequest()
  xhr.open('POST', url)
  xhr.withCredentials = true
  xhr.responseType = 'json'
  const promise = new Promise<ChunkAckResult>((resolve, reject) => {
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        resolve(xhr.response as ChunkAckResult)
      } else {
        reject(new Error(xhr.response?.error || `上传失败（HTTP ${xhr.status}）`))
      }
    }
    xhr.onerror = () => reject(new Error('网络错误'))
    xhr.onabort = () => reject(new Error('已取消'))
  })
  xhr.send(blob)
  return { promise, abort: () => xhr.abort() }
}
