<template>
  <div class="lp-page dash">
    <div class="dash-grid">
      <div class="dash-main">
        <!-- Overview: what the panel manages -->
        <section class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title">{{ t('ui.overview') }}</h2>
            <span class="lp-dim head-note">{{ subtitle }}</span>
          </header>
          <div class="overview">
            <router-link v-for="item in overview" :key="item.to" :to="item.to" class="overview__item">
              <span class="overview__label">{{ item.label }}</span>
              <span class="overview__value lp-num">{{ item.value }}</span>
              <span v-if="item.note" class="overview__note">{{ item.note }}</span>
            </router-link>
          </div>
        </section>

        <!-- Status gauges -->
        <section class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title">{{ t('ui.status') }}</h2>
          </header>
          <div v-if="!latest" class="lp-card-body"><el-skeleton :rows="3" animated /></div>
          <div v-else class="gauges">
            <el-tooltip
              v-for="gauge in gauges"
              :key="gauge.key"
              :disabled="!gauge.tip"
              placement="bottom"
              :show-after="200"
            >
              <template #content><div class="gauge-tip">{{ gauge.tip }}</div></template>
              <div class="gauge">
                <el-progress
                  type="dashboard"
                  :percentage="Math.min(100, Math.round(gauge.percent * 10) / 10)"
                  :width="128"
                  :stroke-width="9"
                  :color="levelColor(gauge.percent)"
                >
                  <template #default>
                    <span class="gauge__pct lp-num">{{ gauge.percent.toFixed(1) }}<small>%</small></span>
                    <span class="gauge__name">{{ gauge.label }}</span>
                  </template>
                </el-progress>
                <div class="gauge__sub lp-truncate">{{ gauge.sub }}</div>
              </div>
            </el-tooltip>
          </div>
        </section>

        <!-- Monitor: one chart, switchable -->
        <section class="lp-card">
          <header class="lp-card-head monitor-head">
            <h2 class="lp-card-title">{{ t('ui.monitor') }}</h2>
            <div class="monitor-tools">
              <el-radio-group v-model="metric" size="small">
                <el-radio-button value="net">{{ t('ui.traffic') }}</el-radio-button>
                <el-radio-button value="io">{{ t('ui.diskIo') }}</el-radio-button>
                <el-radio-button value="cpu">CPU</el-radio-button>
                <el-radio-button value="mem">{{ t('ui.memory') }}</el-radio-button>
              </el-radio-group>
              <el-select v-model="range" size="small" class="range">
                <el-option v-for="option in rangeOptions" :key="option.value" :value="option.value" :label="option.label" />
              </el-select>
            </div>
          </header>
          <div v-if="latest" class="monitor-stats">
            <template v-if="metric === 'net'">
              <span><i class="sw" style="background: var(--lp-series-3)"></i>{{ t('ui.up') }} <b class="lp-num">{{ formatRate(latest.net_tx_rate) }}</b></span>
              <span><i class="sw" style="background: var(--lp-series-1)"></i>{{ t('ui.down') }} <b class="lp-num">{{ formatRate(latest.net_rx_rate) }}</b></span>
              <span>{{ t('ui.sent') }} <b class="lp-num">{{ formatBytes(netTotals.tx) }}</b></span>
              <span>{{ t('ui.received') }} <b class="lp-num">{{ formatBytes(netTotals.rx) }}</b></span>
            </template>
            <template v-else-if="metric === 'io'">
              <span><i class="sw" style="background: var(--lp-series-6)"></i>{{ t('ui.read') }} <b class="lp-num">{{ formatRate(latest.disk_read_rate) }}</b></span>
              <span><i class="sw" style="background: var(--lp-series-4)"></i>{{ t('ui.write') }} <b class="lp-num">{{ formatRate(latest.disk_write_rate) }}</b></span>
            </template>
            <template v-else-if="metric === 'cpu'">
              <span><i class="sw" style="background: var(--lp-series-1)"></i>CPU <b class="lp-num">{{ latest.cpu_percent.toFixed(1) }}%</b></span>
              <span>{{ t('ui.load') }} <b class="lp-num">{{ loadText }}</b></span>
            </template>
            <template v-else>
              <span><i class="sw" style="background: var(--lp-series-2)"></i>{{ t('ui.memory') }} <b class="lp-num">{{ latest.mem_percent.toFixed(1) }}%</b></span>
              <span>{{ t('ui.used') }} <b class="lp-num">{{ formatBytes(latest.mem_used) }}</b></span>
              <span>{{ t('ui.total') }} <b class="lp-num">{{ formatBytes(latest.mem_total) }}</b></span>
            </template>
          </div>
          <div class="monitor-chart">
            <MetricChart
              :series="monitorSeries"
              :height="280"
              :percent="metric === 'cpu' || metric === 'mem'"
              :formatter="metric === 'net' || metric === 'io' ? formatRateShort : undefined"
            />
          </div>
        </section>
      </div>

      <aside class="dash-side">
        <!-- System information -->
        <section class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title">{{ t('ui.systemInfo') }}</h2>
          </header>
          <dl class="info">
            <div><dt>{{ t('ui.hostname') }}</dt><dd class="lp-truncate">{{ staticInfo?.hostname || '—' }}</dd></div>
            <div><dt>{{ t('ui.os') }}</dt><dd class="lp-truncate">{{ staticInfo?.os_version || staticInfo?.os_name || '—' }}</dd></div>
            <div><dt>{{ t('ui.kernel') }}</dt><dd class="lp-truncate">{{ staticInfo?.kernel_version || '—' }}</dd></div>
            <div><dt>{{ t('ui.arch') }}</dt><dd>{{ staticInfo?.arch || staticInfo?.platform || '—' }}</dd></div>
            <div><dt>CPU</dt><dd class="lp-truncate" :title="staticInfo?.cpu_model">{{ staticInfo?.cpu_model || '—' }}</dd></div>
            <div><dt>{{ t('db.physCores') }}</dt><dd>{{ staticInfo?.physical_cores || '—' }} / {{ staticInfo?.logical_cores || '—' }}<span v-if="frequencyText !== '-'" class="lp-dim"> · {{ frequencyText }}</span></dd></div>
            <div><dt>{{ t('ui.bootTime') }}</dt><dd>{{ bootText }}</dd></div>
            <div><dt>{{ t('ui.uptime') }}</dt><dd>{{ latest ? formatDuration(latest.uptime) : '—' }}</dd></div>
            <div><dt>LinxPanel</dt><dd>v{{ staticInfo?.panel_version || '—' }}</dd></div>
          </dl>
        </section>

        <!-- Disks -->
        <section class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title">{{ t('ui.disk') }}</h2>
            <span class="lp-dim head-note">{{ formatBytes(diskOverall.used) }} / {{ formatBytes(diskOverall.total) }}</span>
          </header>
          <div class="bars">
            <div v-for="disk in disks" :key="disk.name + disk.mount_point" class="bar">
              <div class="bar__top">
                <span class="lp-truncate bar__name" :title="disk.name">{{ disk.mount_point }}</span>
                <span class="lp-dim lp-num">{{ formatBytes(disk.used) }} / {{ formatBytes(disk.total) }}</span>
              </div>
              <div class="bar__track"><div class="bar__fill" :style="{ width: disk.usage_percent + '%', background: levelColor(disk.usage_percent) }"></div></div>
            </div>
            <p v-if="!disks.length" class="lp-dim">{{ t('db.noDisks') }}</p>
          </div>
        </section>

        <!-- Network interfaces -->
        <section class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title">{{ t('db.interfaces') }}</h2>
          </header>
          <ul class="ifaces">
            <li v-for="item in activeInterfaces" :key="item.name" class="iface">
              <div class="iface__main">
                <span class="iface__name">{{ item.name }}</span>
                <span class="lp-dim lp-truncate lp-mono iface__ip">{{ item.ip_addresses[0] || item.mac_address || '' }}</span>
              </div>
              <div class="iface__rates lp-num">
                <span class="rx">↓ {{ formatRateShort(item.rx_rate) }}</span>
                <span class="tx">↑ {{ formatRateShort(item.tx_rate) }}</span>
              </div>
            </li>
            <li v-if="!activeInterfaces.length" class="lp-dim">{{ t('db.noInterfaces') }}</li>
          </ul>
        </section>
      </aside>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { storeToRefs } from 'pinia'
