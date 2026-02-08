<template>
  <div class="dashboard">
    <!-- Platform Overview Bar -->
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
        <span class="value">{{ info.platform?.load_avg?.map((v: number) => v.toFixed(2)).join(' / ') || '-' }}</span>
      </div>
    </div>

    <!-- Gauges Row -->
    <el-row :gutter="16" class="gauge-row">
      <el-col :xs="12" :sm="6" v-for="(g, i) in gauges" :key="i">
        <div class="gauge-card">
          <div class="gauge-ring">
            <svg viewBox="0 0 120 120">
              <circle cx="60" cy="60" r="52" fill="none" :stroke="'var(--lp-border-light)'" stroke-width="8" />
              <circle cx="60" cy="60" r="52" fill="none" :stroke="gaugeColor(g.percent)" stroke-width="8"
                stroke-linecap="round" :stroke-dasharray="circumference" :stroke-dashoffset="circumference - (g.percent / 100) * circumference"
                transform="rotate(-90 60 60)" style="transition: stroke-dashoffset 0.6s ease" />
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
      </el-col>
    </el-row>
    <!-- Detail Cards -->
    <el-row :gutter="16" style="margin-top:16px">
      <!-- CPU Detail -->
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
            <el-descriptions-item :label="t('db.frequency')">{{ info.cpu?.frequency_mhz }} MHz</el-descriptions-item>
            <el-descriptions-item :label="t('db.used')">{{ info.cpu?.usage_percent?.toFixed(1) }}%</el-descriptions-item>
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

      <!-- Memory Detail -->
      <el-col :xs="24" :lg="12">
        <el-card shadow="never" class="detail-card">
          <template #header>
            <div class="card-header"><el-icon><Coin /></el-icon> {{ t('db.memDetail') }}</div>
          </template>
          <el-descriptions :column="2" border size="small">
            <el-descriptions-item :label="t('db.total')">{{ fmtB(info.memory?.total) }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.used')">{{ fmtB(info.memory?.used) }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.free')">{{ fmtB(info.memory?.available) }}</el-descriptions-item>
            <el-descriptions-item :label="t('db.free')">{{ fmtB(info.memory?.free) }}</el-descriptions-item>
            <el-descriptions-item :label="'Swap ' + t('db.total')">{{ fmtB(info.memory?.swap_total) }}</el-descriptions-item>
            <el-descriptions-item :label="'Swap ' + t('db.used')">{{ fmtB(info.memory?.swap_used) }}</el-descriptions-item>
          </el-descriptions>
          <div class="mem-bar-wrap">
            <div class="mem-bar-label"><span>{{ t('db.memory') }}</span><span>{{ info.memory?.usage_percent?.toFixed(1) }}%</span></div>
            <div class="mem-bar-track">
              <div class="mem-bar-fill" :style="{ width: (info.memory?.usage_percent || 0) + '%', background: gaugeColor(info.memory?.usage_percent || 0) }"></div>
            </div>
            <div class="mem-bar-label" v-if="info.memory?.swap_total > 0">
              <span>Swap</span><span>{{ swapPercent.toFixed(1) }}%</span>
            </div>
            <div class="mem-bar-track" v-if="info.memory?.swap_total > 0">
              <div class="mem-bar-fill" :style="{ width: swapPercent + '%', background: gaugeColor(swapPercent) }"></div>
            </div>
          </div>
        </el-card>
      </el-col>
    </el-row>
    <!-- Disks & Network -->
    <el-row :gutter="16" style="margin-top:16px">
      <el-col :xs="24" :lg="14">
        <el-card shadow="never" class="detail-card">
          <template #header>
            <div class="card-header"><el-icon><Files /></el-icon> {{ t('db.diskDetail') }}</div>
          </template>
          <el-table :data="info.disks || []" size="small" stripe>
            <el-table-column prop="name" :label="t('db.interface')" min-width="80" />
            <el-table-column prop="mount_point" :label="t('db.mountPoint')" min-width="100" />
            <el-table-column prop="fs_type" :label="t('db.filesystem')" width="90" />
            <el-table-column :label="t('db.total')" width="100">
              <template #default="{ row }">{{ fmtB(row.total) }}</template>
            </el-table-column>
            <el-table-column :label="t('db.used')" width="100">
              <template #default="{ row }">{{ fmtB(row.used) }}</template>
            </el-table-column>
            <el-table-column :label="t('db.used')" width="160">
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
          <el-table :data="(info.networks || []).filter((n: any) => n.rx_bytes > 0 || n.tx_bytes > 0)" size="small" stripe>
            <el-table-column prop="name" :label="t('db.interface')" min-width="100" />
            <el-table-column :label="t('db.received')" width="100">
              <template #default="{ row }">{{ fmtB(row.rx_bytes) }}</template>
            </el-table-column>
            <el-table-column :label="t('db.sent')" width="100">
              <template #default="{ row }">{{ fmtB(row.tx_bytes) }}</template>
            </el-table-column>
            <el-table-column prop="mac_address" label="MAC" min-width="140" />
          </el-table>
        </el-card>
      </el-col>
    </el-row>
  </div>
