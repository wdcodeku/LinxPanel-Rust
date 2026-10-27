<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.runtime') }}</h1>
        <p class="lp-page-sub">{{ subtitle }}</p>
      </div>
      <div class="lp-page-actions">
        <el-button size="small" :icon="Refresh" :loading="loading" @click="loadAll">{{ t('refresh') }}</el-button>
      </div>
    </div>

    <!-- One tab per language: its environment, then its applications. -->
    <div class="langs" role="tablist">
      <button
        v-for="lang in languages"
        :key="lang.key"
        type="button"
        role="tab"
        class="lang"
        :class="{ 'is-active': lang.key === active }"
        :aria-selected="lang.key === active"
        @click="selectLanguage(lang.key)"
      >
        <span class="rt-logo" :data-kind="lang.key">{{ logoText(lang.key) }}</span>
        <span class="lang__text">
          <span class="lang__name">{{ lang.label }}</span>
          <span class="lang__sub">
            <span class="dot" :class="lang.runtime?.installed ? 'dot--ok' : ''"></span>
            {{ lang.runtime?.installed ? (lang.runtime.version || t('rt.installed')) : loaded ? t('rt.notInstalled') : '…' }}
          </span>
        </span>
        <span v-if="lang.apps.length" class="lang__count" :class="{ 'is-running': lang.running > 0 }">
          {{ lang.running }}/{{ lang.apps.length }}
        </span>
      </button>
    </div>

    <template v-if="current">
      <!-- Environment -->
      <section v-if="!loaded" class="lp-card env">
        <el-skeleton animated :rows="2" />
      </section>
      <section v-else class="lp-card env" :class="{ 'env--off': !current.runtime?.installed }">
        <div class="env__main">
          <span class="rt-logo rt-logo--lg" :data-kind="current.key">{{ logoText(current.key) }}</span>
          <div class="env__text">
            <div class="env__title">
              {{ current.runtime?.display_name || current.label }}
              <span class="lp-pill" :class="current.runtime?.installed ? 'lp-pill--ok' : ''">
                <span class="lp-pill__dot"></span>{{ current.runtime?.installed ? current.runtime.version || t('rt.installed') : t('rt.notInstalled') }}
              </span>
            </div>
            <p class="env__desc">{{ t(`rt.langDesc.${current.key}`) }}</p>
            <div v-if="current.runtime?.installed && current.runtime.path" class="env__path lp-mono">{{ current.runtime.path }}</div>
            <p v-if="current.runtime?.message" class="env__msg">{{ current.runtime.message }}</p>
          </div>
          <div class="env__actions">
            <el-button
              v-if="!current.runtime?.installed"
              type="primary"
              :disabled="!current.runtime?.installable"
              :loading="installing === current.key"
              @click="current.runtime && install(current.runtime)"
            >{{ t('rt.installName', { name: current.label }) }}</el-button>
            <el-button
              v-else-if="current.key === 'php'"
              :loading="installing === current.key"
              @click="current.runtime && install(current.runtime, true)"
            >{{ t('rt.repair') }}</el-button>
          </div>
        </div>
        <ul v-if="current.runtime?.components.length" class="env__components">
          <li v-for="component in current.runtime.components" :key="component.name" class="comp" :class="{ 'is-on': component.installed }">
            <span class="comp__dot"></span>
            <span class="comp__name">{{ component.name }}</span>
            <span class="comp__ver lp-num">{{ component.installed ? component.version || t('rt.installed') : t('rt.notInstalled') }}</span>
          </li>
        </ul>
      </section>

      <!-- Applications of this language -->
      <section v-if="loaded" class="lp-card">
        <header class="lp-card-head">
          <h2 class="lp-card-title">
            {{ t('rt.appsOf', { name: current.label }) }}
            <span class="count">{{ current.apps.length }}</span>
          </h2>
          <el-button type="primary" size="small" :icon="Plus" @click="openCreate()">{{ t('rt.newAppOf', { name: current.label }) }}</el-button>
        </header>
        <div v-if="!current.apps.length" class="empty">
          <span class="rt-logo rt-logo--lg" :data-kind="current.key">{{ logoText(current.key) }}</span>
          <p class="empty__title">{{ t('rt.noAppsOf', { name: current.label }) }}</p>
          <p class="empty__hint">{{ t(`rt.appHint.${current.key}`) }}</p>
          <el-button type="primary" :icon="Plus" @click="openCreate()">{{ t('rt.newAppOf', { name: current.label }) }}</el-button>
        </div>
        <el-table v-else :data="current.apps" size="small">
          <el-table-column :label="t('rt.appName')" min-width="190">
            <template #default="{ row }">
              <div class="cell-stack">
                <span class="app-name">{{ row.name }}</span>
                <span class="dir-line">
                  <button type="button" class="dir-link lp-mono lp-truncate" :title="t('rt.openDir')" @click="openDir(row.work_dir)">{{ row.work_dir }}</button>
                  <button type="button" class="dir-icon" :title="t('rt.openDir')" :aria-label="t('rt.openDir')" @click="openDir(row.work_dir)"><el-icon><FolderOpened /></el-icon></button>
                  <button type="button" class="dir-icon" :title="t('rt.openTerminal')" :aria-label="t('rt.openTerminal')" @click="openTerminalAt(row.work_dir)"><el-icon><Monitor /></el-icon></button>
                </span>
              </div>
            </template>
          </el-table-column>
          <el-table-column :label="t('rt.entry')" min-width="170">
            <template #default="{ row }">
              <code class="lp-mono lp-truncate entry">{{ row.entry }} {{ row.args }}</code>
              <span v-if="current.key === 'go'" class="lp-dim kind-note">{{ runtimeLabel(row.runtime) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('rt.port')" width="80">
            <template #default="{ row }"><span class="lp-num">{{ row.port || '—' }}</span></template>
          </el-table-column>
          <el-table-column :label="t('rt.state')" width="150">
            <template #default="{ row }">
              <span class="lp-pill" :class="row.running ? 'lp-pill--ok' : row.last_error ? 'lp-pill--danger' : ''">
                <span class="lp-pill__dot"></span>{{ row.running ? t('rt.running') : t('rt.stopped') }}
              </span>
              <span v-if="row.running && row.pid" class="lp-dim pid">pid {{ row.pid }}</span>
              <div v-if="row.restarts" class="lp-dim restarts">{{ t('rt.restarts', { n: row.restarts }) }}</div>
            </template>
          </el-table-column>
          <el-table-column :label="t('rt.uptimeCol')" width="110">
            <template #default="{ row }"><span class="lp-num lp-dim">{{ row.running ? uptimeOf(row) : '—' }}</span></template>
          </el-table-column>
          <el-table-column :label="t('proc.actions')" width="270" align="right">
            <template #default="{ row }">
              <el-button v-if="!row.running" link type="primary" size="small" :loading="busy === row.id" @click="action(row, 'start')">{{ t('rt.start') }}</el-button>
              <template v-else>
                <el-button link size="small" :loading="busy === row.id" @click="action(row, 'restart')">{{ t('rt.restart') }}</el-button>
                <el-button link size="small" :loading="busy === row.id" @click="action(row, 'stop')">{{ t('rt.stop') }}</el-button>
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
      </section>
    </template>

    <!-- App editor -->
    <el-dialog v-model="formVisible" :title="editing ? t('rt.editApp') : t('rt.newApp')" width="620px" top="6vh">
      <el-form ref="formRef" :model="form" :rules="rules" label-position="top">
        <div class="form-row">
          <el-form-item :label="t('rt.appName')" prop="name" class="form-row__item">
            <el-input v-model="form.name" :placeholder="t('rt.appNamePlaceholder')" maxlength="64" />
          </el-form-item>
          <el-form-item :label="t('rt.runtime')" prop="runtime" class="form-row__item form-row__item--narrow">
            <el-select v-model="form.runtime" style="width: 100%">
              <el-option v-for="kind in formRuntimes" :key="kind.value" :label="kind.label" :value="kind.value" />
            </el-select>
          </el-form-item>
        </div>

        <el-form-item :label="t('rt.workDir')" prop="work_dir">
          <div class="inline-row">
            <el-input v-model="form.work_dir" placeholder="/opt/myapp" class="lp-mono" />
            <el-button :icon="FolderOpened" @click="dirPickerOpen = true">{{ t('rt.chooseDir') }}</el-button>
          </div>
        </el-form-item>

        <div class="form-row">
          <el-form-item :label="entryLabel" prop="entry" class="form-row__item">
            <el-autocomplete
              v-model="form.entry"
              :fetch-suggestions="suggestEntries"
              :placeholder="entryPlaceholder"
              :trigger-on-focus="true"
              class="lp-mono entry-input"
              style="width: 100%"
            >
              <template #default="{ item }">
                <span class="lp-mono">{{ item.value }}</span>
              </template>
            </el-autocomplete>
            <span v-if="entryHint" class="entry-hint">{{ entryHint }}</span>
          </el-form-item>
          <el-form-item :label="t('rt.portOptional')" class="form-row__item form-row__item--narrow">
            <el-input-number
              v-model="form.port"
              :min="1"
              :max="65535"
              :placeholder="t('rt.portNone')"
              controls-position="right"
              style="width: 100%"
            />
            <span class="entry-hint">{{ t('rt.portHint') }}</span>
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

    <el-dialog v-model="dirPickerOpen" :title="t('rt.chooseDir')" width="760px" destroy-on-close append-to-body>
      <div class="dir-picker-shell">
        <DirPicker :init-path="form.work_dir || '/www/wwwroot'" @select="onDirPicked" />
      </div>
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

  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { usePage } from '@/utils/page'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { FormRules } from 'element-plus'
import { FolderOpened, Monitor, Plus, Refresh } from '@element-plus/icons-vue'
import DirPicker from '@/components/DirPicker.vue'
import {
  appAction, clearAppLogs, createApp, deleteApp, getAppLogs, getApps, getRuntimes,
  installAppDeps, installRuntime, listFiles, mkdirApi, pathState, updateApp,
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

type LangKey = 'php' | 'node' | 'python' | 'java' | 'go'
const LANG_RUNTIMES: Record<LangKey, string[]> = {
  php: ['php'],
  node: ['node'],
  python: ['python'],
  java: ['java'],
  // Compiled Go programs and any other binary run the same way.
  go: ['go', 'binary'],
}
const LANG_KEY = 'lp.runtime.lang'
const route = useRoute()
const router = useRouter()
const loaded = ref(false)
const active = ref<LangKey>(initialLanguage())

function initialLanguage(): LangKey {
  const fromUrl = String(route.query.lang || '')
  if (fromUrl in LANG_RUNTIMES) return fromUrl as LangKey
  try {
    const saved = localStorage.getItem(LANG_KEY) || ''
    if (saved in LANG_RUNTIMES) return saved as LangKey
  } catch {
    // Storage blocked: start on PHP.
  }
  return 'php'
}

function selectLanguage(key: LangKey) {
  active.value = key
  try {
    localStorage.setItem(LANG_KEY, key)
  } catch {
    // Not remembered; harmless.
  }
  if (route.query.lang !== key) router.replace({ query: { ...route.query, lang: key } })
}
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

const languages = computed(() =>
  (Object.keys(LANG_RUNTIMES) as LangKey[]).map((key) => {
    const list = apps.value.filter((app) => LANG_RUNTIMES[key].includes(app.runtime))
    return {
      key,
      label: { php: 'PHP', node: 'Node.js', python: 'Python', java: 'Java', go: 'Go' }[key],
      runtime: runtimes.value.find((r) => r.kind === key),
      apps: list,
      running: list.filter((app) => app.running).length,
    }
  }),
)
const current = computed(() => languages.value.find((lang) => lang.key === active.value))
const formRuntimes = computed(() => {
  const own = LANG_RUNTIMES[active.value]
  // Editing keeps the app's own kind selectable.
  return appRuntimes.value.filter((kind) => own.includes(kind.value) || kind.value === form.runtime)
})

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
    // First visit without a saved choice: open a language that is set up.
    if (!loaded.value && !route.query.lang) {
      let saved = ''
      try {
        saved = localStorage.getItem(LANG_KEY) || ''
      } catch {
        // Ignore.
      }
      if (!saved) {
        const ready = (Object.keys(LANG_RUNTIMES) as LangKey[]).find((key) =>
          apps.value.some((app) => LANG_RUNTIMES[key].includes(app.runtime)),
        ) || runtimes.value.find((r) => r.installed && r.kind in LANG_RUNTIMES)?.kind
        if (ready) active.value = ready as LangKey
      }
    }
    loaded.value = true
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

// --- Runtime install (the task window shows the live log) ---
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
  try {
    await installRuntime(item.kind)
    await loadAll()
  } catch {
    // Interceptor reports it.
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
  // Optional: most programs (Go / binaries especially) choose their own.
  port: undefined as number | undefined,
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

// --- Project dependencies (the task window shows the live log) ---
async function installDeps(row: AppRow) {
  try {
    await ElMessageBox.confirm(t('rt.confirmDeps', { name: row.name }), t('rt.installDeps'), { type: 'info' })
  } catch {
    return
  }
  busy.value = `deps-${row.id}`
  try {
    await installAppDeps(row.id, true)
  } catch {
    // Interceptor reports it.
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

// --- Entry: suggest matching files from the project directory ---
const ENTRY_EXTENSIONS: Record<string, string[]> = {
  node: ['.js', '.mjs', '.cjs', '.ts'],
  python: ['.py'],
  java: ['.jar'],
  php: ['.php'],
}
function suggestEntries(query: string, done: (items: { value: string }[]) => void) {
  void entryCandidates(query).then(done)
}
async function entryCandidates(query: string): Promise<{ value: string }[]> {
  const dir = form.work_dir.trim()
  if (!dir.startsWith('/')) return []
  try {
    const state: any = await pathState(dir)
    if (!state.success || !state.data.is_dir) return []
    const res: any = await listFiles(dir)
    if (!res.success) return []
    const exts = ENTRY_EXTENSIONS[form.runtime]
    const q = query.trim().toLowerCase()
    return (res.data as { name: string; is_dir: boolean; permissions: string }[])
      .filter((f) => !f.is_dir)
      // Go / binaries: anything executable.
      .filter((f) => (exts ? exts.some((e) => f.name.toLowerCase().endsWith(e)) : /[1357]/.test(String(f.permissions))))
      .filter((f) => !q || f.name.toLowerCase().includes(q))
      .slice(0, 50)
      .map((f) => ({ value: exts ? f.name : `./${f.name}` }))
  } catch {
    return []
  }
}

// --- Directory: file manager / terminal / picker ---
function openDir(path: string) {
  router.push({ path: '/files', query: { path } })
}
function openTerminalAt(path: string) {
  router.push({ path: '/terminal', query: { cwd: path } })
}
const dirPickerOpen = ref(false)
function onDirPicked(path: string) {
  form.work_dir = path || form.work_dir
  dirPickerOpen.value = false
}

function openCreate() {
  editing.value = null
  const runtime = LANG_RUNTIMES[active.value][0]
  Object.assign(form, {
    name: '',
    runtime,
    work_dir: '',
    entry: '',
    args: '',
    env: '',
    port: undefined,
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
    port: row.port > 0 ? row.port : undefined,
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
  // A project directory that is not there yet: offer to create it.
  const dir = form.work_dir.trim()
  try {
    const state: any = await pathState(dir)
    if (state.success && !state.data.exists) {
      await ElMessageBox.confirm(t('rt.createDirConfirm', { path: dir }), t('rt.workDir'), {
        type: 'info',
        confirmButtonText: t('fm.createIt'),
      })
      const made: any = await mkdirApi(dir)
      if (!made.success) return
    } else if (state.success && !state.data.is_dir) {
      ElMessage.error(t('rt.workDirNotDir'))
      return
    }
  } catch {
    // Declined, or the interceptor reported it.
    return
  }
  saving.value = true
  try {
    const payload = { ...form, port: form.port || 0 }
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

onMounted(() => {
  void loadAll()
  startAppPolling()
})
const { active: pageActive } = usePage(loadAll)
onUnmounted(() => {
  stopAppPolling()
  stopLogPolling()
})
</script>

<style scoped lang="scss">
/* ---------- Language tabs ---------- */
.langs {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: var(--lp-3);
  margin-bottom: var(--lp-4);
}
.lang {
  position: relative;
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  min-width: 0;
  padding: var(--lp-3);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  background: var(--lp-surface);
  color: var(--lp-text);
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: border-color var(--lp-fast), box-shadow var(--lp-fast), background var(--lp-fast);

  &:hover { border-color: var(--lp-line-strong); box-shadow: var(--lp-shadow-sm); }
  &.is-active { border-color: var(--lp-accent); background: var(--lp-accent-soft); box-shadow: 0 0 0 1px var(--lp-accent) inset; }
}
.lang__text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.lang__name { font-weight: 600; }
.lang__sub {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: var(--lp-line-strong); }
.dot--ok { background: var(--lp-ok); }
.lang__count {
  margin-left: auto;
  padding: 1px 7px;
  border-radius: var(--lp-r-full);
  background: var(--lp-surface-sunken);
  font-size: var(--lp-text-xs);
  font-weight: 600;
  color: var(--lp-text-dim);
  font-variant-numeric: tabular-nums;
  &.is-running { background: var(--lp-ok-soft); color: var(--lp-ok); }
}

/* ---------- Environment ---------- */
.env { padding: var(--lp-5); margin-bottom: var(--lp-4); }
.env__main { display: flex; align-items: flex-start; gap: var(--lp-4); }
.env__text { flex: 1; min-width: 0; }
.env__title { display: flex; align-items: center; flex-wrap: wrap; gap: var(--lp-2); font-size: var(--lp-text-lg); font-weight: 650; }
.env__desc { margin: 4px 0 0; color: var(--lp-text-dim); line-height: 1.6; }
.env__path {
  display: inline-block;
  margin-top: var(--lp-2);
  padding: 4px 8px;
  border-radius: var(--lp-r-sm);
  background: var(--lp-surface-sunken);
  color: var(--lp-text-soft);
  font-size: var(--lp-text-xs);
}
.env__msg { margin: var(--lp-2) 0 0; font-size: var(--lp-text-sm); color: var(--lp-warn); }
.env__actions { flex: none; }
.env__components {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: var(--lp-2);
  margin: var(--lp-4) 0 0;
  padding: var(--lp-4) 0 0;
  border-top: 1px solid var(--lp-line-soft);
  list-style: none;
}
.comp {
  display: flex;
  align-items: center;
  gap: var(--lp-2);
  padding: 8px 10px;
  border-radius: var(--lp-r-sm);
  background: var(--lp-surface-sunken);
  font-size: var(--lp-text-sm);
}
.comp__dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: var(--lp-line-strong); }
.comp.is-on .comp__dot { background: var(--lp-ok); }
.comp__name { font-weight: 500; }
.comp__ver { margin-left: auto; color: var(--lp-text-dim); font-size: var(--lp-text-xs); }
.count {
  padding: 0 7px;
  border-radius: var(--lp-r-full);
  background: var(--lp-surface-sunken);
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
}
.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--lp-2);
  padding: var(--lp-8) var(--lp-4);
  text-align: center;
}
.empty__title { margin: var(--lp-2) 0 0; font-weight: 600; }
.empty__hint { max-width: 520px; margin: 0 0 var(--lp-3); color: var(--lp-text-dim); line-height: 1.6; }
.kind-note { display: block; font-size: var(--lp-text-xs); }
.rt-logo--lg { width: 46px; height: 46px; font-size: 15px; }

@media (max-width: 1100px) {
  .langs { grid-template-columns: repeat(3, minmax(0, 1fr)); }
}
@media (max-width: 640px) {
  .langs { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .env__main { flex-direction: column; }
}

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

/* ---------- App table ---------- */
.cell-stack { display: flex; flex-direction: column; line-height: 1.35; min-width: 0; }
.cell-stack .lp-dim { font-size: 11px; }
.dir-line { display: flex; align-items: center; gap: 2px; min-width: 0; }
.dir-link,
.dir-icon {
  padding: 0;
  border: 0;
  background: none;
  font: inherit;
  color: var(--lp-text-dim);
  cursor: pointer;
}
.dir-link { font-size: 11px; min-width: 0; text-align: left; }
.dir-link:hover { color: var(--lp-accent); text-decoration: underline; text-underline-offset: 2px; }
.dir-icon { display: inline-grid; place-items: center; width: 20px; height: 20px; border-radius: 4px; flex-shrink: 0; }
.dir-icon:hover { color: var(--lp-accent); background: var(--lp-accent-soft); }
.inline-row { display: flex; gap: var(--lp-2); width: 100%; }
.inline-row .el-input { flex: 1; min-width: 0; }
.dir-picker-shell {
  height: 520px;
  padding: 4px;
  border-radius: var(--lp-r-lg);
  background: var(--lp-bg-page);
}
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
