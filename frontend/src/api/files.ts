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
