<template>
  <el-dialog
    v-model="dialogVisible"
    :width="width"
    :close-on-click-modal="false"
    :append-to-body="true"
    class="operation-log-dialog"
    top="8vh"
  >
    <template #header>
      <div class="oplog-header">
        <span class="oplog-title">{{ title }}</span>
        <el-tag size="small" :type="statusTagType" effect="dark">{{ statusText }}</el-tag>
      </div>
      <div v-if="description" class="oplog-desc">{{ description }}</div>
    </template>

    <div class="oplog-toolbar">
      <div class="oplog-toolbar-left">
        <el-switch v-model="autoScroll" size="small" />
        <span>{{ t('lg.autoScroll') }}</span>
      </div>
      <div class="oplog-toolbar-right">
        <el-button text size="small" @click="scrollToBottom">{{ t('lg.scrollBottom') }}</el-button>
        <el-button text size="small" :disabled="!rawLog" @click="copyLog">{{ t('copy') }}</el-button>
        <el-button v-if="showClear" text size="small" @click="$emit('clear')">{{ t('lg.clearLog') }}</el-button>
      </div>
    </div>

    <div ref="logWrapRef" class="oplog-content" v-loading="loading">
      <pre>{{ rawLog || t('lg.noLog') }}</pre>
    </div>

    <template #footer>
      <el-button @click="dialogVisible = false">{{ t('close') }}</el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { useI18n } from '@/i18n'

type LogStatus = 'idle' | 'running' | 'success' | 'error'

const props = withDefaults(
  defineProps<{
    modelValue: boolean
    title: string
    description?: string
    log?: string
    status?: LogStatus
    loading?: boolean
    width?: string
    showClear?: boolean
  }>(),
  {
    description: '',
    log: '',
    status: 'idle',
    loading: false,
    width: '860px',
    showClear: true,
  },
)

const emit = defineEmits<{
  (event: 'update:modelValue', value: boolean): void
  (event: 'clear'): void
}>()

const { t } = useI18n()
const autoScroll = ref(true)
const logWrapRef = ref<HTMLElement | null>(null)

const dialogVisible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})

const rawLog = computed(() => String(props.log || ''))

const statusText = computed(() => {
  if (props.status === 'running') return t('lg.statusRunning')
  if (props.status === 'success') return t('lg.statusSuccess')
  if (props.status === 'error') return t('lg.statusError')
  return t('lg.statusIdle')
})

const statusTagType = computed(() => {
  if (props.status === 'running') return 'warning'
  if (props.status === 'success') return 'success'
  if (props.status === 'error') return 'danger'
  return 'info'
})

async function copyLog() {
  if (!rawLog.value) return
  try {
    await navigator.clipboard.writeText(rawLog.value)
    ElMessage.success(t('fm.copied'))
  } catch {
    ElMessage.error(t('failed'))
  }
}

function scrollToBottom() {
  const element = logWrapRef.value
  if (!element) return
  element.scrollTop = element.scrollHeight
}

watch(
  () => props.modelValue,
  async (visible) => {
    if (!visible) return
    await nextTick()
    scrollToBottom()
  },
)

watch(
  () => props.log,
  async () => {
    if (!props.modelValue || !autoScroll.value) return
    await nextTick()
    scrollToBottom()
  },
)
</script>

<style scoped lang="scss">
.oplog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.oplog-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--lp-text-primary);
}
.oplog-desc {
  margin-top: 8px;
  font-size: 12px;
  color: var(--lp-text-secondary);
  line-height: 1.5;
}
.oplog-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 10px;
}
.oplog-toolbar-left {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--lp-text-secondary);
  font-size: 12px;
}
.oplog-toolbar-right {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}
.oplog-content {
  height: 460px;
  max-height: 58vh;
  overflow: auto;
  border: 1px solid var(--lp-border-light);
  border-radius: var(--lp-radius);
  background: var(--lp-bg-page);
  padding: 10px;
  pre {
    margin: 0;
    font-size: 12px;
    line-height: 1.55;
    color: var(--lp-text-primary);
    white-space: pre-wrap;
    word-break: break-word;
    font-family: 'Cascadia Code', 'Fira Code', monospace;
  }
}
:deep(.operation-log-dialog .el-dialog) {
  border: 1px solid var(--lp-border-light);
  border-radius: var(--lp-radius-xl);
  overflow: hidden;
}
:deep(.operation-log-dialog .el-dialog__header) {
  border-bottom: 1px solid var(--lp-border-light);
  padding: 14px 16px 10px;
}
:deep(.operation-log-dialog .el-dialog__body) {
  padding: 12px 16px 14px;
  background: var(--lp-card-bg);
}
:deep(.operation-log-dialog .el-dialog__footer) {
  border-top: 1px solid var(--lp-border-light);
  padding: 10px 16px 12px;
  background: var(--lp-card-bg);
}
</style>