import MetricChart, { type Series } from '@/components/MetricChart.vue'
import { getSystemCounts } from '@/api'
import { useRealtimeStore } from '@/stores/realtime'
import { useI18n } from '@/i18n'
import { usePage } from '@/utils/page'
import { formatBytes, formatDuration, formatFullTime, formatRate, formatRateShort } from '@/utils/format'

const { t } = useI18n()
const realtime = useRealtimeStore()
const { latest, staticInfo, detail, history } = storeToRefs(realtime)

/** Window of history to plot, in seconds. */
const range = ref(300)
const rangeOptions = [
  { value: 60, label: '1m' },
  { value: 120, label: '2m' },
  { value: 300, label: '5m' },
  { value: 600, label: '10m' },
]
const metric = ref<'net' | 'io' | 'cpu' | 'mem'>('net')

const counts = ref<{ sites: number; sites_running: number; databases: number; cron_jobs: number; apps: number } | null>(null)
async function loadCounts() {
  try {
    const res: any = await getSystemCounts()
    if (res.success) counts.value = res.data
  } catch {
    /* the card keeps its last numbers */
  }
}
onMounted(loadCounts)
usePage(loadCounts)

const overview = computed(() => {
  const c = counts.value
  const n = (v: number | undefined) => (c ? String(v ?? 0) : '—')
  return [
    { to: '/sites', label: t('ui.sites'), value: n(c?.sites), note: c && c.sites ? `${c.sites_running} ${t('ui.running')}` : '' },
    { to: '/databases', label: t('ui.databases'), value: n(c?.databases), note: '' },
    { to: '/cron', label: t('ui.cronJobs'), value: n(c?.cron_jobs), note: '' },
    { to: '/appstore', label: t('ui.apps'), value: n(c?.apps), note: '' },
  ]
})

