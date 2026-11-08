<template>
  <el-dialog
    :model-value="modelValue"
    :fullscreen="fullscreen"
    :width="fullscreen ? '100%' : 'min(1200px, 96vw)'"
    top="3vh"
    append-to-body
    destroy-on-close
    :close-on-click-modal="false"
    :close-on-press-escape="false"
    :before-close="requestClose"
    class="fe-dialog"
    :class="{ 'is-full': fullscreen }"
  >
    <template #header>
      <div class="fe-head">
        <el-icon class="fe-head__icon"><Document /></el-icon>
        <span class="fe-head__name" :title="path">{{ fileName }}</span>
        <span v-if="dirty" class="fe-dirty" :title="t('ed.unsaved')">●</span>
        <span class="fe-head__dir lp-mono lp-truncate" :title="path">{{ dirName }}</span>
      </div>
    </template>

    <div class="fe-toolbar">
      <el-select v-model="language" size="small" filterable class="fe-lang">
        <el-option v-for="opt in LANGUAGES" :key="opt.id" :label="opt.label" :value="opt.id" />
      </el-select>
      <el-tooltip :content="t('ed.search')" :show-after="300">
        <el-button size="small" :icon="Search" @click="editor?.search()" />
      </el-tooltip>
      <el-tooltip :content="t('ed.gotoLine')" :show-after="300">
        <el-button size="small" @click="editor?.gotoLine()">#</el-button>
      </el-tooltip>
      <el-tooltip :content="t('ed.wrap')" :show-after="300">
        <el-button size="small" :type="wrap ? 'primary' : 'default'" :plain="wrap" @click="wrap = !wrap">
          <el-icon><Tickets /></el-icon>
        </el-button>
      </el-tooltip>
      <el-button-group size="small">
        <el-button size="small" @click="zoom(-1)">A−</el-button>
        <el-button size="small" @click="zoom(1)">A+</el-button>
      </el-button-group>
      <el-tooltip :content="t('ed.reload')" :show-after="300">
        <el-button size="small" :icon="RefreshRight" @click="reload" />
      </el-tooltip>
      <span class="fe-spacer"></span>
      <el-tooltip :content="fullscreen ? t('tm.restore') : t('tm.maximize')" :show-after="300">
        <el-button size="small" @click="fullscreen = !fullscreen"><el-icon><component :is="fullscreen ? 'CopyDocument' : 'FullScreen'" /></el-icon></el-button>
      </el-tooltip>
    </div>

    <div class="fe-body" :class="{ 'is-full': fullscreen }" v-loading="loading">
      <el-result v-if="error" icon="warning" :title="t('ed.cannotOpen')" :sub-title="error" />
      <CodeEditor
        v-else-if="loaded"
        ref="editor"
        v-model="content"
        :language="language"
        :dark="dark"
        :wrap="wrap"
        :font-size="fontSize"
        :line-ending="lineEnding"
        @save="save()"
        @cursor="(c) => (cursor = c)"
      />
    </div>

    <template #footer>
      <div class="fe-foot">
        <span class="fe-status lp-mono">
          {{ t('ed.position', { line: cursor.line, col: cursor.col }) }}
          <template v-if="cursor.selected"> · {{ t('ed.selected', { n: cursor.selected }) }}</template>
          · {{ t('ed.lines', { n: cursor.lines }) }} · {{ formatBytes(byteSize) }} · UTF-8 · {{ lineEnding === '\r\n' ? 'CRLF' : 'LF' }}
        </span>
        <span class="fe-hint">{{ t('ed.keys') }}</span>
        <el-button @click="requestClose()">{{ t('close') }}</el-button>
        <el-button type="primary" :loading="saving" :disabled="!loaded || !!error" @click="save()">{{ t('save') }} <span class="fe-key">Ctrl+S</span></el-button>
      </div>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Document, RefreshRight, Search, Tickets } from '@element-plus/icons-vue'
import CodeEditor from './CodeEditor.vue'
import { LANGUAGES, detectLanguage } from './languages'
import { readFileContent, writeFileContent } from '@/api'
import { useI18n } from '@/i18n'
import { useUserStore } from '@/stores/user'
import { formatBytes } from '@/utils/format'

const props = defineProps<{ modelValue: boolean; path: string }>()
const emit = defineEmits<{ (e: 'update:modelValue', v: boolean): void; (e: 'saved', path: string): void }>()
const { t } = useI18n()
const userStore = useUserStore()

const editor = ref<InstanceType<typeof CodeEditor> | null>(null)
const loading = ref(false)
const loaded = ref(false)
const saving = ref(false)
const error = ref('')
const content = ref('')
const original = ref('')
const version = ref('')
const lineEnding = ref('\n')
const language = ref('text')
const cursor = ref({ line: 1, col: 1, selected: 0, lines: 1 })
const fullscreen = ref(false)

