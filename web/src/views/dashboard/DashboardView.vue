<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('dashboard') }}</h1>
        <p class="lp-page-sub">{{ subtitle }}</p>
      </div>
      <div class="lp-page-actions">
        <el-radio-group v-model="range" size="small">
          <el-radio-button v-for="option in rangeOptions" :key="option.value" :value="option.value">
            {{ option.label }}
          </el-radio-button>
        </el-radio-group>
      </div>
    </div>

    <!-- Waiting only shows before the first frame; after that the page is always live. -->
    <div v-if="!latest" class="dash-skeleton">
      <el-skeleton :rows="6" animated />
    </div>

    <template v-else>
      <section class="tile-grid">
        <StatTile
          :label="t('db.cpu')"
          :value="latest.cpu_percent.toFixed(1)"
          unit="%"
          :percent="latest.cpu_percent"
          :spark="cpuSpark"
          :sub="`${staticInfo?.logical_cores || 0} ${t('db.cores')} · ${t('db.load')} ${loadText}`"
          :badge="cpuBadge.text"
          :badge-level="cpuBadge.level"
        />
        <StatTile
          :label="t('db.memory')"
          :value="latest.mem_percent.toFixed(1)"
          unit="%"
          :percent="latest.mem_percent"
          :spark="memSpark"
          :sub="`${formatBytes(latest.mem_used)} / ${formatBytes(latest.mem_total)}`"
          :badge="swapBadge"
          badge-level="muted"
        />
        <StatTile
          :label="t('db.disk')"
          :value="diskOverall.percent.toFixed(1)"
          unit="%"
          :percent="diskOverall.percent"
          :sub="`${formatBytes(diskOverall.used)} / ${formatBytes(diskOverall.total)}`"
          :badge="`${disks.length} ${t('db.volumes')}`"
          badge-level="muted"
        />
        <StatTile
          :label="t('db.network')"
          :value="formatRateShort(latest.net_rx_rate + latest.net_tx_rate)"
          unit="/s"
          :spark="netSpark"
          tone="accent"
          :sub="`↓ ${formatRate(latest.net_rx_rate)}  ↑ ${formatRate(latest.net_tx_rate)}`"
          :badge="`${processCount} ${t('db.processes')}`"
          badge-level="muted"
        />
      </section>

      <section class="chart-grid">
        <article class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Cpu /></el-icon>{{ t('db.cpuTrend') }}</h2>
            <span class="chart-now lp-num">{{ latest.cpu_percent.toFixed(1) }}%</span>
          </header>
          <div class="lp-card-body">
            <MetricChart :series="cpuSeries" :height="188" percent />
          </div>
        </article>

        <article class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Coin /></el-icon>{{ t('db.memTrend') }}</h2>
            <span class="chart-now lp-num">{{ formatBytes(latest.mem_used) }}</span>
          </header>
          <div class="lp-card-body">
            <MetricChart :series="memSeries" :height="188" percent />
          </div>
        </article>

        <article class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Connection /></el-icon>{{ t('db.netTrend') }}</h2>
            <span class="chart-now lp-num">↓{{ formatRateShort(latest.net_rx_rate) }} ↑{{ formatRateShort(latest.net_tx_rate) }}</span>
          </header>
          <div class="lp-card-body">
            <MetricChart :series="netSeries" :height="188" :formatter="formatRateShort" show-legend />
          </div>
        </article>

        <article class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Files /></el-icon>{{ t('db.diskIoTrend') }}</h2>
            <span class="chart-now lp-num">R{{ formatRateShort(latest.disk_read_rate) }} W{{ formatRateShort(latest.disk_write_rate) }}</span>
          </header>
          <div class="lp-card-body">
            <MetricChart :series="diskIoSeries" :height="188" :formatter="formatRateShort" show-legend />
          </div>
        </article>
      </section>

      <section class="detail-grid">
        <article class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Cpu /></el-icon>{{ t('db.perCore') }}</h2>
            <span class="lp-dim">{{ t('db.avg') }} {{ latest.cpu_percent.toFixed(1) }}%</span>
          </header>
          <div class="lp-card-body">
            <div class="core-grid">
              <div v-for="(usage, index) in latest.cpu_cores" :key="index" class="core">
                <div class="core__track">
                  <div class="core__fill" :style="{ height: Math.max(2, usage) + '%', background: levelVar(usage) }"></div>
                </div>
                <span class="core__label lp-num">{{ index }}</span>
              </div>
            </div>
            <dl class="spec">
              <div><dt>{{ t('db.model') }}</dt><dd class="lp-truncate">{{ staticInfo?.cpu_model || '-' }}</dd></div>
              <div><dt>{{ t('db.physCores') }}</dt><dd>{{ staticInfo?.physical_cores }} / {{ staticInfo?.logical_cores }}</dd></div>
              <div><dt>{{ t('db.frequency') }}</dt><dd>{{ frequencyText }}</dd></div>
              <div><dt>{{ t('db.kernel') }}</dt><dd class="lp-truncate">{{ staticInfo?.kernel_version || '-' }}</dd></div>
            </dl>
          </div>
        </article>

        <article class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Files /></el-icon>{{ t('db.diskDetail') }}</h2>
          </header>
          <div class="lp-card-body lp-card-body--flush">
            <el-table :data="disks" size="small" :empty-text="t('db.noDisks')">
              <el-table-column :label="t('db.device')" min-width="130">
                <template #default="{ row }">
                  <div class="cell-stack">
                    <span class="lp-truncate">{{ row.name }}</span>
                    <span class="lp-dim lp-truncate">{{ row.mount_point }}</span>
                  </div>
                </template>
              </el-table-column>
              <el-table-column :label="t('db.fsType')" prop="fs_type" width="90" />
              <el-table-column :label="t('db.capacity')" width="150">
                <template #default="{ row }">
                  <span class="lp-num">{{ formatBytes(row.used) }} / {{ formatBytes(row.total) }}</span>
                </template>
              </el-table-column>
              <el-table-column :label="t('db.usage')" min-width="150">
                <template #default="{ row }">
                  <div class="usage">
                    <div class="usage__track">
                      <div class="usage__fill" :style="{ width: row.usage_percent + '%', background: levelVar(row.usage_percent) }"></div>
                    </div>
                    <span class="usage__pct lp-num">{{ row.usage_percent.toFixed(1) }}%</span>
                  </div>
                </template>
              </el-table-column>
            </el-table>
          </div>
        </article>

        <article class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Connection /></el-icon>{{ t('db.interfaces') }}</h2>
          </header>
          <div class="lp-card-body lp-card-body--flush">
            <el-table :data="activeInterfaces" size="small" :empty-text="t('db.noInterfaces')">
              <el-table-column :label="t('db.interface')" min-width="120">
                <template #default="{ row }">
                  <div class="cell-stack">
                    <span>{{ row.name }}</span>
                    <span class="lp-dim lp-truncate">{{ row.ip_addresses[0] || row.mac_address || '-' }}</span>
                  </div>
                </template>
              </el-table-column>
              <el-table-column :label="t('db.rxRate')" width="110" align="right">
                <template #default="{ row }"><span class="lp-num rx">↓ {{ formatRateShort(row.rx_rate) }}</span></template>
              </el-table-column>
              <el-table-column :label="t('db.txRate')" width="110" align="right">
                <template #default="{ row }"><span class="lp-num tx">↑ {{ formatRateShort(row.tx_rate) }}</span></template>
              </el-table-column>
              <el-table-column :label="t('db.total')" width="150" align="right">
                <template #default="{ row }">
                  <span class="lp-num lp-dim">{{ formatBytes(row.rx_bytes) }} / {{ formatBytes(row.tx_bytes) }}</span>
                </template>
              </el-table-column>
            </el-table>
          </div>
        </article>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { Coin, Connection, Cpu, Files } from '@element-plus/icons-vue'