const windowed = computed(() => {
  const points = history.value
  if (!points.length) return []
  const cutoff = points[points.length - 1].ts - range.value * 1000
  // History is append-only and ordered, so a findIndex from the front is enough.
  const start = points.findIndex((point) => point.ts >= cutoff)
  return start <= 0 ? points : points.slice(start)
})

function seriesOf(pick: (sample: (typeof windowed.value)[number]) => number): [number, number][] {
  return windowed.value.map((sample) => [sample.ts, pick(sample)])
}

function cssVar(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}

const monitorSeries = computed<Series[]>(() => {
  switch (metric.value) {
    case 'cpu':
      return [{ name: 'CPU', color: cssVar('--lp-series-1'), data: seriesOf((s) => s.cpu_percent) }]
    case 'mem':
      return [{ name: t('ui.memory'), color: cssVar('--lp-series-2'), data: seriesOf((s) => s.mem_percent) }]
    case 'io':
      return [
        { name: t('ui.read'), color: cssVar('--lp-series-6'), data: seriesOf((s) => s.disk_read_rate) },
        { name: t('ui.write'), color: cssVar('--lp-series-4'), data: seriesOf((s) => s.disk_write_rate) },
      ]
    default:
      return [
        { name: t('ui.up'), color: cssVar('--lp-series-3'), data: seriesOf((s) => s.net_tx_rate) },
        { name: t('ui.down'), color: cssVar('--lp-series-1'), data: seriesOf((s) => s.net_rx_rate) },
      ]
  }
})

const disks = computed(() => {
  const rows = detail.value?.disks || []
  // The engine reports every mount; collapse to real volumes for the overview.
  const seen = new Set<string>()
  return rows
    .filter((disk) => disk.total > 0)
    .filter((disk) => {
      const key = `${disk.name}|${disk.total}`
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })
    .sort((a, b) => b.total - a.total)
})

