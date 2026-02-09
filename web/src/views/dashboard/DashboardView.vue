<template>
  <div class="dashboard">
    <div class="platform-bar">
      <div class="platform-item">
        <el-icon :size="16"><Platform /></el-icon>
        <span class="label">{{ t('db.system') }}</span>
        <span class="value">{{ info.platform?.os_version || '-' }}</span>
      </div>
      <div class="platform-item">
        <el-icon :size="16"><Cpu /></el-icon>
        <span class="label">{{ t('db.processor') }}</span>
        <span class="value">{{ info.cpu?.model_name || '-' }}</span>
      </div>
      <div class="platform-item">
        <el-icon :size="16"><Connection /></el-icon>
        <span class="label">{{ t('db.platform') }}</span>
        <span class="value">{{ info.platform?.platform || '-' }}</span>
      </div>
      <div class="platform-item">
        <el-icon :size="16"><Histogram /></el-icon>
        <span class="label">{{ t('db.load') }}</span>
        <span class="value">{{ loadDisplay }}</span>
      </div>
    </div>

    <el-row :gutter="16" class="gauge-row">
      <el-col :xs="12" :sm="6" v-for="g in gauges" :key="g.key">
        <el-tooltip placement="bottom" :show-after="120" :enterable="true">
          <template #content>
            <div class="gauge-tip-content">
              <div class="gauge-tip-title">{{ g.title }}</div>
              <div v-for="(line, idx) in g.details" :key="`${g.key}-${idx}`" class="gauge-tip-line">{{ line }}</div>
              <div v-if="g.key === 'load'" class="gauge-tip-actions">
                <el-button link class="gauge-tip-action" @click.stop="openProcessDialog">
                  {{ t('db.viewProcesses') }}
                </el-button>
              </div>
            </div>
          </template>
          <div class="gauge-card">
            <div class="gauge-ring">
              <svg viewBox="0 0 120 120">
                <circle cx="60" cy="60" r="52" fill="none" :stroke="'var(--lp-border-light)'" stroke-width="8" />
                <circle
                  cx="60"
                  cy="60"
                  r="52"
                  fill="none"
                  :stroke="gaugeColor(g.percent)"
                  stroke-width="8"
                  stroke-linecap="round"
                  :stroke-dasharray="circumference"
                  :stroke-dashoffset="circumference - (g.percent / 100) * circumference"
                  transform="rotate(-90 60 60)"
                  style="transition: stroke-dashoffset 0.45s ease"
                />
              </svg>
              <div class="gauge-center">
                <span class="gauge-pct">{{ g.percent.toFixed(1) }}%</span>
              </div>
            </div>
            <div class="gauge-meta">
              <div class="gauge-title">{{ g.title }}</div>
              <div class="gauge-sub">{{ g.sub }}</div>
            </div>
          </div>
        </el-tooltip>
      </el-col>
    </el-row>

    <el-row :gutter="16" style="margin-top:16px">
      <el-col :xs="24" :lg="12">
        <el-card shadow="never" class="detail-card">
          <template #header>
            <div class="card-header"><el-icon><Cpu /></el-icon> {{ t('db.cpuDetail') }}</div>
          </template>
          <el-descriptions :column="2" border size="small">
            <el-descriptions-item :label="t('db.model')">{{ info.cpu?.model_name }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.vendor')">{{ info.cpu?.vendor }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.physCores')">{{ info.cpu?.physical_cores }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.logicCores')">{{ info.cpu?.logical_cores }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.frequency')">{{ cpuFrequencyDisplay }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.used')">{{ (info.cpu?.usage_percent || 0).toFixed(1) }}%</el-descriptions-item>
          </el-descriptions>
          <div class="core-grid" v-if="info.cpu?.per_core_usage?.length">
            <div class="core-item" v-for="(usage, idx) in info.cpu.per_core_usage" :key="idx">
              <div class="core-bar-bg">
                <div class="core-bar" :style="{ height: usage + '%', background: gaugeColor(usage) }"></div>
              </div>
              <span class="core-label">{{ idx }}</span>
            </div>
          </div>
        </el-card>
      </el-col>

      <el-col :xs="24" :lg="12">
        <el-card shadow="never" class="detail-card">
          <template #header>
            <div class="card-header"><el-icon><Coin /></el-icon> {{ t('db.memDetail') }}</div>
          </template>
          <el-descriptions :column="2" border size="small">
            <el-descriptions-item :label="t('db.total')">{{ fmtB(info.memory?.total) }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.used')">{{ fmtB(info.memory?.used) }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.available')">{{ fmtB(info.memory?.available) }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.free')">{{ fmtB(info.memory?.free) }}</el-descriptions-item>
          </el-descriptions>
          <div class="kv-summary">
            <span>{{ t('db.used') }} {{ memoryPercent.toFixed(1) }}%</span>
            <span>{{ t('db.available') }} {{ memoryAvailablePercent.toFixed(1) }}%</span>
            <span>{{ t('db.free') }} {{ memoryFreePercent.toFixed(1) }}%</span>
          </div>
          <div class="mem-bar-wrap">
            <div class="mem-bar-label">
              <span>{{ t('db.memory') }}</span>
              <span>{{ memoryPercent.toFixed(1) }}%</span>
            </div>
            <div class="mem-bar-track">
              <div class="mem-bar-fill" :style="{ width: memoryPercent + '%', background: gaugeColor(memoryPercent) }"></div>
            </div>
          </div>
        </el-card>
      </el-col>
    </el-row>

    <el-row :gutter="16" style="margin-top:16px">
      <el-col :xs="24" :lg="14">
        <el-card shadow="never" class="detail-card">
          <template #header>
            <div class="card-header"><el-icon><Files /></el-icon> {{ t('db.diskDetail') }}</div>
          </template>
          <div class="kv-summary disk-summary">
            <span>{{ t('db.total') }} {{ fmtB(diskOverall.total) }}</span>
            <span>{{ t('db.used') }} {{ fmtB(diskOverall.used) }}</span>
            <span>{{ t('db.free') }} {{ fmtB(Math.max(0, diskOverall.total - diskOverall.used)) }}</span>
            <span>{{ t('db.used') }} {{ diskOverall.percent.toFixed(1) }}%</span>
          </div>
          <el-table :data="mergedDisks" size="small" stripe>
            <el-table-column :label="t('db.interface')" min-width="110">
              <template #default="{ row }">
                <div class="disk-name-cell">
                  <el-icon><Coin /></el-icon>
                  <span>{{ row.name }}</span>
                </div>
              </template>
            </el-table-column>
            <el-table-column prop="fs_type" :label="t('db.filesystem')" min-width="110" />
            <el-table-column :label="t('db.total')" width="110">
              <template #default="{ row }">{{ fmtB(row.total) }}</template>
            </el-table-column>
            <el-table-column :label="t('db.used')" width="110">
              <template #default="{ row }">{{ fmtB(row.used) }}</template>
            </el-table-column>
            <el-table-column :label="t('db.free')" width="110">
              <template #default="{ row }">{{ fmtB(row.free) }}</template>
            </el-table-column>
            <el-table-column :label="t('db.used')" width="170">
              <template #default="{ row }">
                <el-progress :percentage="Math.round(row.usage_percent)" :color="gaugeColor(row.usage_percent)" :stroke-width="10" />
              </template>
            </el-table-column>
          </el-table>
        </el-card>
      </el-col>

      <el-col :xs="24" :lg="10">
        <el-card shadow="never" class="detail-card">
          <template #header>
            <div class="card-header"><el-icon><Connection /></el-icon> {{ t('db.netDetail') }}</div>
          </template>
          <el-table :data="networkRows" size="small" stripe>
            <el-table-column :label="t('db.interface')" min-width="110">
              <template #default="{ row }">
                <div class="net-iface">
                  <el-icon><Connection /></el-icon>
                  <span>{{ row.name }}</span>
                </div>
              </template>
            </el-table-column>
            <el-table-column :label="t('db.received')" width="110">
              <template #default="{ row }">{{ fmtB(row.rx_bytes) }}</template>
            </el-table-column>
            <el-table-column :label="t('db.sent')" width="110">
              <template #default="{ row }">{{ fmtB(row.tx_bytes) }}</template>
            </el-table-column>
            <el-table-column :label="t('db.realtime')" min-width="136">
              <template #default="{ row }">
                <div class="net-stack">
                  <div class="net-metric">
                    <el-icon class="rx"><Download /></el-icon>
                    <span>{{ fmtB(row.rx_rate) }}/s</span>
                  </div>
                  <div class="net-metric">
                    <el-icon class="tx"><Upload /></el-icon>
                    <span>{{ fmtB(row.tx_rate) }}/s</span>
                  </div>
                </div>
              </template>
            </el-table-column>
            <el-table-column :label="t('db.peak')" min-width="136">
              <template #default="{ row }">
                <div class="net-stack">
                  <div class="net-metric">
                    <el-icon class="rx"><Download /></el-icon>
                    <span>{{ fmtB(row.rx_peak) }}/s</span>
                  </div>
                  <div class="net-metric">
                    <el-icon class="tx"><Upload /></el-icon>
                    <span>{{ fmtB(row.tx_peak) }}/s</span>
                  </div>
                </div>
              </template>
            </el-table-column>
          </el-table>
        </el-card>
      </el-col>
    </el-row>

    <el-dialog v-model="showProcessDialog" :title="t('db.loadProcesses')" width="760px" top="8vh" class="process-dialog" destroy-on-close>
      <div class="proc-toolbar">
        <span class="proc-note">{{ t('db.topNProcesses', { n: processLimit }) }}</span>
        <el-button size="small" :loading="processLoading" @click="fetchTopProcesses">{{ t('refresh') }}</el-button>
      </div>
      <div class="proc-summary">
        <span>{{ t('db.process') }}: {{ processSummary.count }}</span>
        <span>{{ t('db.cpu') }}: {{ processSummary.cpuTotal.toFixed(1) }}%</span>
        <span>{{ t('db.memory') }}: {{ fmtB(processSummary.memTotal) }}</span>
        <span>{{ t('db.topProcess') }}: {{ processSummary.topName }}</span>
      </div>
      <el-table :data="topProcesses" size="small" stripe v-loading="processLoading" max-height="420">
        <el-table-column prop="pid" :label="t('db.pid')" width="92" />
        <el-table-column prop="name" :label="t('db.process')" min-width="220" show-overflow-tooltip />
        <el-table-column :label="t('db.cpu')" width="100">
          <template #default="{ row }">{{ Number(row.cpu_percent || 0).toFixed(1) }}%</template>
        </el-table-column>
        <el-table-column :label="t('db.memory')" width="120">
          <template #default="{ row }">{{ fmtB(Number(row.memory || 0)) }}</template>
        </el-table-column>
        <el-table-column :label="t('db.status')" min-width="120">
          <template #default="{ row }">{{ row.status || '-' }}</template>
        </el-table-column>
      </el-table>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from '@/i18n'
