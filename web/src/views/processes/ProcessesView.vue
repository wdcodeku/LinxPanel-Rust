<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.processes') }}</h1>
        <p class="lp-page-sub">{{ t('proc.subtitle', { total }) }}</p>
      </div>
      <div class="lp-page-actions">
        <el-input
          v-model="keyword"
          :placeholder="t('proc.searchPlaceholder')"
          :prefix-icon="Search"
          clearable
          size="small"
          class="search-input"
          @input="onSearchInput"
        />
        <el-select v-model="autoRefresh" size="small" style="width: 120px" @change="restartTimer">
          <el-option :label="t('proc.paused')" :value="0" />
          <el-option label="2s" :value="2000" />
          <el-option label="5s" :value="5000" />
          <el-option label="10s" :value="10000" />
        </el-select>
        <el-button size="small" :icon="Refresh" :loading="loading" @click="fetchRows">
          {{ t('refresh') }}
        </el-button>
      </div>
    </div>

    <div class="lp-card">
      <el-table
        :data="rows"
        v-loading="loading && !rows.length"
        size="small"
        :empty-text="t('proc.empty')"
        :default-sort="{ prop: sortBy, order: order === 'desc' ? 'descending' : 'ascending' }"
        @sort-change="onSortChange"
      >
        <el-table-column prop="pid" label="PID" width="80" sortable="custom" />
        <el-table-column :label="t('proc.name')" min-width="200" sortable="custom" prop="name">
          <template #default="{ row }">
            <div class="cell-stack">
              <span class="proc-name lp-truncate">{{ row.name }}</span>
              <span v-if="row.cmd" class="lp-dim lp-truncate lp-mono">{{ row.cmd }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('proc.user')" prop="user" width="110" />
        <el-table-column :label="t('proc.cpu')" width="120" sortable="custom" prop="cpu_percent">
          <template #default="{ row }">
            <div class="usage">
              <div class="usage__track">
                <div class="usage__fill" :style="{ width: Math.min(100, row.cpu_percent) + '%', background: levelVar(row.cpu_percent) }"></div>
              </div>
              <span class="lp-num usage__pct">{{ row.cpu_percent.toFixed(1) }}%</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('proc.memory')" width="110" sortable="custom" prop="memory" align="right">
          <template #default="{ row }"><span class="lp-num">{{ formatBytes(row.memory) }}</span></template>
        </el-table-column>
        <el-table-column :label="t('proc.status')" width="100">
          <template #default="{ row }">
            <span class="lp-pill" :class="statusClass(row.status)">{{ row.status }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('proc.runtime')" width="110" sortable="custom" prop="run_time" align="right">
          <template #default="{ row }"><span class="lp-num lp-dim">{{ formatDuration(row.run_time) }}</span></template>
        </el-table-column>
        <el-table-column :label="t('proc.actions')" width="140" align="right">
          <template #default="{ row }">
            <el-button link size="small" @click="showDetail(row)">{{ t('proc.detail') }}</el-button>
            <el-button link type="danger" size="small" @click="confirmKill(row)">{{ t('proc.kill') }}</el-button>
          </template>
        </el-table-column>
      </el-table>

      <div class="table-foot">
        <el-pagination
          v-model:current-page="page"
          :page-size="pageSize"
          :total="total"
          layout="total, prev, pager, next"
          size="small"
          background
          @current-change="fetchRows"
        />
      </div>
    </div>

    <el-dialog v-model="detailVisible" :title="t('proc.detail')" width="560px">
      <dl v-if="selected" class="detail-list">
        <div><dt>PID</dt><dd class="lp-mono">{{ selected.pid }}</dd></div>
        <div><dt>{{ t('proc.parentPid') }}</dt><dd class="lp-mono">{{ selected.parent_pid ?? '-' }}</dd></div>
        <div><dt>{{ t('proc.name') }}</dt><dd>{{ selected.name }}</dd></div>
        <div><dt>{{ t('proc.user') }}</dt><dd>{{ selected.user || '-' }}</dd></div>
        <div><dt>{{ t('proc.status') }}</dt><dd>{{ selected.status }}</dd></div>
        <div><dt>{{ t('proc.cpu') }}</dt><dd>{{ selected.cpu_percent.toFixed(1) }}%</dd></div>
        <div><dt>{{ t('proc.memory') }}</dt><dd>{{ formatBytes(selected.memory) }}</dd></div>
        <div><dt>{{ t('proc.virtualMemory') }}</dt><dd>{{ formatBytes(selected.virtual_memory) }}</dd></div>
        <div><dt>{{ t('proc.started') }}</dt><dd>{{ formatDateTime(selected.start_time) }}</dd></div>
        <div class="detail-list__wide"><dt>{{ t('proc.executable') }}</dt><dd class="lp-mono detail-break">{{ selected.exe || '-' }}</dd></div>
        <div class="detail-list__wide"><dt>{{ t('proc.command') }}</dt><dd class="lp-mono detail-break">{{ selected.cmd || '-' }}</dd></div>
      </dl>
    </el-dialog>

    <el-dialog v-model="killVisible" :title="t('proc.killTitle')" width="420px">
      <p class="kill-text">{{ t('proc.killConfirm', { name: killTarget?.name, pid: killTarget?.pid }) }}</p>
      <el-form label-position="top">
        <el-form-item :label="t('proc.signal')">
          <el-select v-model="killSignal" style="width: 100%">
            <el-option label="SIGTERM — graceful" value="TERM" />
            <el-option label="SIGINT — interrupt" value="INT" />
            <el-option label="SIGHUP — reload" value="HUP" />
            <el-option label="SIGKILL — force" value="KILL" />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="killVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="danger" :loading="killing" @click="doKill">{{ t('proc.send') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { Refresh, Search } from '@element-plus/icons-vue'
import { getProcesses, signalProcess } from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes, formatDateTime, formatDuration } from '@/utils/format'

type ProcessRow = {
  pid: number
  parent_pid: number | null
  name: string
  cmd: string
  exe: string
  user: string
  cpu_percent: number
  memory: number
  virtual_memory: number
  status: string
  run_time: number
  start_time: number
}

const { t } = useI18n()

const rows = ref<ProcessRow[]>([])
const total = ref(0)
const loading = ref(false)
const keyword = ref('')
const sortBy = ref('cpu_percent')
const order = ref<'asc' | 'desc'>('desc')
const page = ref(1)
const pageSize = 50
const autoRefresh = ref(5000)

let timer: number | null = null
let searchTimer: number | null = null

/** Table prop names differ from the API's sort keys. */
const SORT_KEYS: Record<string, string> = {
  cpu_percent: 'cpu',
  memory: 'memory',
  pid: 'pid',
  name: 'name',
  run_time: 'run_time',
}

async function fetchRows() {
  if (loading.value) return
  loading.value = true
  try {
    const res: any = await getProcesses({
      keyword: keyword.value,
      sort_by: SORT_KEYS[sortBy.value] || 'cpu',
      order: order.value,
      offset: (page.value - 1) * pageSize,
      limit: pageSize,
    })
    if (res.success) {
      rows.value = res.data?.rows || []
      total.value = res.data?.total || 0
    }
  } catch {
    // The interceptor already surfaced the error; keep the last good rows on screen.
  } finally {
    loading.value = false
  }
}

function onSearchInput() {
  if (searchTimer !== null) window.clearTimeout(searchTimer)
  searchTimer = window.setTimeout(() => {
    page.value = 1
    void fetchRows()
  }, 300)
}

function onSortChange({ prop, order: direction }: { prop: string; order: string | null }) {
  if (!prop || !direction) return
  sortBy.value = prop
  order.value = direction === 'ascending' ? 'asc' : 'desc'
  page.value = 1
  void fetchRows()
}

function restartTimer() {
  if (timer !== null) {
    window.clearInterval(timer)
    timer = null
  }
  if (autoRefresh.value > 0) {
    timer = window.setInterval(() => {
      // Skip refreshes while a dialog is open so the row under the cursor stays put.
      if (!detailVisible.value && !killVisible.value && !document.hidden) void fetchRows()
    }, autoRefresh.value)
  }
}

// --- Detail ---
const detailVisible = ref(false)
const selected = ref<ProcessRow | null>(null)
function showDetail(row: ProcessRow) {
  selected.value = row
  detailVisible.value = true
}

// --- Kill ---
const killVisible = ref(false)
const killTarget = ref<ProcessRow | null>(null)
const killSignal = ref('TERM')
const killing = ref(false)

function confirmKill(row: ProcessRow) {
  killTarget.value = row
  killSignal.value = 'TERM'
  killVisible.value = true
}

async function doKill() {
  if (!killTarget.value) return
  killing.value = true
  try {
    const res: any = await signalProcess(killTarget.value.pid, killSignal.value)
    if (res.success) {
      ElMessage.success(t('proc.signalSent'))
      killVisible.value = false
      await fetchRows()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    killing.value = false
  }
}

function statusClass(status: string) {
  const value = (status || '').toLowerCase()
  if (value.includes('run')) return 'lp-pill--ok'
  if (value.includes('zombie') || value.includes('dead')) return 'lp-pill--danger'
  if (value.includes('stop')) return 'lp-pill--warn'
  return ''
}

function levelVar(percent: number) {
  if (percent >= 70) return 'var(--lp-danger)'
  if (percent >= 35) return 'var(--lp-warn)'
  return 'var(--lp-accent)'
}

onMounted(() => {
  void fetchRows()
  restartTimer()
})

onUnmounted(() => {
  if (timer !== null) window.clearInterval(timer)
  if (searchTimer !== null) window.clearTimeout(searchTimer)
})
</script>

<style scoped lang="scss">
.search-input { width: 230px; }

.cell-stack { display: flex; flex-direction: column; line-height: 1.35; min-width: 0; }
.cell-stack .lp-dim { font-size: 11px; }
.proc-name { font-weight: 500; }

.usage { display: flex; align-items: center; gap: var(--lp-2); }
.usage__track {
  flex: 1;
  height: 4px;
  min-width: 34px;
  border-radius: var(--lp-r-full);
  background: var(--lp-surface-sunken);
  overflow: hidden;
}
.usage__fill { height: 100%; border-radius: inherit; transition: width 400ms ease; }
.usage__pct { font-size: var(--lp-text-xs); color: var(--lp-text-soft); min-width: 40px; text-align: right; }

.table-foot {
  display: flex;
  justify-content: flex-end;
  padding: var(--lp-3) var(--lp-4);
  border-top: 1px solid var(--lp-line-soft);
}

.detail-list {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--lp-3) var(--lp-5);
  margin: 0;

  dt { font-size: var(--lp-text-xs); color: var(--lp-text-dim); margin-bottom: 2px; }
  dd { margin: 0; font-size: var(--lp-text-base); color: var(--lp-text); }
  &__wide { grid-column: 1 / -1; }
}
.detail-break { word-break: break-all; line-height: 1.5; }

.kill-text { margin-bottom: var(--lp-4); color: var(--lp-text-soft); }

@media (max-width: 768px) {
  .search-input { width: 100%; }
}
</style>