const diskOverall = computed(() => {
  const total = disks.value.reduce((sum, disk) => sum + disk.total, 0)
  const used = disks.value.reduce((sum, disk) => sum + disk.used, 0)
  return { total, used, percent: total > 0 ? (used / total) * 100 : 0 }
})

const activeInterfaces = computed(() =>
  (detail.value?.interfaces || [])
    .filter((item) => item.rx_bytes > 0 || item.tx_bytes > 0)
    .sort((a, b) => b.rx_rate + b.tx_rate - (a.rx_rate + a.tx_rate))
    .slice(0, 6),
)

const netTotals = computed(() =>
  (detail.value?.interfaces || [])
    .filter((item) => item.name !== 'lo')
    .reduce((sum, item) => ({ rx: sum.rx + item.rx_bytes, tx: sum.tx + item.tx_bytes }), { rx: 0, tx: 0 }),
)

const loadText = computed(() => {
  const load = latest.value?.load_avg || [0, 0, 0]
  return load.map((value) => value.toFixed(2)).join(' / ')
})

const cores = computed(() => staticInfo.value?.logical_cores || 1)

/** Load, CPU, memory, then the largest volumes. */
const gauges = computed(() => {
  const sample = latest.value
  if (!sample) return []
  const load1 = sample.load_avg?.[0] ?? 0
  const loadPercent = Math.min(100, (load1 / cores.value) * 100)
  const level =
    loadPercent >= 90 ? 'full' : loadPercent >= 70 ? 'high' : loadPercent >= 30 ? 'mid' : 'low'
  const list = [
    {
      key: 'load',
      label: t('ui.load'),
      percent: loadPercent,
      sub: t(`ui.loadLevel.${level}`),
      tip: `${t('ui.load')} (1 / 5 / 15 min): ${loadText.value}`,
    },
    {
      key: 'cpu',
      label: 'CPU',
      percent: sample.cpu_percent,
      sub: t('ui.cores', { n: cores.value }),
      tip: (sample.cpu_cores || []).map((v, i) => `CPU${i}: ${v.toFixed(1)}%`).join('\n'),
    },
    {
      key: 'mem',
      label: t('ui.memory'),
      percent: sample.mem_percent,
      sub: `${formatBytes(sample.mem_used)} / ${formatBytes(sample.mem_total)}`,
      tip: sample.swap_total ? `Swap: ${formatBytes(sample.swap_used)} / ${formatBytes(sample.swap_total)}` : '',
    },
  ]
  for (const disk of disks.value.slice(0, 1)) {
    list.push({
      key: `disk-${disk.mount_point}`,
      label: disk.mount_point,
      percent: disk.usage_percent,
      sub: `${formatBytes(disk.used)} / ${formatBytes(disk.total)}`,
      tip: `${disk.name} · ${disk.fs_type}`,
    })
  }
  return list
})

const frequencyText = computed(() => {
  const mhz = staticInfo.value?.cpu_frequency_mhz || 0
  if (!mhz) return '-'
  return mhz >= 1000 ? `${(mhz / 1000).toFixed(2)} GHz` : `${mhz} MHz`
})

const bootText = computed(() => (staticInfo.value?.boot_time ? formatFullTime(staticInfo.value.boot_time) : '—'))

const subtitle = computed(() => {
  const info = staticInfo.value
  if (!info) return ''
  return `${info.os_version || info.os_name} · ${info.platform}`
})

function levelColor(percent: number) {
  if (percent >= 88) return cssVar('--lp-danger')
  if (percent >= 70) return cssVar('--lp-warn')
  return cssVar('--lp-accent')
}
</script>

<style scoped lang="scss">
.dash-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 340px;
  gap: var(--lp-3);
  align-items: start;
}
.dash-main,
.dash-side { display: flex; flex-direction: column; gap: var(--lp-3); min-width: 0; }
@media (max-width: 1100px) {
  .dash-grid { grid-template-columns: 1fr; }
}
.head-note { font-size: var(--lp-text-sm); }