import { getSystemInfo, getSystemProcesses } from '@/api'

const { t } = useI18n()

type DiskMergedRow = {
  name: string
  fs_type: string
  total: number
  used: number
  free: number
  usage_percent: number
}

type NetworkRuntimeRow = {
  rx_rate: number
  tx_rate: number
  rx_peak: number
  tx_peak: number
}

type ProcessRow = {
  pid: string
  name: string
  cpu_percent: number
  memory: number
  virtual_memory: number
  status: string
}

const info = ref<any>({})
const circumference = 2 * Math.PI * 52
const FAST_POLL_MS = 2000
const SLOW_POLL_MS = 7000
let pollTimer: number | null = null
let fetching = false
const processLimit = 20
const showProcessDialog = ref(false)
const processLoading = ref(false)
const topProcesses = ref<ProcessRow[]>([])
let processPollTimer: number | null = null

const networkRuntime = ref<Record<string, NetworkRuntimeRow>>({})
let lastNetworkSnapshot: { ts: number; counters: Record<string, { rx: number; tx: number }> } | null = null

function clampPercent(value: number) {
  if (!Number.isFinite(value)) return 0
  return Math.min(100, Math.max(0, value))
}

function diskDeviceKey(rawName: string): string {
  const value = String(rawName || '').trim()
  if (!value) return '-'
  if (/^[A-Za-z]:$/.test(value)) return value.toUpperCase()
  if (!value.startsWith('/dev/')) return value
  const name = value.slice('/dev/'.length)
  if (/^nvme\d+n\d+p\d+$/i.test(name) || /^mmcblk\d+p\d+$/i.test(name)) {
    return '/dev/' + name.replace(/p\d+$/i, '')
  }
  if (/^[a-z]+[0-9]+$/i.test(name)) {
    return '/dev/' + name.replace(/\d+$/, '')
  }
  return value
}

