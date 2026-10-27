<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.runtime') }}</h1>
        <p class="lp-page-sub">{{ subtitle }}</p>
      </div>
      <div class="lp-page-actions">
        <el-button size="small" :icon="Refresh" :loading="loading" @click="loadAll">{{ t('refresh') }}</el-button>
        <el-button v-if="tab === 'apps'" type="primary" size="small" :icon="Plus" @click="openCreate">
          {{ t('rt.newApp') }}
        </el-button>
      </div>
    </div>

    <el-tabs v-model="tab">
      <!-- ============ Runtimes ============ -->
      <el-tab-pane :label="t('rt.environments')" name="runtimes">
        <div class="rt-grid">
          <article v-for="item in runtimes" :key="item.kind" class="rt-card" :class="{ 'rt-card--on': item.installed }">
            <div class="rt-card__head">
              <span class="rt-logo" :data-kind="item.kind">{{ logoText(item.kind) }}</span>
              <div class="rt-title">
                <span class="rt-name">{{ item.display_name }}</span>
                <span class="lp-pill" :class="item.installed ? 'lp-pill--ok' : ''">
                  <span class="lp-pill__dot"></span>
                  {{ item.installed ? item.version || t('rt.installed') : t('rt.notInstalled') }}
                </span>
              </div>
            </div>

            <div v-if="item.installed && item.path" class="rt-path lp-mono lp-truncate" :title="item.path">
              {{ item.path }}
            </div>

            <ul v-if="item.components.length" class="rt-components">
              <li v-for="component in item.components" :key="component.name" class="rt-component">
                <span class="rt-component__dot" :class="{ 'is-on': component.installed }"></span>
                <span class="rt-component__name">{{ component.name }}</span>
                <span class="rt-component__ver lp-num">{{ component.installed ? component.version : '—' }}</span>
              </li>
            </ul>

            <div class="rt-card__foot">
              <span v-if="item.message" class="rt-msg lp-truncate" :title="item.message">{{ item.message }}</span>
              <span v-else-if="item.installed" class="rt-msg">{{ t('rt.readyForApps') }}</span>
              <el-button
                v-if="!item.installed"
                type="primary"
                size="small"
                plain
                :disabled="!item.installable"
                :loading="installing === item.kind"
                @click="install(item)"
              >{{ t('rt.install') }}</el-button>
              <el-button
                v-else-if="item.kind === 'php'"
                size="small"
                :loading="installing === item.kind"
                @click="install(item, true)"
              >{{ t('rt.repair') }}</el-button>
            </div>
          </article>
        </div>
      </el-tab-pane>

      <!-- ============ Applications ============ -->
      <el-tab-pane :label="t('rt.applications')" name="apps">
        <div class="lp-card">
          <el-table :data="apps" v-loading="loading" size="small" :empty-text="t('rt.noApps')">
            <el-table-column :label="t('rt.appName')" min-width="170">
              <template #default="{ row }">
                <div class="cell-stack">
                  <span class="app-name">{{ row.name }}</span>
                  <span class="lp-dim lp-truncate lp-mono">{{ row.work_dir }}</span>
                </div>
              </template>
            </el-table-column>
            <el-table-column :label="t('rt.runtime')" width="110">
              <template #default="{ row }">
                <span class="lp-pill lp-pill--accent">{{ runtimeLabel(row.runtime) }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('rt.entry')" min-width="150">
              <template #default="{ row }">
                <code class="lp-mono lp-truncate entry">{{ row.entry }} {{ row.args }}</code>
              </template>
            </el-table-column>
            <el-table-column :label="t('rt.port')" width="80">
              <template #default="{ row }">
                <span class="lp-num">{{ row.port || '—' }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('rt.state')" width="150">
              <template #default="{ row }">
                <span class="lp-pill" :class="row.running ? 'lp-pill--ok' : ''">
                  <span class="lp-pill__dot"></span>{{ row.running ? t('rt.running') : t('rt.stopped') }}
                </span>
                <span v-if="row.running && row.pid" class="lp-dim pid">pid {{ row.pid }}</span>
                <div v-if="row.restarts" class="lp-dim restarts">{{ t('rt.restarts', { n: row.restarts }) }}</div>
              </template>
            </el-table-column>
            <el-table-column :label="t('rt.uptimeCol')" width="110">
              <template #default="{ row }">
                <span class="lp-num lp-dim">{{ row.running ? uptimeOf(row) : '—' }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('proc.actions')" width="250" align="right">
              <template #default="{ row }">
                <el-button
                  v-if="!row.running"
                  link
                  type="primary"
                  size="small"
                  :loading="busy === row.id"
                  @click="action(row, 'start')"
                >{{ t('rt.start') }}</el-button>
                <template v-else>
                  <el-button link size="small" :loading="busy === row.id" @click="action(row, 'restart')">
                    {{ t('rt.restart') }}
                  </el-button>
                  <el-button link size="small" :loading="busy === row.id" @click="action(row, 'stop')">
                    {{ t('rt.stop') }}
                  </el-button>
                </template>
                <el-button
                  v-if="['python', 'node', 'php'].includes(row.runtime)"
                  link
                  size="small"
                  :loading="busy === `deps-${row.id}`"
                  @click="installDeps(row)"
                >{{ t('rt.installDeps') }}</el-button>
                <el-button link size="small" @click="openLogs(row)">{{ t('rt.logs') }}</el-button>
                <el-button link size="small" @click="openEdit(row)">{{ t('cron.edit') }}</el-button>
                <el-button link type="danger" size="small" @click="remove(row)">{{ t('delete') }}</el-button>
              </template>
            </el-table-column>
          </el-table>
        </div>
      </el-tab-pane>
    </el-tabs>

    <!-- App editor -->
    <el-dialog v-model="formVisible" :title="editing ? t('rt.editApp') : t('rt.newApp')" width="620px" top="6vh">
      <el-form ref="formRef" :model="form" :rules="rules" label-position="top">
        <div class="form-row">
          <el-form-item :label="t('rt.appName')" prop="name" class="form-row__item">
            <el-input v-model="form.name" :placeholder="t('rt.appNamePlaceholder')" maxlength="64" />
          </el-form-item>
          <el-form-item :label="t('rt.runtime')" prop="runtime" class="form-row__item form-row__item--narrow">
            <el-select v-model="form.runtime" style="width: 100%">
              <el-option v-for="kind in appRuntimes" :key="kind.value" :label="kind.label" :value="kind.value" />
            </el-select>
          </el-form-item>
        </div>

        <el-form-item :label="t('rt.workDir')" prop="work_dir">
          <el-input v-model="form.work_dir" placeholder="/opt/myapp" class="lp-mono" />
        </el-form-item>

        <div class="form-row">
          <el-form-item :label="entryLabel" prop="entry" class="form-row__item">
            <el-input v-model="form.entry" :placeholder="entryPlaceholder" class="lp-mono" />
            <span v-if="entryHint" class="entry-hint">{{ entryHint }}</span>
          </el-form-item>
          <el-form-item :label="t('rt.port')" class="form-row__item form-row__item--narrow">
            <el-input-number v-model="form.port" :min="0" :max="65535" controls-position="right" style="width: 100%" />
          </el-form-item>
        </div>

        <el-form-item :label="t('rt.args')">
          <el-input v-model="form.args" :placeholder="t('rt.argsPlaceholder')" class="lp-mono" />
        </el-form-item>

        <el-form-item :label="t('rt.env')">
          <el-input
            v-model="form.env"
            type="textarea"
            :rows="4"
            placeholder="NODE_ENV=production&#10;DATABASE_URL=..."
            class="lp-mono"
          />
          <span class="field-hint">{{ t('rt.envHint') }}</span>
        </el-form-item>

        <el-form-item>
          <el-switch v-model="form.autostart" :active-text="t('rt.autostart')" />
          <span class="field-hint field-hint--inline">{{ t('rt.autostartHint') }}</span>
        </el-form-item>
      </el-form>

      <template #footer>
        <el-button @click="formVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="submit">{{ t('save') }}</el-button>
      </template>
    </el-dialog>

    <!-- App logs -->
    <el-dialog v-model="logsVisible" :title="logsTitle" width="820px" top="6vh" @closed="stopLogPolling">
      <div class="log-view" v-loading="logsLoading">
        <pre ref="logPre" class="log-pre">{{ logsText || t('rt.noLogs') }}</pre>
      </div>
      <template #footer>
        <el-switch v-model="logsFollow" :active-text="t('rt.follow')" size="small" style="margin-right: auto" />
        <el-button size="small" @click="clearLogs">{{ t('rt.clearLogs') }}</el-button>
        <el-button size="small" :icon="Refresh" :loading="logsLoading" @click="fetchLogs">{{ t('refresh') }}</el-button>
        <el-button size="small" @click="logsVisible = false">{{ t('close') }}</el-button>
      </template>
    </el-dialog>

    <OperationLogDialog
      v-model="installLogVisible"
      :title="installLogTitle"
      :log="installLog"
      :status="installStatus"
      :loading="installing !== ''"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { usePage } from '@/utils/page'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { FormRules } from 'element-plus'
import { Plus, Refresh } from '@element-plus/icons-vue'
import OperationLogDialog from '@/components/OperationLogDialog.vue'
import {
  appAction, clearAppLogs, createApp, deleteApp, getAppLogs, getApps, getRuntimes,
  installAppDeps, installRuntime, updateApp,
} from '@/api'
import { useI18n } from '@/i18n'
import { formatDuration } from '@/utils/format'

type RuntimeComponent = { name: string; installed: boolean; version: string; path: string }
type RuntimeStatus = {
  kind: string
  display_name: string
  installed: boolean
  version: string
  path: string
  components: RuntimeComponent[]
  package_manager: string
  installable: boolean
  message: string
}
type AppRow = {
  id: number
  name: string
  runtime: string
  work_dir: string
  entry: string
  args: string
  env: string
  port: number
  autostart: boolean
  running: boolean
  pid: number | null
  started_at: number | null
  restarts: number
  last_error: string
}

const { t } = useI18n()

const tab = ref('runtimes')
const runtimes = ref<RuntimeStatus[]>([])
const apps = ref<AppRow[]>([])
const loading = ref(false)
const busy = ref<number | string | null>(null)
const installing = ref('')

let pollTimer: number | null = null

const subtitle = computed(() => {
  const ready = runtimes.value.filter((item) => item.installed).length
  const running = apps.value.filter((item) => item.running).length
  return t('rt.subtitle', { ready, total: runtimes.value.length, running, apps: apps.value.length })
})

const appRuntimes = computed(() => [
  { value: 'node', label: 'Node.js' },
  { value: 'python', label: 'Python' },
  { value: 'java', label: 'Java (jar)' },
  { value: 'go', label: t('rt.goBinary') },
  { value: 'binary', label: t('rt.binary') },
  { value: 'php', label: 'PHP (CLI)' },
])

function runtimeLabel(kind: string) {
  return appRuntimes.value.find((item) => item.value === kind)?.label || kind
}

function logoText(kind: string) {
  return { php: 'php', node: 'js', python: 'py', java: 'jv', go: 'go' }[kind] || kind.slice(0, 2)
}

function uptimeOf(row: AppRow) {
  if (!row.started_at) return '—'
  return formatDuration(Math.max(0, Math.floor(Date.now() / 1000) - row.started_at))
}

async function loadAll() {
  loading.value = true
  try {
    const [runtimeRes, appRes]: any[] = await Promise.all([getRuntimes(), getApps()])
    if (runtimeRes.success) runtimes.value = runtimeRes.data || []
    if (appRes.success) apps.value = appRes.data || []
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}

/** Refresh only the app table, so running/pid stay current without re-probing runtimes. */
async function refreshApps() {
  try {
    const res: any = await getApps()
    if (res.success) apps.value = res.data || []
  } catch {
    // Ignore; the next tick retries.
  }
}

// --- Runtime install ---
const installLogVisible = ref(false)
const installLog = ref('')
const installLogTitle = ref('')
const installStatus = ref<'idle' | 'running' | 'success' | 'error'>('idle')

async function install(item: RuntimeStatus, repair = false) {
  try {
    await ElMessageBox.confirm(
      repair
        ? t('rt.confirmRepair')
        : t('rt.confirmInstall', { name: item.display_name, manager: item.package_manager }),
      t('rt.install'),
      { type: 'warning', confirmButtonText: t('confirm'), cancelButtonText: t('cancel') },
    )
  } catch {
    return
  }

  installing.value = item.kind
  installLogTitle.value = `${item.display_name} — ${t('rt.install')}`
  installLog.value = t('rt.installRunning')
  installStatus.value = 'running'
  try {
    const res: any = await installRuntime(item.kind)
    if (res.success) {
      installLog.value = res.data?.log || ''
      installStatus.value = res.data?.status?.installed && !/failed/i.test(res.data?.log || '') ? 'success' : 'error'
      if (res.data?.changed) ElMessage.success(t('rt.installed'))
      await loadAll()
    } else {
      installLog.value = res.message || ''
      installStatus.value = 'error'
    }
  } catch (err: any) {
    installLog.value = err?.message || ''
    installStatus.value = 'error'
  } finally {
    installing.value = ''
  }
}

// --- App form ---
const formVisible = ref(false)
const saving = ref(false)
const editing = ref<AppRow | null>(null)
const formRef = ref()
const form = reactive({
  name: '',
  runtime: 'node',
  work_dir: '',
  entry: '',
  args: '',
  env: '',
  port: 3000,
  autostart: true,
})

const entryLabel = computed(() =>
  form.runtime === 'java' ? t('rt.entryJar') : form.runtime === 'go' || form.runtime === 'binary'
    ? t('rt.entryBinary')
    : t('rt.entry'),
)
const entryHint = computed(() =>
  ({ python: t('rt.entryHintPython'), node: t('rt.entryHintNode') } as Record<string, string>)[form.runtime] || '',
)

// --- Project dependencies ---
async function installDeps(row: AppRow) {
  try {
    await ElMessageBox.confirm(t('rt.confirmDeps', { name: row.name }), t('rt.installDeps'), { type: 'info' })
  } catch {
    return
  }
  busy.value = `deps-${row.id}`
  installLogTitle.value = `${row.name} — ${t('rt.installDeps')}`
  installLog.value = t('rt.installRunning')
  installStatus.value = 'running'
  try {
    const res: any = await installAppDeps(row.id, true)
    installLog.value = res.data?.log || res.message || ''
    installStatus.value = res.success && res.data?.success ? 'success' : 'error'
  } catch {
    installStatus.value = 'error'
  } finally {
    busy.value = null
  }
}

const entryPlaceholder = computed(
  () =>
    ({
      node: 'server.js',
      python: 'main.py',
      java: 'app.jar',
      go: './server',
      binary: './myapp',
      php: 'worker.php',
    })[form.runtime] || 'server.js',
)

const rules = computed<FormRules>(() => ({
  name: [{ required: true, message: t('rt.nameRequired'), trigger: 'blur' }],
  work_dir: [{ required: true, message: t('rt.workDirRequired'), trigger: 'blur' }],
  entry: [{ required: true, message: t('rt.entryRequired'), trigger: 'blur' }],
}))

function openCreate() {
  editing.value = null
  Object.assign(form, {
    name: '',
    runtime: 'node',
    work_dir: '',
    entry: '',
    args: '',
    env: '',
    port: 3000,
    autostart: true,
  })
  formVisible.value = true
}

function openEdit(row: AppRow) {
  editing.value = row
  Object.assign(form, {
    name: row.name,
    runtime: row.runtime,
    work_dir: row.work_dir,
    entry: row.entry,
    args: row.args,
    env: row.env,
    port: row.port,
    autostart: row.autostart,
  })
  formVisible.value = true
}

async function submit() {
  try {
    await formRef.value?.validate()
  } catch {
    return
  }
  saving.value = true
  try {
    const payload = { ...form }
    const res: any = editing.value
      ? await updateApp(editing.value.id, payload)
      : await createApp(payload)
    if (res.success) {
      ElMessage.success(t('rt.saved'))
      formVisible.value = false
      await loadAll()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

async function action(row: AppRow, verb: 'start' | 'stop' | 'restart') {
  busy.value = row.id
  try {
    const res: any = await appAction(row.id, verb)
    if (res.success) {
      ElMessage.success(t(`rt.${verb}ed`))
      await refreshApps()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = null
  }
}

async function remove(row: AppRow) {
  try {
    await ElMessageBox.confirm(t('rt.confirmDelete', { name: row.name }), t('delete'), {
      type: 'warning',
      confirmButtonText: t('delete'),
      cancelButtonText: t('cancel'),
    })
  } catch {
    return
  }
  try {
    const res: any = await deleteApp(row.id)
    if (res.success) {
      ElMessage.success(t('rt.deleted'))
      await loadAll()
    }
  } catch {
    // Interceptor reports it.
  }
}

// --- App logs ---
const logsVisible = ref(false)
const logsLoading = ref(false)
const logsText = ref('')
const logsFollow = ref(true)
const logsTarget = ref<AppRow | null>(null)
const logPre = ref<HTMLElement | null>(null)
let logTimer: number | null = null

const logsTitle = computed(() =>
  logsTarget.value ? `${logsTarget.value.name} — ${t('rt.logs')}` : t('rt.logs'),
)

function openLogs(row: AppRow) {
  logsTarget.value = row
  logsText.value = ''
  logsVisible.value = true
  void fetchLogs()
  startLogPolling()
}

function startLogPolling() {
  stopLogPolling()
  logTimer = window.setInterval(() => {
    if (logsFollow.value && logsVisible.value) void fetchLogs()
  }, 3000)
}

function stopLogPolling() {
  if (logTimer !== null) {
    window.clearInterval(logTimer)
    logTimer = null
  }
}

async function fetchLogs() {
  if (!logsTarget.value) return
  logsLoading.value = true
  try {
    const res: any = await getAppLogs(logsTarget.value.id, 500)
    if (res.success) {
      logsText.value = res.data || ''
      if (logsFollow.value) {
        await nextTick()
        if (logPre.value) logPre.value.scrollTop = logPre.value.scrollHeight
      }
    }
  } catch {
    // Interceptor reports it.
  } finally {
    logsLoading.value = false
  }
}

async function clearLogs() {
  if (!logsTarget.value) return
  try {
    const res: any = await clearAppLogs(logsTarget.value.id)
    if (res.success) {
      logsText.value = ''
      ElMessage.success(t('rt.logsCleared'))
    }
  } catch {
    // Interceptor reports it.
  }
}

watch(tab, (value) => {
  // Only poll while the app table is actually on screen.
  if (value === 'apps') startAppPolling()
  else stopAppPolling()
})

function startAppPolling() {
  stopAppPolling()
  pollTimer = window.setInterval(() => {
    if (pageActive.value && !document.hidden && !formVisible.value) void refreshApps()
  }, 5000)
}

function stopAppPolling() {
  if (pollTimer !== null) {
    window.clearInterval(pollTimer)
    pollTimer = null
  }
}

onMounted(loadAll)
const { active: pageActive } = usePage(loadAll)
onUnmounted(() => {
  stopAppPolling()
  stopLogPolling()
})
</script>

<style scoped lang="scss">
/* ---------- Runtime cards ---------- */
.rt-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: var(--lp-4);
}

.rt-card {
  display: flex;
  flex-direction: column;
  gap: var(--lp-3);
  padding: var(--lp-4);
  background: var(--lp-surface);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  box-shadow: var(--lp-shadow-sm);
  transition: border-color var(--lp-fast), box-shadow var(--lp-fast);

  &:hover { box-shadow: var(--lp-shadow); }
  &--on { border-color: color-mix(in srgb, var(--lp-ok) 35%, var(--lp-line)); }
}

.rt-card__head { display: flex; align-items: center; gap: var(--lp-3); }

.rt-logo {
  display: grid;
  place-items: center;
  width: 38px;
  height: 38px;
  flex: none;
  border-radius: var(--lp-r);
  background: var(--lp-surface-sunken);
  color: var(--lp-text-soft);
  font-family: var(--lp-font-mono);
  font-size: 13px;
  font-weight: 700;

  &[data-kind='php'] { background: rgba(119, 123, 180, 0.16); color: #6b70b8; }
  &[data-kind='node'] { background: rgba(87, 163, 61, 0.16); color: #5aa33d; }
  &[data-kind='python'] { background: rgba(55, 118, 171, 0.16); color: #3776ab; }
  &[data-kind='java'] { background: rgba(215, 99, 60, 0.16); color: #d7633c; }
  &[data-kind='go'] { background: rgba(0, 173, 216, 0.16); color: #00add8; }
}

.rt-title { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.rt-name { font-size: var(--lp-text-md); font-weight: 600; color: var(--lp-text); }

.rt-path {
  padding: 6px 8px;
  border-radius: var(--lp-r-sm);
  background: var(--lp-surface-sunken);
  color: var(--lp-text-dim);
  font-size: var(--lp-text-xs);
}

.rt-components { display: flex; flex-direction: column; gap: 5px; }
.rt-component {
  display: flex;
  align-items: center;
  gap: var(--lp-2);
  font-size: var(--lp-text-sm);
}
.rt-component__dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--lp-line-strong);
  flex: none;
  &.is-on { background: var(--lp-ok); }
}
.rt-component__name { color: var(--lp-text-soft); }
.rt-component__ver { margin-left: auto; color: var(--lp-text-dim); font-size: var(--lp-text-xs); }

.rt-card__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--lp-2);
  margin-top: auto;
  padding-top: var(--lp-3);
  border-top: 1px solid var(--lp-line-soft);
  min-height: 42px;
}
.rt-msg { font-size: var(--lp-text-xs); color: var(--lp-text-dim); min-width: 0; }

/* ---------- App table ---------- */
.cell-stack { display: flex; flex-direction: column; line-height: 1.35; min-width: 0; }
.cell-stack .lp-dim { font-size: 11px; }
.app-name { font-weight: 600; }
.entry { display: block; font-size: var(--lp-text-sm); color: var(--lp-text-soft); }
.pid { margin-left: 6px; font-size: var(--lp-text-xs); }
.restarts { font-size: 10px; margin-top: 2px; }

/* ---------- Form ---------- */
.form-row { display: flex; gap: var(--lp-4); }
.form-row__item { flex: 1; min-width: 0; }
.form-row__item--narrow { flex: 0 0 170px; }

.field-hint {
  display: block;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  line-height: 1.5;

  &--inline { display: inline; margin-left: var(--lp-3); }
}

/* ---------- Logs ---------- */
.log-view {
  max-height: 60vh;
  background: var(--lp-surface-sunken);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-sm);
}
.log-pre {
  margin: 0;
  padding: var(--lp-3);
  max-height: 60vh;
  overflow: auto;
  font-family: var(--lp-font-mono);
  font-size: var(--lp-text-sm);
  line-height: 1.55;
  color: var(--lp-text-soft);
  white-space: pre-wrap;
  word-break: break-word;
}

@media (max-width: 640px) {
  .form-row { flex-direction: column; gap: 0; }
  .form-row__item--narrow { flex: 1; }
}

.entry-hint {
  display: block;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  line-height: 1.5;
}
</style>
