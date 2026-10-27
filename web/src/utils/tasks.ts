import { ElMessage } from 'element-plus'
import { useTaskStore } from '@/stores/tasks'
import type { TaskEnvelope } from '@/stores/tasks'
import { localizeApiMessage } from '@/utils/request'

/**
 * Runs a long server operation as a background task: `start` posts the
 * request (the server answers with a task id at once), the task window
 * streams its log, and this resolves with the operation's usual
 * `{ success, message, data }` - so callers handle it exactly as before.
 */
export async function runTask(
  title: string,
  start: () => Promise<any>,
  options: { closeOnSuccess?: boolean } = {},
): Promise<TaskEnvelope> {
  const started: any = await start()
  const id = started?.success ? started.data?.task_id : null
  if (!id) return started as TaskEnvelope
  const store = useTaskStore()
  const result = await store.follow(id, title)
  // Callers that show their own result (e.g. new credentials) take over.
  if (result.success && options.closeOnSuccess && store.visibleId === id) store.show(null)
  if (!result.success) {
    // Same feedback as a failed ordinary request.
    ElMessage.error(localizeApiMessage(result.message))
  }
  return result
}