const VIRTUAL_FS_TYPES = new Set([
  'overlay', 'aufs', 'tmpfs', 'devtmpfs', 'squashfs', 'ramfs', 'zram', 'nsfs', 'tracefs',
  'securityfs', 'cgroup', 'cgroup2', 'autofs', 'proc', 'procfs', 'sysfs', 'devpts',
  'mqueue', 'hugetlbfs', 'fusectl', 'pstore', 'configfs', 'debugfs', 'rpc_pipefs', 'bpf',
  'binfmt_misc',
])

function isRealDiskRow(row: any): boolean {
  const fsType = String(row?.fs_type || '').trim().toLowerCase()
  const name = String(row?.name || '').trim().toLowerCase()
  const mountPoint = String(row?.mount_point || '').trim().toLowerCase()
  const total = Number(row?.total || 0)
  if (!Number.isFinite(total) || total <= 0) return false
  if (VIRTUAL_FS_TYPES.has(fsType)) return false
  if (name === 'overlay' || fsType === 'overlay') return false
  if (mountPoint.startsWith('/proc') || mountPoint.startsWith('/sys') || mountPoint.startsWith('/run')) return false
  return true
}

const mergedDisks = computed<DiskMergedRow[]>(() => {
  const rawRows = Array.isArray(info.value.disks) ? info.value.disks : []
  const preferredRows = rawRows.filter((row: any) => isRealDiskRow(row))
  const rows = preferredRows.length > 0
    ? preferredRows
    : rawRows.filter((row: any) => Number(row?.total || 0) > 0)

  const map = new Map<string, {
    name: string
    fs_types: Set<string>
    total: number
    used: number
    free: number
    seen: Set<string>
  }>()

  for (const row of rows) {
    const key = diskDeviceKey(String(row?.name || row?.mount_point || '-'))
    if (!map.has(key)) {
      map.set(key, {
        name: key,
        fs_types: new Set<string>(),
        total: 0,
        used: 0,
        free: 0,
        seen: new Set<string>(),
      })
    }
    const item = map.get(key)!
    const fsType = String(row?.fs_type || '').trim()
    const total = Number(row?.total || 0)
    const used = Number(row?.used || 0)
    const free = Number(row?.free || 0)
    const signature = `${Math.round(total / 1024 / 1024)}:${Math.round(used / 1024 / 1024)}:${Math.round(free / 1024 / 1024)}:${fsType}`
    if (item.seen.has(signature)) continue
    item.seen.add(signature)
    item.total += total
    item.used += used
    item.free += free
    if (fsType) item.fs_types.add(fsType)
  }

  return Array.from(map.values())
    .map((item) => ({
      name: item.name,
      fs_type: item.fs_types.size > 0 ? Array.from(item.fs_types).join(', ') : '-',
      total: item.total,
      used: item.used,
      free: item.free,
      usage_percent: item.total > 0 ? (item.used / item.total) * 100 : 0,
    }))
    .sort((a, b) => b.total - a.total)
})

