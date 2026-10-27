<template>
  <div class="setting-stack" v-loading="!status">
    <el-alert
      v-if="status && !status.can_update"
      type="info"
      :closable="false"
      show-icon
      :title="t('upd.notInstalled')"
    />
    <el-alert
      v-if="lastResult"
      :type="lastResult.success ? 'success' : 'error'"
      :title="lastResult.success ? t('upd.resultOk', { tag: lastResult.target }) : t('upd.resultFail', { tag: lastResult.target })"
      :description="lastResult.success ? '' : lastResult.message"
      show-icon
      @close="lastResultDismissed = true"
    />

    <!-- Version -->
    <section class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Upload /></el-icon>{{ t('upd.title') }}</h2>
        <el-button size="small" :icon="Refresh" :loading="checking" :disabled="busy" @click="check()">
          {{ t('upd.check') }}
        </el-button>
      </header>
      <div class="lp-card-body">
        <div class="versions">
          <div class="version">
            <span class="version__label">{{ t('upd.current') }}</span>
            <span class="version__value lp-mono">v{{ status?.current }}</span>
          </div>
          <el-icon class="version__arrow" v-if="hasUpdate"><Right /></el-icon>
          <div class="version" v-if="latest">
            <span class="version__label">{{ t('upd.latest') }}</span>
            <span class="version__value lp-mono">{{ latest.tag }}</span>
          </div>
          <span class="lp-pill" :class="hasUpdate ? 'lp-pill--accent' : 'lp-pill--ok'" v-if="status?.check">
            <span class="lp-pill__dot"></span>{{ hasUpdate ? t('upd.available') : t('upd.upToDate') }}
          </span>
          <div class="versions__actions">
            <el-button
              v-if="hasUpdate"
              type="primary"
              :disabled="!status?.can_update || busy"
              :loading="starting"
              @click="runUpdate"
            >{{ t('upd.updateNow', { tag: latest?.tag || '' }) }}</el-button>
          </div>
        </div>
        <p class="lp-dim meta" v-if="status?.check">
          {{ t('upd.checkedAt', { time: formatTime(status.check.checked_at) }) }}
          <span v-if="latest?.published_at"> · {{ t('upd.published', { time: new Date(latest.published_at).toLocaleDateString() }) }}</span>
          <a v-if="latest?.url" :href="latest.url" target="_blank" rel="noopener noreferrer"> · {{ t('upd.viewRelease') }}</a>
        </p>
        <p class="lp-dim meta" v-else-if="status">{{ t('upd.notChecked') }}</p>
        <p class="meta meta--warn" v-if="status?.check?.error">{{ t('upd.checkFailed') }}: {{ status.check.error }}</p>
        <pre v-if="hasUpdate && latest?.notes" class="notes">{{ latest.notes }}</pre>
      </div>
    </section>

    <!-- Progress -->
    <section class="lp-card" v-if="busy || log">
      <header class="lp-card-head">
        <h2 class="lp-card-title">
          <el-icon :class="{ spin: busy }"><Loading v-if="busy" /><Document v-else /></el-icon>
          {{ busy ? progressTitle : t('upd.lastLog') }}
        </h2>
      </header>
      <div class="lp-card-body">
        <p v-if="busy" class="lp-dim meta">{{ t('upd.progressHint') }}</p>
        <pre ref="logRef" class="log">{{ log || '…' }}</pre>
      </div>
    </section>

    <!-- Settings -->
    <section class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Setting /></el-icon>{{ t('upd.settings') }}</h2>
        <el-button type="primary" size="small" :loading="saving" @click="saveSettings">{{ t('save') }}</el-button>
      </header>
      <div class="lp-card-body">
        <el-form label-position="top" class="form">
          <el-form-item :label="t('upd.auto')">
            <div class="auto-row">
              <el-switch v-model="form.auto_update" />
              <el-time-select
                v-model="form.auto_time"
                start="00:00"
                step="00:30"
                end="23:30"
                :disabled="!form.auto_update"
                :clearable="false"
                style="width: 130px"
              />
            </div>
            <span class="hint">{{ t('upd.autoHint') }}</span>
          </el-form-item>
          <el-form-item :label="t('upd.source')">
            <el-radio-group v-model="form.source">
              <el-radio-button value="github">{{ t('upd.srcGithub') }}</el-radio-button>
              <el-radio-button value="proxy">{{ t('upd.srcProxy') }}</el-radio-button>
              <el-radio-button value="mirror">{{ t('upd.srcMirror') }}</el-radio-button>
            </el-radio-group>
          </el-form-item>
          <el-form-item v-if="form.source === 'proxy'" :label="t('upd.proxy')">
            <el-input v-model="form.proxy" placeholder="https://ghfast.top/" class="lp-mono" />
            <span class="hint">{{ t('upd.proxyHint') }}</span>
          </el-form-item>
          <el-form-item v-if="form.source === 'mirror'" :label="t('upd.mirror')">
            <el-input v-model="form.mirror" placeholder="https://dl.example.com/linxpanel" class="lp-mono" />
            <span class="hint">{{ t('upd.mirrorHint') }}</span>
          </el-form-item>
        </el-form>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { usePage } from '@/utils/page'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Document, Loading, Refresh, Right, Setting, Upload } from '@element-plus/icons-vue'
import { checkUpdate, getUpdateStatus, runPanelUpdate, saveUpdateSettings } from '@/api'
import type { UpdateConfig, UpdateStatus } from '@/api'
import { useI18n } from '@/i18n'

const { t } = useI18n()

