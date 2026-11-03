/**
 * Resumable uploads for the file manager.
 *
 * Every file goes up in chunks; the server appends each one to a hidden
 * partial file and answers with the size it now holds. A dropped or slow
 * connection only costs that chunk: the uploader waits a moment, asks the
 * server how much arrived, and carries on from there. Folders keep their
 * structure (the name is the relative path).
 */
import axios from 'axios'
import { computed, reactive } from 'vue'

export type UploadStatus = 'waiting' | 'uploading' | 'retrying' | 'done' | 'failed' | 'canceled'

export interface UploadItem {
  id: string
  file: File
  /** Directory the upload goes into. */
  dir: string
  /** File name, or a relative path inside `dir` for folder uploads. */
  name: string
  mode: string
  loaded: number
  speed: number
  status: UploadStatus
  error: string
  /** Attempts in a row that moved nothing; reset whenever bytes arrive. */
  attempt: number
  chunk: number
  controller?: AbortController
}

/** Chunks start at 4 MB, halve after a failed attempt (a poor link gets
 * pieces it can carry) and grow back after successful ones. */
const CHUNK_MAX = 4 * 1024 * 1024
const CHUNK_MIN = 256 * 1024
const PARALLEL = 3
/** Waits before each retry; after the last one the file is marked failed. */
const BACKOFF_MS = [1000, 2000, 4000, 8000, 15000, 15000, 30000, 30000]
const CHUNK_TIMEOUT_MS = 5 * 60 * 1000

// A client of its own: retries must not flash the global error toasts.
const http = axios.create({ baseURL: '/api' })
http.interceptors.request.use((config) => {
  const token = localStorage.getItem('token')
  if (token) config.headers.Authorization = `Bearer ${token}`
  return config
})

/** Read fresh: cancel() may change the status while an upload awaits. */
const isCanceled = (item: UploadItem) => item.status === 'canceled'

class ServerRefused extends Error {}
class SessionExpired extends Error {}

function newId() {
  const bytes = crypto.getRandomValues(new Uint8Array(12))
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

const sleep = (ms: number, signal?: AbortSignal) =>
  new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, ms)
    signal?.addEventListener('abort', () => {
      clearTimeout(timer)
      resolve()
    })
  })

function retryable(err: any) {
  if (err instanceof ServerRefused || err instanceof SessionExpired) return false
  const status = err?.response?.status
  // No response (network), timeouts, and gateway / server hiccups.
  return !status || status === 408 || status === 429 || status >= 500
}