const loadDisplay = computed(() => {
  const load = info.value.platform?.load_avg || [0, 0, 0]
  const one = Number(load[0] || 0).toFixed(2)
  const five = Number(load[1] || 0).toFixed(2)
  const fifteen = Number(load[2] || 0).toFixed(2)
  return `${one} / ${five} / ${fifteen}`
})

const memoryPercent = computed(() => clampPercent(Number(info.value.memory?.usage_percent || 0)))
const memoryAvailablePercent = computed(() => {
  const total = Number(info.value.memory?.total || 0)
  const available = Number(info.value.memory?.available || 0)
  return total > 0 ? clampPercent((available / total) * 100) : 0
})
const memoryFreePercent = computed(() => {
  const total = Number(info.value.memory?.total || 0)
  const free = Number(info.value.memory?.free || 0)
  return total > 0 ? clampPercent((free / total) * 100) : 0
})

const cpuCoreStats = computed(() => {
  const list: number[] = Array.isArray(info.value.cpu?.per_core_usage) ? info.value.cpu.per_core_usage : []
  if (!list.length) {
    return { avg: 0, max: 0, maxIndex: -1, min: 0, minIndex: -1 }
  }
  let max = -1
  let maxIndex = -1
  let min = 101
  let minIndex = -1
  let sum = 0
  list.forEach((value, index) => {
    const v = Number(value || 0)
    sum += v
    if (v > max) { max = v; maxIndex = index }
    if (v < min) { min = v; minIndex = index }
  })
  return { avg: sum / list.length, max, maxIndex, min, minIndex }
})