/* Overview */
.overview {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  padding: var(--lp-4) var(--lp-2);
}
.overview__item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--lp-2) var(--lp-5);
  color: inherit;
  text-decoration: none;
  border-left: 1px solid var(--lp-line-soft);
  &:first-child { border-left: 0; }
  &:hover .overview__value { color: var(--lp-accent); }
}
.overview__label { font-size: var(--lp-text-sm); color: var(--lp-text-dim); }
.overview__value { font-size: 28px; font-weight: 600; line-height: 1.3; color: var(--lp-text); transition: color var(--lp-fast); }
.overview__note { font-size: var(--lp-text-xs); color: var(--lp-ok); }
@media (max-width: 640px) {
  .overview { grid-template-columns: repeat(2, minmax(0, 1fr)); row-gap: var(--lp-3); }
  .overview__item:nth-child(3) { border-left: 0; }
}

/* Gauges */
.gauges {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--lp-4);
  padding: var(--lp-5);
}
.gauge { display: flex; flex-direction: column; align-items: center; gap: 4px; min-width: 0; cursor: default; }
.gauge :deep(.el-progress__text) { display: flex; flex-direction: column; align-items: center; gap: 2px; color: var(--lp-text); }
.gauge :deep(.el-progress-circle__track) { stroke: var(--lp-line-soft); }
.gauge__pct { font-size: 22px; font-weight: 600; small { font-size: 12px; margin-left: 1px; color: var(--lp-text-dim); } }
.gauge__name { font-size: var(--lp-text-sm); color: var(--lp-text-soft); max-width: 90px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gauge__sub { font-size: var(--lp-text-xs); color: var(--lp-text-dim); max-width: 100%; }
.gauge-tip { white-space: pre-line; font-family: var(--lp-font-mono); font-size: 12px; }

@media (max-width: 640px) {
  .gauges { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}

/* Monitor */
.monitor-head { flex-wrap: wrap; }
.monitor-tools { display: flex; align-items: center; gap: var(--lp-2); flex-wrap: wrap; }
.range { width: 80px; }
.monitor-stats {
  display: flex;
  gap: var(--lp-2) var(--lp-6);
  flex-wrap: wrap;
  padding: var(--lp-3) var(--lp-5) 0;
  font-size: var(--lp-text-sm);
  color: var(--lp-text-dim);
  b { color: var(--lp-text); font-weight: 600; margin-left: 4px; }
}
.sw { display: inline-block; width: 8px; height: 8px; border-radius: 2px; margin-right: 6px; vertical-align: 0; }
.monitor-chart { padding: var(--lp-2) var(--lp-4) var(--lp-4); }

/* Side cards */
.info {
  margin: 0;
  padding: var(--lp-3) var(--lp-5) var(--lp-4);
  div { display: grid; grid-template-columns: 88px minmax(0, 1fr); gap: var(--lp-3); padding: 6px 0; font-size: var(--lp-text-sm); }
  dt { color: var(--lp-text-dim); }
  dd { margin: 0; color: var(--lp-text); }
}
.bars { display: flex; flex-direction: column; gap: var(--lp-3); padding: var(--lp-4) var(--lp-5); }
.bar__top { display: flex; justify-content: space-between; gap: var(--lp-3); font-size: var(--lp-text-sm); margin-bottom: 6px; }
.bar__name { color: var(--lp-text); }
.bar__track { height: 6px; border-radius: 3px; background: var(--lp-line-soft); overflow: hidden; }
.bar__fill { height: 100%; border-radius: 3px; transition: width var(--lp-base); }
.ifaces { padding: var(--lp-2) var(--lp-5) var(--lp-3); }
.iface {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--lp-3);
  padding: 8px 0;
  border-bottom: 1px solid var(--lp-line-soft);
  font-size: var(--lp-text-sm);
  &:last-child { border-bottom: 0; }
}
.iface__main { display: flex; flex-direction: column; min-width: 0; }
.iface__name { color: var(--lp-text); font-weight: 500; }
.iface__ip { font-size: 11px; }
.iface__rates { display: flex; flex-direction: column; align-items: flex-end; font-size: var(--lp-text-xs); }
.rx { color: var(--lp-series-1); }
.tx { color: var(--lp-series-3); }
</style>