const status = ref<UpdateStatus | null>(null)
const form = reactive<UpdateConfig>({ auto_update: false, auto_time: '04:00', source: 'github', proxy: '', mirror: '' })
const checking = ref(false)
const saving = ref(false)
const starting = ref(false)
const lastResultDismissed = ref(false)
const logRef = ref<HTMLElement>()

// While updating, the panel restarts: requests fail for a while, then the new
// version answers. These track that without error toasts.
const updating = ref(false)
const restarting = ref(false)
const startedFrom = ref('')

const latest = computed(() => status.value?.check?.latest || null)
const hasUpdate = computed(() => !!status.value?.check?.has_update)
const busy = computed(() => updating.value || !!status.value?.running)
const log = computed(() => status.value?.log || '')
const lastResult = computed(() => {
  const result = status.value?.last_result
  if (!result || lastResultDismissed.value || busy.value) return null
  return Date.now() / 1000 - result.finished_at < 86400 ? result : null
})
const progressTitle = computed(() => (restarting.value ? t('upd.restarting') : t('upd.running')))

function applyStatus(next: UpdateStatus) {
  status.value = next
  Object.assign(form, next.config)
}

async function load() {
  try {
    const res: any = await getUpdateStatus()
    if (res.success) applyStatus(res.data)
  } catch {
    // Interceptor reports it.
  }
}

async function check(quiet = false) {
  checking.value = true
  try {
    const res: any = await checkUpdate()
    if (res.success) {
      applyStatus(res.data)
      if (quiet) return
      ElMessage.success(res.data.check?.has_update ? t('upd.foundNew', { tag: res.data.check.latest.tag }) : t('upd.upToDate'))
    }
  } catch {
    // Interceptor reports it.
  } finally {
    checking.value = false
  }
}

async function saveSettings() {
  saving.value = true
  try {
    const res: any = await saveUpdateSettings({ ...form })
    if (res.success) ElMessage.success(t('upd.saved'))
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

async function runUpdate() {
  const tag = latest.value?.tag
  if (!tag) return
  try {
    await ElMessageBox.confirm(t('upd.confirm', { tag }), t('upd.confirmTitle'), {
      type: 'warning',
      confirmButtonText: t('upd.updateNow', { tag }),
    })
  } catch {
    return
  }
  starting.value = true
  try {
    const res: any = await runPanelUpdate(tag)
    if (res.success) {
      startedFrom.value = status.value?.current || ''
      updating.value = true
      lastResultDismissed.value = false
      poll()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    starting.value = false
  }
}

// Plain fetch: failures are expected while the panel restarts.
async function quietStatus(): Promise<UpdateStatus | null> {
  try {
    const token = localStorage.getItem('token')
    const response = await fetch('/api/update', {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
      cache: 'no-store',
    })
    if (!response.ok) return null
    const body = await response.json()
    return body?.success ? (body.data as UpdateStatus) : null
  } catch {
    return null
  }
}

let timer: ReturnType<typeof setTimeout> | undefined
let deadline = 0
async function poll() {
  deadline = deadline || Date.now() + 10 * 60 * 1000
  const next = await quietStatus()
  if (!next) {
    restarting.value = true
  } else {
    restarting.value = false
    applyStatus(next)
    await nextTick()
    logRef.value?.scrollTo({ top: logRef.value.scrollHeight })
    // Done when no run is in progress any more.
    if (!next.running && updating.value) {
      updating.value = false
      deadline = 0
      if (next.current !== startedFrom.value) {
        ElMessage.success(t('upd.done', { tag: `v${next.current}` }))
        // New assets: reload into the new version.
        setTimeout(() => window.location.reload(), 1500)
      }
      return
    }
  }
  if (Date.now() > deadline) {
    updating.value = false
    deadline = 0
    ElMessage.warning(t('upd.timeout'))
    return
  }
  timer = setTimeout(poll, 2000)
}

function formatTime(unix: number) {
  return unix ? new Date(unix * 1000).toLocaleString() : '-'
}

onMounted(async () => {
  await load()
  // After a restart nothing is cached yet: look it up now rather than show "not checked".
  if (status.value && !status.value.check && !status.value.running) check(true)
  // An update started elsewhere (or automatically) is followed too.
  if (status.value?.running) {
    startedFrom.value = status.value.current
    updating.value = true
    poll()
  }
})
onBeforeUnmount(() => clearTimeout(timer))
usePage(() => {
  if (!updating.value) void load()
})
</script>

<style scoped lang="scss">
.versions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--lp-4);
}
.version { display: flex; flex-direction: column; gap: 2px; }
.version__label { font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
.version__value { font-size: var(--lp-text-lg); font-weight: 600; }
.version__arrow { color: var(--lp-text-faint); }
.versions__actions { margin-left: auto; }

.meta { margin: var(--lp-3) 0 0; font-size: var(--lp-text-xs); }
.meta a { color: var(--lp-accent); text-decoration: none; }
.meta--warn { color: var(--lp-warn); }

.notes,
.log {
  margin: var(--lp-3) 0 0;
  padding: var(--lp-3) var(--lp-4);
  background: var(--lp-surface-sunken, var(--lp-bg));
  border: 1px solid var(--lp-line-soft);
  border-radius: var(--lp-r-sm);
  font-family: var(--lp-font-mono);
  font-size: var(--lp-text-xs);
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 280px;
  overflow: auto;
}
.log { max-height: 360px; }

.auto-row { display: flex; align-items: center; gap: var(--lp-3); }
.hint { display: block; width: 100%; margin-top: var(--lp-1); font-size: var(--lp-text-xs); color: var(--lp-text-dim); line-height: 1.5; }
.form { max-width: 640px; }

.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
</style>