const diskOverall = computed(() => {
  const total = mergedDisks.value.reduce((sum, row) => sum + row.total, 0)
  const used = mergedDisks.value.reduce((sum, row) => sum + row.used, 0)
  const percent = total > 0 ? (used / total) * 100 : 0
  return { total, used, percent: clampPercent(percent) }
})

const cpuFrequencyDisplay = computed(() => {
  const mhz = Number(info.value.cpu?.frequency_mhz || 0)
  if (mhz <= 0) return '0 MHz'
  if (mhz >= 1000) return `${(mhz / 1000).toFixed(2)} GHz`
  return `${Math.round(mhz)} MHz`
})

const processSummary = computed(() => {
  const rows = topProcesses.value
  const cpuTotal = rows.reduce((sum, row) => sum + Number(row.cpu_percent || 0), 0)
  const memTotal = rows.reduce((sum, row) => sum + Number(row.memory || 0), 0)
  const topName = rows[0]?.name || '-'
  return { count: rows.length, cpuTotal, memTotal, topName }
})

const gauges = computed(() => {
  const load = info.value.platform?.load_avg || [0, 0, 0]
  const physicalCores = Math.max(1, Number(info.value.cpu?.physical_cores || 1))
  const logicalCores = Math.max(1, Number(info.value.cpu?.logical_cores || 1))
  const loadOne = Number(load[0] || 0)
  const loadFive = Number(load[1] || 0)
  const loadFifteen = Number(load[2] || 0)
  const loadPercent = clampPercent((loadOne / logicalCores) * 100)
  const cpuPercent = clampPercent(Number(info.value.cpu?.usage_percent || 0))
  const memPercent = memoryPercent.value
  const diskPercent = diskOverall.value.percent

  return [
    {
      key: 'load',
      title: t('db.load'),
      percent: loadPercent,
      sub: `1m ${loadOne.toFixed(2)} / ${logicalCores}C`,
      details: [
        `1m: ${loadOne.toFixed(2)}`,
        `5m: ${loadFive.toFixed(2)}`,
        `15m: ${loadFifteen.toFixed(2)}`,
        `${t('db.loadPerCore')}: ${loadPercent.toFixed(1)}%`,
        `${t('db.logicCores')}: ${logicalCores}`,
        t('db.viewProcessesHint'),
      ],
    },
    {
      key: 'cpu',
      title: 'CPU',
      percent: cpuPercent,
      sub: `${logicalCores} cores`,
      details: [
        `${t('db.used')}: ${cpuPercent.toFixed(1)}%`,
        `${t('db.physCores')}: ${physicalCores}`,
        `${t('db.logicCores')}: ${logicalCores}`,
        `${t('db.coreAvg')}: ${cpuCoreStats.value.avg.toFixed(1)}%`,
        `${t('db.coreMax')}: #${cpuCoreStats.value.maxIndex >= 0 ? cpuCoreStats.value.maxIndex : 0} ${cpuCoreStats.value.max.toFixed(1)}%`,
        `${t('db.model')}: ${info.value.cpu?.model_name || '-'}`,
        `${t('db.frequency')}: ${cpuFrequencyDisplay.value}`,
      ],
    },
    {
      key: 'memory',
      title: t('db.memory'),
      percent: memPercent,
      sub: `${fmtB(info.value.memory?.used)} / ${fmtB(info.value.memory?.total)}`,
      details: [
        `${t('db.used')}: ${fmtB(info.value.memory?.used)}`,
        `${t('db.total')}: ${fmtB(info.value.memory?.total)}`,
        `${t('db.available')}: ${fmtB(info.value.memory?.available)}`,
        `${t('db.free')}: ${fmtB(info.value.memory?.free)}`,
        `${t('db.used')}: ${memPercent.toFixed(1)}%`,
      ],
    },
    {
      key: 'disk',
      title: t('db.disk'),
      percent: diskPercent,
      sub: `${fmtB(diskOverall.value.total)} ${t('db.total')}`,
      details: [
        `${t('db.totalCapacity')}: ${fmtB(diskOverall.value.total)}`,
        `${t('db.used')}: ${fmtB(diskOverall.value.used)}`,
        `${t('db.free')}: ${fmtB(Math.max(0, diskOverall.value.total - diskOverall.value.used))}`,
        `${t('db.used')}: ${diskPercent.toFixed(1)}%`,
        `${t('db.diskDetail')}: ${mergedDisks.value.length}`,
      ],
    },
  ]
})

