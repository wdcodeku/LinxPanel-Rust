<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.services') }}</h1>
        <p class="lp-page-sub">{{ subtitle }}</p>
      </div>
      <div class="lp-page-actions">
        <el-input
          v-model="keyword"
          :placeholder="t('svc.searchPlaceholder')"
          :prefix-icon="Search"
          clearable
          size="small"
          class="search-input"
        />
        <el-select v-model="stateFilter" size="small" style="width: 130px">
          <el-option :label="t('svc.allStates')" value="all" />
          <el-option :label="t('svc.running')" value="active" />
          <el-option :label="t('svc.stopped')" value="inactive" />
          <el-option :label="t('svc.failed')" value="failed" />
        </el-select>
        <el-button size="small" :icon="Refresh" :loading="loading" @click="load">{{ t('refresh') }}</el-button>
      </div>
    </div>

    <el-alert
      v-if="backend && !backend.manageable && backend.message"
      :title="backend.message"
      type="info"
      :closable="false"
      show-icon
      class="notice"
    />

    <div class="lp-card">
      <el-table :data="filtered" v-loading="loading" size="small" :empty-text="emptyText">
        <el-table-column :label="t('svc.name')" min-width="240">
          <template #default="{ row }">
            <div class="cell-stack">
              <span class="svc-name lp-truncate">{{ row.name }}</span>
              <span v-if="row.description" class="lp-dim lp-truncate">{{ row.description }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('svc.state')" width="130">
          <template #default="{ row }">
            <span class="lp-pill" :class="stateClass(row.active_state)">
              <span class="lp-pill__dot"></span>{{ row.active_state }}
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('svc.subState')" prop="sub_state" width="110" />
        <el-table-column :label="t('svc.startup')" width="110">
          <template #default="{ row }">
            <span class="lp-dim">{{ row.enabled }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('proc.actions')" width="250" align="right">
          <template #default="{ row }">
            <el-button link size="small" @click="openLogs(row)">{{ t('svc.logs') }}</el-button>
            <template v-if="row.can_manage">
              <el-button
                v-if="row.active_state === 'active'"
                link
                size="small"
                :loading="busy === row.name"
                @click="run(row, 'stop')"
              >{{ t('svc.stop') }}</el-button>
              <el-button
                v-else
                link
                type="primary"
                size="small"
                :loading="busy === row.name"
                @click="run(row, 'start')"
              >{{ t('svc.start') }}</el-button>
              <el-button link size="small" :loading="busy === row.name" @click="run(row, 'restart')">
                {{ t('svc.restart') }}
              </el-button>
              <el-button
                link
                size="small"
                :loading="busy === row.name"
                @click="run(row, row.enabled === 'enabled' ? 'disable' : 'enable')"
              >{{ row.enabled === 'enabled' ? t('svc.disable') : t('svc.enable') }}</el-button>
            </template>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <el-dialog v-model="logsVisible" :title="logsTitle" width="760px" top="6vh">
      <div class="log-view" v-loading="logsLoading">
        <pre class="log-pre">{{ logsText || t('svc.noLogs') }}</pre>
      </div>
      <template #footer>
        <el-button size="small" :icon="Refresh" :loading="logsLoading" @click="fetchLogs">{{ t('refresh') }}</el-button>
        <el-button size="small" @click="logsVisible = false">{{ t('close') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Refresh, Search } from '@element-plus/icons-vue'
import {
  getServiceBackend, getServiceLogs, getServiceStatus, getServices, serviceAction,
} from '@/api'
import { useI18n } from '@/i18n'

type ServiceRow = {
  name: string
  description: string
  load_state: string
  active_state: string
  sub_state: string
  enabled: string
  can_manage: boolean
}

type Backend = { kind: string; available: boolean; manageable: boolean; message: string }

const { t } = useI18n()

const rows = ref<ServiceRow[]>([])
const backend = ref<Backend | null>(null)
const loading = ref(false)
const keyword = ref('')
const stateFilter = ref('all')
const busy = ref('')
const loadError = ref('')

const subtitle = computed(() => {
  if (!backend.value) return ''
  const kind = backend.value.kind === 'none' ? t('svc.noBackend') : backend.value.kind
  return t('svc.subtitle', { backend: kind, total: rows.value.length })
})

const emptyText = computed(() => loadError.value || t('svc.empty'))

const filtered = computed(() => {
  const search = keyword.value.trim().toLowerCase()
  return rows.value.filter((row) => {
    if (stateFilter.value !== 'all' && row.active_state !== stateFilter.value) return false
    if (!search) return true
    return row.name.toLowerCase().includes(search) || row.description.toLowerCase().includes(search)
  })
})

async function load() {
  loading.value = true
  loadError.value = ''
  try {
    const [backendRes, listRes]: any[] = await Promise.all([getServiceBackend(), getServices()])
    if (backendRes.success) backend.value = backendRes.data
    if (listRes.success) {
      rows.value = listRes.data || []
    } else {
      rows.value = []
      loadError.value = listRes.message || ''
    }
  } catch {
    rows.value = []
  } finally {
    loading.value = false
  }
}

async function run(row: ServiceRow, action: string) {
  const destructive = action === 'stop' || action === 'restart' || action === 'disable'
  if (destructive) {
    try {
      await ElMessageBox.confirm(
        t('svc.confirmAction', { action: t(`svc.${action}`), name: row.name }),
        t('svc.confirmTitle'),
        { type: 'warning', confirmButtonText: t('confirm'), cancelButtonText: t('cancel') },
      )
    } catch {
      return
    }
  }

  busy.value = row.name
  try {
    const res: any = await serviceAction({ name: row.name, action })
    if (res.success) {
      ElMessage.success(t('svc.actionDone'))
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Logs ---
const logsVisible = ref(false)
const logsLoading = ref(false)
const logsText = ref('')
const logsTarget = ref<ServiceRow | null>(null)
const logsTitle = computed(() => (logsTarget.value ? `${logsTarget.value.name} — ${t('svc.logs')}` : t('svc.logs')))

function openLogs(row: ServiceRow) {
  logsTarget.value = row
  logsText.value = ''
  logsVisible.value = true
  void fetchLogs()
}

async function fetchLogs() {
  if (!logsTarget.value) return
  logsLoading.value = true
  try {
    // journalctl is the richer source; fall back to unit status where it is absent.
    const res: any = await getServiceLogs(logsTarget.value.name, 300)
    if (res.success && res.data?.trim()) {
      logsText.value = res.data
    } else {
      const statusRes: any = await getServiceStatus(logsTarget.value.name)
      logsText.value = statusRes.success ? statusRes.data : res.message || ''
    }
  } catch {
    logsText.value = ''
  } finally {
    logsLoading.value = false
  }
}

function stateClass(state: string) {
  if (state === 'active') return 'lp-pill--ok'
  if (state === 'failed') return 'lp-pill--danger'
  if (state === 'activating' || state === 'deactivating') return 'lp-pill--warn'
  return ''
}

onMounted(load)
</script>

<style scoped lang="scss">
.search-input { width: 230px; }
.notice { margin-bottom: var(--lp-4); border-radius: var(--lp-r); }

.cell-stack { display: flex; flex-direction: column; line-height: 1.35; min-width: 0; }
.cell-stack .lp-dim { font-size: 11px; }
.svc-name { font-weight: 500; font-family: var(--lp-font-mono); font-size: var(--lp-text-sm); }

.log-view {
  max-height: 62vh;
  overflow: auto;
  background: var(--lp-surface-sunken);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-sm);
}
.log-pre {
  margin: 0;
  padding: var(--lp-3);
  font-family: var(--lp-font-mono);
  font-size: var(--lp-text-sm);
  line-height: 1.55;
  color: var(--lp-text-soft);
  white-space: pre-wrap;
  word-break: break-word;
}

@media (max-width: 768px) {
  .search-input { width: 100%; }
}
</style>