export function createUploader(onFileDone: (item: UploadItem) => void) {
  const queue = reactive<UploadItem[]>([])
  let running = 0
  /** Lowered for good when a proxy rejects a chunk size (HTTP 413). */
  let chunkCeiling = CHUNK_MAX
  /** Set when the session expired: nothing more is sent until a retry. */
  let halted = false

  const active = computed(() => queue.some((u) => u.status === 'waiting' || u.status === 'uploading' || u.status === 'retrying'))
  const counted = computed(() => queue.filter((u) => u.status !== 'canceled'))
  const doneCount = computed(() => queue.filter((u) => u.status === 'done').length)
  const failedCount = computed(() => queue.filter((u) => u.status === 'failed').length)
  const totalBytes = computed(() => counted.value.reduce((n, u) => n + u.file.size, 0))
  const loadedBytes = computed(() => counted.value.reduce((n, u) => n + (u.status === 'done' ? u.file.size : u.loaded), 0))
  const percent = computed(() => (totalBytes.value ? Math.min(100, Math.floor((loadedBytes.value / totalBytes.value) * 100)) : 100))
  const speed = computed(() => queue.reduce((n, u) => n + (u.status === 'uploading' ? u.speed : 0), 0))

  function add(files: { file: File; name: string }[], dir: string, mode: string) {
    for (const { file, name } of files) {
      queue.push({ id: newId(), file, dir, name, mode, loaded: 0, speed: 0, status: 'waiting', error: '', attempt: 0, chunk: chunkCeiling })
    }
    halted = false
    pump()
  }

  function pump() {
    while (!halted && running < PARALLEL) {
      const item = queue.find((u) => u.status === 'waiting')
      if (!item) return
      running++
      void upload(item).finally(() => {
        running--
        pump()
      })
    }
  }

  async function serverSize(item: UploadItem): Promise<number | null> {
    try {
      const res = await http.get('/files/upload/status', {
        params: { dir: item.dir, name: item.name, id: item.id },
        timeout: 30000,
        signal: item.controller?.signal,
      })
      return res.data?.success ? Number(res.data.data.size) : null
    } catch {
      return null
    }
  }

  async function sendChunk(item: UploadItem, offset: number) {
    const end = Math.min(offset + item.chunk, item.file.size)
    const blob = item.file.slice(offset, end)
    let lastAt = performance.now()
    let lastLoaded = 0
    const res = await http.post('/files/upload/chunk', blob, {
      params: { dir: item.dir, name: item.name, id: item.id, offset, total: item.file.size, mode: item.mode },
      headers: { 'Content-Type': 'application/octet-stream' },
      timeout: CHUNK_TIMEOUT_MS,
      signal: item.controller?.signal,
      onUploadProgress: (event) => {
        item.loaded = offset + Math.min(event.loaded, end - offset)
        const now = performance.now()
        if (now - lastAt >= 500) {
          item.speed = ((event.loaded - lastLoaded) * 1000) / (now - lastAt)
          lastAt = now
          lastLoaded = event.loaded
        }
      },
    }).catch((err) => {
      if (err?.response?.status === 401) throw new SessionExpired('login')
      throw err
    })
    const body = res.data
    if (!body?.success) throw new ServerRefused(String(body?.message || 'Upload failed'))
    return body.data as { size: number; complete: boolean }
  }

  async function upload(item: UploadItem) {
    item.status = 'uploading'
    item.error = ''
    item.attempt = 0
    item.controller = new AbortController()
    let offset = item.loaded > 0 ? ((await serverSize(item)) ?? 0) : 0
    try {
      for (;;) {
        if (isCanceled(item)) return
        try {
          const state = await sendChunk(item, offset)
          offset = state.size
          item.loaded = offset
          item.attempt = 0
          item.chunk = Math.min(chunkCeiling, item.chunk * 2)
          if (item.status === 'retrying') item.status = 'uploading'
          if (state.complete) {
            item.status = 'done'
            item.speed = 0
            onFileDone(item)
            return
          }
        } catch (err: any) {
          if (isCanceled(item) || axios.isCancel(err)) return
          // A reverse proxy in front of the panel limits request bodies
          // (nginx: 1 MB by default): send smaller chunks right away.
          if (err?.response?.status === 413 && item.chunk > CHUNK_MIN) {
            chunkCeiling = Math.max(CHUNK_MIN, Math.min(chunkCeiling, Math.floor(item.chunk / 2)))
            item.chunk = chunkCeiling
            const size = await serverSize(item)
            if (size !== null) offset = size
            continue
          }
          if (err instanceof SessionExpired) {
            halted = true
            item.status = 'failed'
            item.error = 'session'
            return
          }
          if (!retryable(err) || item.attempt >= BACKOFF_MS.length) {
            item.status = 'failed'
            item.error = err instanceof ServerRefused ? err.message : 'network'
            return
          }
          item.status = 'retrying'
          item.speed = 0
          item.chunk = Math.max(CHUNK_MIN, Math.floor(item.chunk / 2))
          await sleep(BACKOFF_MS[item.attempt], item.controller?.signal)
          if (isCanceled(item)) return
          // Continue from what actually arrived; only attempts that moved
          // nothing count towards giving up.
          const size = await serverSize(item)
          if (size !== null && size > offset) item.attempt = 0
          else item.attempt++
          if (size !== null) offset = size
          item.loaded = offset
        }
      }
    } finally {
      item.controller = undefined
    }
  }

  function cancel(item: UploadItem) {
    if (item.status === 'done' || item.status === 'canceled') return
    const wasStarted = item.status !== 'waiting'
    item.status = 'canceled'
    item.speed = 0
    item.controller?.abort()
    if (wasStarted) {
      void http.post('/files/upload/abort', { dir: item.dir, name: item.name, id: item.id }).catch(() => undefined)
    }
  }

  function cancelAll() {
    queue.forEach(cancel)
  }

  function retry(item: UploadItem) {
    if (item.status !== 'failed' && item.status !== 'canceled') return
    // A canceled upload's partial file is gone: start over with a new id.
    if (item.status === 'canceled') {
      item.id = newId()
      item.loaded = 0
    }
    item.status = 'waiting'
    halted = false
    pump()
  }

  function retryFailed() {
    queue.filter((u) => u.status === 'failed').forEach(retry)
  }

  function clearFinished() {
    for (let i = queue.length - 1; i >= 0; i--) {
      if (['done', 'canceled', 'failed'].includes(queue[i].status)) queue.splice(i, 1)
    }
  }

  return { queue, active, doneCount, failedCount, totalBytes, loadedBytes, percent, speed, add, cancel, cancelAll, retry, retryFailed, clearFinished }
}

/** Files of a drop, folders included, with their paths relative to the drop. */
export async function droppedFiles(transfer: DataTransfer): Promise<{ file: File; name: string }[]> {
  const items = Array.from(transfer.items || []).filter((item) => item.kind === 'file')
  const entries = items.map((item) => (item as any).webkitGetAsEntry?.() as any).filter(Boolean)
  if (!entries.length) return Array.from(transfer.files || []).map((file) => ({ file, name: file.name }))
  const out: { file: File; name: string }[] = []
  const walk = async (entry: any, prefix: string): Promise<void> => {
    if (entry.isFile) {
      const file: File = await new Promise((resolve, reject) => entry.file(resolve, reject))
      out.push({ file, name: prefix + entry.name })
      return
    }
    if (entry.isDirectory) {
      const reader = entry.createReader()
      // readEntries returns batches until an empty one.
      for (;;) {
        const batch: any[] = await new Promise((resolve, reject) => reader.readEntries(resolve, reject))
        if (!batch.length) break
        for (const child of batch) await walk(child, `${prefix}${entry.name}/`)
      }
    }
  }
  for (const entry of entries) await walk(entry, '')
  return out
}

/** Files picked with a folder input (`webkitdirectory`). */
export function pickedFolderFiles(list: FileList | File[]): { file: File; name: string }[] {
  return Array.from(list).map((file) => ({ file, name: (file as any).webkitRelativePath || file.name }))
}