const networkRows = computed(() => {
  const rows = Array.isArray(info.value.networks) ? info.value.networks : []
  return rows
    .map((row: any) => {
      const runtime = networkRuntime.value[String(row?.name || '')] || { rx_rate: 0, tx_rate: 0, rx_peak: 0, tx_peak: 0 }
      return { ...row, ...runtime }
    })
    .filter((row: any) => row.rx_bytes > 0 || row.tx_bytes > 0 || row.rx_rate > 0 || row.tx_rate > 0)
    .sort((a: any, b: any) => (b.rx_rate + b.tx_rate) - (a.rx_rate + a.tx_rate))
})

function updateNetworkRuntime(networks: any[], timestamp: number) {
  const previous = lastNetworkSnapshot
  const nextCounters: Record<string, { rx: number; tx: number }> = {}
  const nextRuntime: Record<string, NetworkRuntimeRow> = {}
  const dt = previous ? Math.max(0.001, (timestamp - previous.ts) / 1000) : 0

  for (const row of networks) {
    const name = String(row?.name || '')
    if (!name) continue
    const rx = Number(row?.rx_bytes || 0)
    const tx = Number(row?.tx_bytes || 0)
    nextCounters[name] = { rx, tx }

    const previousCounters = previous?.counters[name]
    const oldRuntime = networkRuntime.value[name] || { rx_rate: 0, tx_rate: 0, rx_peak: 0, tx_peak: 0 }

    let rxRate = 0
    let txRate = 0
    if (previousCounters && dt > 0) {
      const rxDelta = rx - previousCounters.rx
      const txDelta = tx - previousCounters.tx
      rxRate = rxDelta > 0 ? rxDelta / dt : 0
      txRate = txDelta > 0 ? txDelta / dt : 0
    }

    nextRuntime[name] = {
      rx_rate: rxRate,
      tx_rate: txRate,
      rx_peak: Math.max(oldRuntime.rx_peak, rxRate),
      tx_peak: Math.max(oldRuntime.tx_peak, txRate),
    }
  }

  networkRuntime.value = nextRuntime
  lastNetworkSnapshot = { ts: timestamp, counters: nextCounters }
}

