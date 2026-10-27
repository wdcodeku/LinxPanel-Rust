<template>
  <el-dialog
    :model-value="!!task"
    :title="task?.title"
    width="760px"
    class="task-dialog"
    :close-on-click-modal="false"
    append-to-body
    @close="store.show(null)"
  >
    <template v-if="task">
      <div class="task-head">
        <span class="lp-pill" :class="pillClass">
          <el-icon v-if="task.status === 'running'" class="spin"><Loading /></el-icon>
          <span v-else class="lp-pill__dot"></span>
          {{ statusText }}
        </span>
        <span class="lp-dim task-time">{{ elapsed }}</span>
        <span v-if="task.status === 'failed' && task.message" class="task-error">{{ task.message }}</span>
      </div>
      <pre ref="logRef" class="task-log" @scroll="onScroll">{{ task.log || t('task.waiting') }}</pre>
      <p v-if="task.status === 'running'" class="lp-dim task-hint">{{ t('task.backgroundHint') }}</p>
    </template>
    <template #footer>
      <el-button v-if="task?.status === 'running'" @click="store.show(null)">{{ t('task.background') }}</el-button>
      <el-button v-else type="primary" @click="store.show(null)">{{ t('task.close') }}</el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElNotification } from 'element-plus'
import { Loading } from '@element-plus/icons-vue'
import { useTaskStore } from '@/stores/tasks'
import { useI18n } from '@/i18n'

const { t } = useI18n()
const store = useTaskStore()
const task = computed(() => store.visible)
const logRef = ref<HTMLElement>()

const pillClass = computed(() => ({
  running: 'lp-pill--accent',
  success: 'lp-pill--ok',
  failed: 'lp-pill--danger',
}[task.value?.status || 'running']))
const statusText = computed(() => t(`task.status.${task.value?.status || 'running'}`))

// Elapsed time, ticking while running.
const now = ref(Date.now())
let clock: ReturnType<typeof setInterval> | undefined
onMounted(() => { clock = setInterval(() => (now.value = Date.now()), 1000) })
onBeforeUnmount(() => clearInterval(clock))
const elapsed = computed(() => {
  if (!task.value) return ''
  const end = task.value.finishedAt || now.value
  const secs = Math.max(0, Math.round((end - task.value.startedAt) / 1000))
  return secs >= 60 ? `${Math.floor(secs / 60)}m ${secs % 60}s` : `${secs}s`
})

// Follow the log unless the user scrolled up to read.
const pinned = ref(true)
function onScroll() {
  const el = logRef.value
  if (el) pinned.value = el.scrollHeight - el.scrollTop - el.clientHeight < 40
}
watch(() => [task.value?.id, task.value?.log.length], async ([id], [prevId]) => {
  if (id !== prevId) pinned.value = true
  if (!pinned.value) return
  await nextTick()
  logRef.value?.scrollTo({ top: logRef.value.scrollHeight })
})

// A task finishing in the background still reports back.
watch(
  () => store.tasks.map((x) => `${x.id}:${x.status}`).join(','),
  (_now, before) => {
    for (const x of store.tasks) {
      if (x.status === 'running' || x.id === store.visibleId) continue
      if (!before?.includes(`${x.id}:running`)) continue
      ElNotification({
        title: x.title,
        message: x.status === 'success' ? t('task.doneOk') : t('task.doneFailed'),
        type: x.status === 'success' ? 'success' : 'error',
        onClick: () => store.show(x.id),
      })
    }
  },
)
</script>

<style scoped lang="scss">
.task-head {
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  margin-bottom: var(--lp-3);
  flex-wrap: wrap;
}
.task-time { font-size: var(--lp-text-xs); font-variant-numeric: tabular-nums; }
.task-error { color: var(--lp-danger); font-size: var(--lp-text-xs); }
.task-log {
  margin: 0;
  height: min(52vh, 460px);
  overflow: auto;
  padding: var(--lp-3) var(--lp-4);
  border-radius: var(--lp-r-sm);
  /* A terminal look in both themes. */
  background: #0f1117;
  color: #d6dae2;
  font-family: var(--lp-font-mono);
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
  word-break: break-word;
}
.task-hint { margin: var(--lp-2) 0 0; font-size: var(--lp-text-xs); }
.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
</style>