// Preferences that follow the user from file to file.
function pref<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(`ed_${key}`)
    return raw === null ? fallback : (JSON.parse(raw) as T)
  } catch {
    return fallback
  }
}
const wrap = ref<boolean>(pref('wrap', false))
const fontSize = ref<number>(pref('font', 13))
watch([wrap, fontSize], () => {
  try {
    localStorage.setItem('ed_wrap', JSON.stringify(wrap.value))
    localStorage.setItem('ed_font', JSON.stringify(fontSize.value))
  } catch { /* private mode */ }
})
function zoom(step: number) {
  fontSize.value = Math.min(22, Math.max(10, fontSize.value + step))
}

const dark = computed(() => userStore.theme === 'dark')
const dirty = computed(() => loaded.value && content.value !== original.value)
const fileName = computed(() => props.path.slice(props.path.lastIndexOf('/') + 1))
const dirName = computed(() => props.path.slice(0, props.path.lastIndexOf('/')) || '/')
const byteSize = computed(() => new TextEncoder().encode(content.value).length)

async function load() {
  loading.value = true
  error.value = ''
  loaded.value = false
  try {
    const res: any = await readFileContent(props.path)
    if (!res.success) {
      error.value = res.message
      return
    }
    content.value = res.data.content
    original.value = res.data.content
    version.value = res.data.version || ''
    lineEnding.value = res.data.line_ending === '\r\n' ? '\r\n' : '\n'
    language.value = detectLanguage(props.path, res.data.content)
    loaded.value = true
  } catch (e: any) {
    error.value = e?.message || t('fm.readFailed')
  } finally {
    loading.value = false
  }
}

watch(
  () => [props.modelValue, props.path] as const,
  ([open]) => {
    if (open) {
      fullscreen.value = false
      void load()
    }
  },
  { immediate: true },
)

async function save(force = false) {
  if (!loaded.value || saving.value) return
  saving.value = true
  try {
    const res: any = await writeFileContent(props.path, content.value, { expected_version: version.value || undefined, force })
    if (res.success) {
      original.value = content.value
      version.value = res.data?.version || version.value
      ElMessage.success(t('fm.saved'))
      emit('saved', props.path)
      return
    }
    if (res.message === 'CHANGED_ON_DISK') {
      await resolveConflict()
      return
    }
    ElMessage.error(res.message)
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

/** The file changed on disk since it was opened: overwrite, or reload theirs. */
async function resolveConflict() {
  try {
    await ElMessageBox.confirm(t('ed.conflict'), t('ed.conflictTitle'), {
      type: 'warning',
      distinguishCancelAndClose: true,
      confirmButtonText: t('ed.overwrite'),
      cancelButtonText: t('ed.reloadTheirs'),
    })
    saving.value = false
    await save(true)
  } catch (action) {
    if (action === 'cancel') await load()
  }
}

async function reload() {
  if (dirty.value) {
    try {
      await ElMessageBox.confirm(t('ed.discard'), t('ed.reload'), { type: 'warning' })
    } catch {
      return
    }
  }
  await load()
}

async function requestClose(done?: () => void) {
  if (dirty.value) {
    try {
      await ElMessageBox.confirm(t('ed.closeUnsaved'), fileName.value, {
        type: 'warning',
        distinguishCancelAndClose: true,
        confirmButtonText: t('ed.saveAndClose'),
        cancelButtonText: t('ed.discardChanges'),
      })
      await save()
      if (dirty.value) return // the save did not go through
    } catch (action) {
      if (action !== 'cancel') return // closed the box: keep editing
    }
  }
  if (done) done()
  // Always tell the parent: with only done() its v-model stayed true, and
  // opening a file again changed nothing until the page was reloaded.
  emit('update:modelValue', false)
}
</script>

<style scoped lang="scss">
.fe-head { display: flex; align-items: center; gap: 8px; min-width: 0; padding-right: 24px; }
.fe-head__icon { color: var(--lp-accent); }
.fe-head__name { font-weight: 600; white-space: nowrap; }
.fe-head__dir { flex: 1; min-width: 0; font-size: 12px; color: var(--lp-text-dim); }
.fe-dirty { color: var(--lp-warn); font-size: 12px; }
.fe-toolbar { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; margin-bottom: 8px; }
.fe-lang { width: 150px; }
.fe-spacer { flex: 1; }
.fe-body {
  height: min(68vh, 760px);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-sm);
  overflow: hidden;
}
.fe-body.is-full { height: calc(100vh - 170px); }
.fe-foot { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.fe-status { margin-right: auto; font-size: 12px; color: var(--lp-text-dim); text-align: left; }
.fe-hint { font-size: 11px; color: var(--lp-text-faint, var(--lp-text-dim)); }
.fe-key { margin-left: 6px; font-size: 11px; opacity: 0.75; }
@media (max-width: 760px) {
  .fe-hint { display: none; }
  .fe-body { height: 60vh; }
}
</style>

<style lang="scss">
.fe-dialog .el-dialog__body { padding-top: 4px; padding-bottom: 4px; }
</style>