function gaugeColor(pct: number): string {
  if (pct < 50) return 'var(--lp-gauge-low)'
  if (pct < 80) return 'var(--lp-gauge-mid)'
  return 'var(--lp-gauge-high)'
}

function fmtB(b: number): string {
  if (!b) return '0 B'
  const unit = ['B', 'KB', 'MB', 'GB', 'TB']
  const index = Math.floor(Math.log(b) / Math.log(1024))
  return (b / Math.pow(1024, index)).toFixed(1) + ' ' + unit[index]
}

function clearPollTimer() {
  if (pollTimer !== null) {
    window.clearTimeout(pollTimer)
    pollTimer = null
  }
}

function clearProcessPollTimer() {
  if (processPollTimer !== null) {
    window.clearTimeout(processPollTimer)
    processPollTimer = null
  }
}

function scheduleProcessPoll() {
  clearProcessPollTimer()
  if (!showProcessDialog.value) return
  processPollTimer = window.setTimeout(async () => {
    await fetchTopProcesses()
    scheduleProcessPoll()
  }, 3500)
}

async function fetchTopProcesses() {
  if (processLoading.value) return
  processLoading.value = true
  try {
    const res: any = await getSystemProcesses(processLimit)
    if (res.success) {
      topProcesses.value = Array.isArray(res.data) ? res.data : []
    }
  } catch {
  } finally {
    processLoading.value = false
  }
}

function openProcessDialog() {
  showProcessDialog.value = true
}

function schedulePoll() {
  clearPollTimer()
  const delay = document.hidden ? SLOW_POLL_MS : FAST_POLL_MS
  pollTimer = window.setTimeout(async () => {
    await fetchInfo()
    schedulePoll()
  }, delay)
}

async function fetchInfo() {
  if (fetching) return
  fetching = true
  try {
    const res: any = await getSystemInfo()
    if (res.success) {
      const data = res.data || {}
      updateNetworkRuntime(Array.isArray(data.networks) ? data.networks : [], Date.now())
      info.value = data
    }
  } catch {
  } finally {
    fetching = false
  }
}

function onVisibilityChange() {
  if (!document.hidden) {
    void fetchInfo()
  }
  schedulePoll()
}

watch(showProcessDialog, (visible) => {
  if (visible) {
    void fetchTopProcesses()
    scheduleProcessPoll()
  } else {
    clearProcessPollTimer()
  }
})

onMounted(async () => {
  await fetchInfo()
  schedulePoll()
  document.addEventListener('visibilitychange', onVisibilityChange)
})

onUnmounted(() => {
  clearPollTimer()
  clearProcessPollTimer()
  document.removeEventListener('visibilitychange', onVisibilityChange)
})
</script>

<style scoped lang="scss">
.dashboard { max-width: 1400px; }

.platform-bar {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  padding: 16px 20px;
  background: var(--lp-card-bg);
  border-radius: var(--lp-radius-lg);
  box-shadow: var(--lp-shadow-sm);
  margin-bottom: 16px;
}

.platform-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  .label { color: var(--lp-text-secondary); }
  .value { color: var(--lp-text-primary); font-weight: 500; }
  .el-icon { color: var(--lp-primary); }
}

.gauge-row { margin-bottom: 0; }

.gauge-card {
  background: var(--lp-card-bg);
  border-radius: var(--lp-radius-lg);
  padding: 20px;
  display: flex;
  align-items: center;
  gap: 16px;
  box-shadow: var(--lp-shadow-sm);
  transition: box-shadow var(--lp-transition), transform var(--lp-transition);
  &:hover { box-shadow: var(--lp-shadow); transform: translateY(-1px); }
}

.gauge-tip-title {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 4px;
}

.gauge-tip-content {
  min-width: 190px;
  text-align: center;
}

.gauge-tip-line {
  font-size: 12px;
  line-height: 1.45;
}