import MetricChart, { type Series } from '@/components/MetricChart.vue'
import StatTile from '@/components/StatTile.vue'
import { useRealtimeStore } from '@/stores/realtime'
import { useI18n } from '@/i18n'
import { formatBytes, formatDuration, formatRate, formatRateShort } from '@/utils/format'

const { t } = useI18n()
const realtime = useRealtimeStore()
const { latest, staticInfo, detail, history } = storeToRefs(realtime)

/** Window of history to plot, in seconds. */
const range = ref(120)
const rangeOptions = [
  { value: 60, label: '1m' },
  { value: 120, label: '2m' },
  { value: 300, label: '5m' },
  { value: 600, label: '10m' },
]

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

const cpuSeries = computed<Series[]>(() => [
  { name: t('db.cpu'), color: cssVar('--lp-series-1'), data: seriesOf((s) => s.cpu_percent) },
])
const memSeries = computed<Series[]>(() => [
  { name: t('db.memory'), color: cssVar('--lp-series-2'), data: seriesOf((s) => s.mem_percent) },
])
const netSeries = computed<Series[]>(() => [
  { name: t('db.rx'), color: cssVar('--lp-series-1'), data: seriesOf((s) => s.net_rx_rate) },
  { name: t('db.tx'), color: cssVar('--lp-series-3'), data: seriesOf((s) => s.net_tx_rate) },
])
const diskIoSeries = computed<Series[]>(() => [
  { name: t('db.read'), color: cssVar('--lp-series-6'), data: seriesOf((s) => s.disk_read_rate) },
  { name: t('db.write'), color: cssVar('--lp-series-4'), data: seriesOf((s) => s.disk_write_rate) },
])