</template>
<script setup lang="ts">
import { ref, computed as _computed, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from '@/i18n'
import { getSystemInfo } from '@/api'

const { t } = useI18n()

const info = ref<any>({})
const circumference = 2 * Math.PI * 52

const gauges = computed(() => {
  const cpu = info.value.cpu?.usage_percent || 0
  const mem = info.value.memory?.usage_percent || 0
  const disk = info.value.disks?.length
    ? info.value.disks.reduce((a: number, d: any) => a + d.usage_percent, 0) / info.value.disks.length
    : 0
  const swap = info.value.memory?.swap_total
    ? (info.value.memory.swap_used / info.value.memory.swap_total) * 100
    : 0
  return [
    { title: 'CPU', percent: cpu, sub: `${info.value.cpu?.logical_cores || 0} cores` },
    { title: t('db.memory'), percent: mem, sub: `${fmtB(info.value.memory?.used)} / ${fmtB(info.value.memory?.total)}` },
    { title: t('db.disk'), percent: disk, sub: `${info.value.disks?.length || 0} disks` },
    { title: 'Swap', percent: swap, sub: `${fmtB(info.value.memory?.swap_used)} / ${fmtB(info.value.memory?.swap_total)}` },
  ]
})

const swapPercent = computed(() => {
  const m = info.value.memory
  return m?.swap_total ? (m.swap_used / m.swap_total) * 100 : 0
})

function gaugeColor(pct: number): string {
  if (pct < 50) return 'var(--lp-gauge-low)'
  if (pct < 80) return 'var(--lp-gauge-mid)'
  return 'var(--lp-gauge-high)'
}

function fmtB(b: number): string {
  if (!b) return '0 B'
  const u = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(b) / Math.log(1024))
  return (b / Math.pow(1024, i)).toFixed(1) + ' ' + u[i]
}

let timer: any
async function fetchInfo() {
  try {
    const res: any = await getSystemInfo()
    if (res.success) info.value = res.data
  } catch {}
}
onMounted(() => { fetchInfo(); timer = setInterval(fetchInfo, 5000) })
onUnmounted(() => clearInterval(timer))
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
  transition: box-shadow var(--lp-transition);
  &:hover { box-shadow: var(--lp-shadow); }
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
  transition: height 0.6s ease, background 0.3s;
  min-height: 2px;
}
.core-label { font-size: 9px; color: var(--lp-text-placeholder); margin-top: 3px; }

.mem-bar-wrap { margin-top: 16px; padding-top: 12px; border-top: 1px solid var(--lp-border-light); }
.mem-bar-label { display: flex; justify-content: space-between; font-size: 12px; color: var(--lp-text-secondary); margin-bottom: 4px; margin-top: 8px; }
.mem-bar-track { height: 10px; background: var(--lp-border-light); border-radius: 5px; overflow: hidden; }
.mem-bar-fill { height: 100%; border-radius: 5px; transition: width 0.6s ease, background 0.3s; }
</style>