.gauge-tip-actions {
  margin-top: 8px;
  padding-top: 6px;
  border-top: 1px solid var(--lp-border-light);
}

.gauge-tip-action {
  font-size: 12px;
  font-weight: 600;
  padding: 0 !important;
}

.gauge-ring {
  position: relative;
  width: 80px;
  height: 80px;
  flex-shrink: 0;
  svg { width: 100%; height: 100%; }
}

.gauge-center {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
}

.gauge-pct { font-size: 16px; font-weight: 700; color: var(--lp-text-primary); }
.gauge-title { font-size: 15px; font-weight: 600; color: var(--lp-text-primary); }
.gauge-sub { font-size: 12px; color: var(--lp-text-secondary); margin-top: 2px; }
.gauge-meta { min-width: 0; }

.detail-card {
  background: var(--lp-card-bg);
  border: 1px solid var(--lp-border-light);
  border-radius: var(--lp-radius-lg);
  :deep(.el-card__header) { padding: 14px 20px; border-bottom: 1px solid var(--lp-border-light); }
  :deep(.el-card__body) { padding: 16px 20px; }
}

.card-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 15px;
  font-weight: 600;
  color: var(--lp-text-primary);
  .el-icon { color: var(--lp-primary); }
}

.core-grid {
  display: flex;
  gap: 3px;
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid var(--lp-border-light);
  align-items: flex-end;
  height: 80px;
}

.core-item {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  height: 100%;
}

.core-bar-bg {
  flex: 1;
  width: 100%;
  max-width: 16px;
  background: var(--lp-border-light);
  border-radius: 3px;
  position: relative;
  overflow: hidden;
  display: flex;
  align-items: flex-end;
}

.core-bar {
  width: 100%;
  border-radius: 3px;
  transition: height 0.45s ease, background 0.25s;
  min-height: 2px;
}

.core-label { font-size: 9px; color: var(--lp-text-placeholder); margin-top: 3px; }

.mem-bar-wrap { margin-top: 16px; padding-top: 12px; border-top: 1px solid var(--lp-border-light); }
.mem-bar-label { display: flex; justify-content: space-between; font-size: 12px; color: var(--lp-text-secondary); margin-bottom: 4px; margin-top: 8px; }
.mem-bar-track { height: 10px; background: var(--lp-border-light); border-radius: 5px; overflow: hidden; }
.mem-bar-fill { height: 100%; border-radius: 5px; transition: width 0.45s ease, background 0.25s; }
.kv-summary {
  margin-top: 10px;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  font-size: 12px;
  color: var(--lp-text-secondary);
  span {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--lp-border-light);
    background: var(--lp-table-header-bg);
  }
}
.disk-summary { margin-bottom: 10px; }

.disk-name-cell {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  .el-icon { color: var(--lp-warning); }
}

.net-iface {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  .el-icon { color: var(--lp-primary); }
}

.net-stack {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.net-metric {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  line-height: 1.2;
  font-family: 'Cascadia Code', 'Fira Code', monospace;
  .rx { color: var(--lp-success); }
  .tx { color: var(--lp-warning); }
}

.proc-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.proc-note { font-size: 12px; color: var(--lp-text-secondary); }
.proc-summary {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 12px;
  span {
    display: inline-flex;
    align-items: center;
    padding: 3px 10px;
    border-radius: 999px;
    border: 1px solid var(--lp-border-light);
    background: var(--lp-table-header-bg);
    font-size: 12px;
    color: var(--lp-text-secondary);
  }
}

:deep(.process-dialog .el-dialog) {
  border-radius: var(--lp-radius-lg);
  border: 1px solid var(--lp-border-light);
  overflow: hidden;
}

:deep(.process-dialog .el-dialog__header) {
  padding: 14px 18px;
  border-bottom: 1px solid var(--lp-border-light);
  background: var(--lp-table-header-bg);
}

:deep(.process-dialog .el-dialog__body) {
  padding: 14px 18px 16px;
}
</style>
