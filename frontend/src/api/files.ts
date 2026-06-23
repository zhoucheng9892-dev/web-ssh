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

  /** Multipart upload of one or more files to a remote directory. */
  upload: (connectionId: number, dir: string, files: File[]) => {
    const form = new FormData()
    for (const f of files) form.append('files', f, f.name)
    return http
      .post<{ uploaded: string[] }>('/files/upload', form, params(connectionId, dir))
      .then((r) => r.data)
  },

  /** Full download URL (lets the browser stream the file directly). */
  downloadUrl: (connectionId: number, path: string) =>
    `${appPath('api/files/download')}?connection_id=${connectionId}&path=${encodeURIComponent(path)}`,
}