/** Sparklines use a short tail so the tile reflects the last ~40s, not the whole window. */
const cpuSpark = computed(() => history.value.slice(-40).map((s) => s.cpu_percent))
const memSpark = computed(() => history.value.slice(-40).map((s) => s.mem_percent))
const netSpark = computed(() => history.value.slice(-40).map((s) => s.net_rx_rate + s.net_tx_rate))

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
    .slice(0, 8),
)

const processCount = computed(() => detail.value?.process_count ?? 0)

const loadText = computed(() => {
  const load = latest.value?.load_avg || [0, 0, 0]
  return load.map((value) => value.toFixed(2)).join(' / ')
})

const cpuBadge = computed(() => {
  const cores = staticInfo.value?.logical_cores || 1
  const one = latest.value?.load_avg?.[0] ?? 0
  const ratio = one / cores
  if (ratio >= 1) return { text: t('db.saturated'), level: 'danger' as const }
  if (ratio >= 0.7) return { text: t('db.busy'), level: 'warn' as const }
  return { text: t('db.healthy'), level: 'ok' as const }
})

const swapBadge = computed(() => {
  const sample = latest.value
  if (!sample || !sample.swap_total) return ''
  return `Swap ${formatBytes(sample.swap_used)}`
})

const frequencyText = computed(() => {
  const mhz = staticInfo.value?.cpu_frequency_mhz || 0
  if (!mhz) return '-'
  return mhz >= 1000 ? `${(mhz / 1000).toFixed(2)} GHz` : `${mhz} MHz`
})

const subtitle = computed(() => {
  const info = staticInfo.value
  if (!info) return ''
  const uptime = latest.value ? formatDuration(latest.value.uptime) : '—'
  return `${info.os_version || info.os_name} · ${info.platform} · ${t('nav.uptime')} ${uptime}`
})

function levelVar(percent: number) {
  if (percent >= 88) return 'var(--lp-danger)'
  if (percent >= 70) return 'var(--lp-warn)'
  return 'var(--lp-accent)'
}
</script>

<style scoped lang="scss">
.dash-skeleton { padding: var(--lp-6); background: var(--lp-surface); border: 1px solid var(--lp-line); border-radius: var(--lp-r-lg); }

.tile-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
  gap: var(--lp-4);
}

.chart-grid,
.detail-grid {
  display: grid;
  gap: var(--lp-4);
  margin-top: var(--lp-4);
}
/* Four charts read best as a 2x2 block; auto-fit would leave a lone card on wide screens. */
.chart-grid { grid-template-columns: 1fr; }
@media (min-width: 1000px) {
  .chart-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
.detail-grid { grid-template-columns: repeat(auto-fit, minmax(380px, 1fr)); }

.chart-now {
  font-size: var(--lp-text-sm);
  font-weight: 600;
  color: var(--lp-text-soft);
}

/* Per-core bars */
.core-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(22px, 1fr));
  gap: 6px;
  margin-bottom: var(--lp-4);
}
.core { display: flex; flex-direction: column; align-items: center; gap: 4px; }
.core__track {
  position: relative;
  width: 100%;
  max-width: 26px;
  height: 56px;
  border-radius: var(--lp-r-sm);
  background: var(--lp-surface-sunken);
  overflow: hidden;
}
.core__fill {
  position: absolute;
  inset: auto 0 0 0;
  border-radius: inherit;
  transition: height 700ms cubic-bezier(0.4, 0, 0.2, 1), background var(--lp-base);
}
.core__label { font-size: 10px; color: var(--lp-text-faint); }

/* Spec list */
.spec {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: var(--lp-3);
  margin: 0;
  padding-top: var(--lp-4);
  border-top: 1px solid var(--lp-line-soft);

  div { min-width: 0; }
  dt { font-size: var(--lp-text-xs); color: var(--lp-text-dim); margin-bottom: 2px; }
  dd { margin: 0; font-size: var(--lp-text-base); color: var(--lp-text); font-weight: 500; }
}

.cell-stack { display: flex; flex-direction: column; line-height: 1.35; min-width: 0; }
.cell-stack .lp-dim { font-size: var(--lp-text-xs); }

.usage { display: flex; align-items: center; gap: var(--lp-2); }
.usage__track {
  flex: 1;
  height: 5px;
  min-width: 48px;
  border-radius: var(--lp-r-full);
  background: var(--lp-surface-sunken);
  overflow: hidden;
}
.usage__fill { height: 100%; border-radius: inherit; transition: width 600ms cubic-bezier(0.4, 0, 0.2, 1); }
.usage__pct { font-size: var(--lp-text-xs); color: var(--lp-text-soft); min-width: 42px; text-align: right; }

.rx { color: var(--lp-series-1); }
.tx { color: var(--lp-series-3); }
</style>
