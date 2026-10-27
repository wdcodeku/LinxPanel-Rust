import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

/** A long operation running on the server (install, deploy...). */
export type TaskView = {
  id: string
  title: string
  status: 'running' | 'success' | 'failed'
  log: string
  offset: number
  startedAt: number
  finishedAt: number | null
  message: string
}

export type TaskEnvelope = { success: boolean; message: string; data: any }

// Plain fetch: a blip while following a task must not raise error toasts.
async function fetchProgress(id: string, offset: number) {
  try {
    const token = localStorage.getItem('token')
    const response = await fetch(`/api/tasks/${encodeURIComponent(id)}?offset=${offset}`, {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
      cache: 'no-store',
    })
    if (!response.ok) return null
    const body = await response.json()
    return body?.success ? body.data : null
  } catch {
    return null
  }
}

const MAX_LOG_CHARS = 400_000

export const useTaskStore = defineStore('tasks', () => {
  const tasks = ref<TaskView[]>([])
  /** The task shown in the log window; null when it is closed. */
  const visibleId = ref<string | null>(null)

  const running = computed(() => tasks.value.filter((t) => t.status === 'running'))
  const visible = computed(() => tasks.value.find((t) => t.id === visibleId.value) || null)

  function show(id: string | null) {
    visibleId.value = id
  }

  /**
   * Follows task `id` until it ends; resolves with the endpoint's own
   * `{ success, message, data }` envelope.
   */
  function follow(id: string, title: string): Promise<TaskEnvelope> {
    const view: TaskView = {
      id, title, status: 'running', log: '', offset: 0,
      startedAt: Date.now(), finishedAt: null, message: '',
    }
    tasks.value = [view, ...tasks.value.filter((t) => t.id !== id)].slice(0, 20)
    visibleId.value = id
    const task = () => tasks.value.find((t) => t.id === id)!

    return new Promise((resolve) => {
      let misses = 0
      const tick = async () => {
        const progress = await fetchProgress(id, task().offset)
        if (!progress) {
          // Server restarting or task gone: give up after ~2 minutes.
          if (++misses > 120) {
            Object.assign(task(), { status: 'failed', finishedAt: Date.now(), message: 'Lost track of the task' })
            resolve({ success: false, message: 'Lost track of the task', data: null })
            return
          }
          setTimeout(tick, 1000)
          return
        }
        misses = 0
        const current = task()
        if (progress.log) {
          current.log += progress.log
          if (current.log.length > MAX_LOG_CHARS) current.log = current.log.slice(-MAX_LOG_CHARS)
        }
        current.offset = progress.next_offset
        if (progress.success === null || progress.success === undefined) {
          setTimeout(tick, 700)
          return
        }
        current.status = progress.success ? 'success' : 'failed'
        current.finishedAt = Date.now()
        current.message = progress.message || ''
        resolve({ success: progress.success, message: progress.message, data: progress.data })
      }
      tick()
    })
  }

  return { tasks, visibleId, visible, running, show, follow }
})
