<template>
  <div class="site-logs">
    <!-- Toolbar -->
    <div class="bar">
      <el-radio-group v-model="view" size="small">
        <el-radio-button value="report">{{ t('al.analysis') }}</el-radio-button>
        <el-radio-button value="raw">{{ t('al.raw') }}</el-radio-button>
      </el-radio-group>
      <el-radio-group v-if="view === 'report'" v-model="range" size="small" @change="load">
        <el-radio-button v-for="r in ranges" :key="r" :value="r">{{ t(`al.range.${r}`) }}</el-radio-button>
      </el-radio-group>
      <span class="bar__spacer" />
      <span v-if="usage" class="lp-dim bar__usage">
        {{ t('al.usage', { current: formatBytes(usage.current), archived: formatBytes(usage.archived), n: usage.archives }) }}
      </span>
      <el-button size="small" :icon="Refresh" :loading="loading" @click="view === 'report' ? load() : loadTail()" />
      <el-dropdown trigger="click" @command="onMore">
        <el-button size="small" :icon="MoreFilled" />
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="settings" :icon="Setting">{{ t('al.settings') }}</el-dropdown-item>
            <el-dropdown-item command="clear" :icon="Delete">{{ t('al.clear') }}</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>

    <!-- Analysis -->
    <template v-if="view === 'report'">
      <div v-if="!report && loading" class="skeleton">
        <el-skeleton :rows="8" animated />
      </div>
      <el-empty v-else-if="report && !report.logging" :description="t('al.noLogging')" :image-size="80" />
      <template v-else-if="report">
        <div class="kpis">
          <StatTile :label="t('al.requests')" :value="num(report.totals.requests)" :sub="t('al.perVisitor', { n: perVisitor })" />
          <StatTile :label="t('al.visitors')" :value="num(report.totals.visitors)" :sub="t('al.byIp')" />
          <StatTile :label="t('al.traffic')" :value="formatBytes(report.totals.bytes)" :sub="t('al.avgMs', { n: report.totals.avg_ms })" />
          <StatTile
            :label="t('al.errors')"
            :value="num(report.totals.status_4xx + report.totals.status_5xx)"
            :sub="`4xx ${num(report.totals.status_4xx)} · 5xx ${num(report.totals.status_5xx)}`"
            :badge="report.totals.status_5xx ? '5xx' : ''"
            badge-level="danger"
          />
          <StatTile :label="t('al.bots')" :value="pct(report.totals.bots)" :sub="t('al.botsSub', { n: num(report.totals.bots) })" />
          <StatTile
            :label="t('al.attacks')"
            :value="num(report.totals.attacks)"
            :sub="t('al.attacksSub')"
            :badge="report.totals.attacks ? t('al.review') : ''"
            badge-level="warn"
          />
        </div>
        <p v-if="report.partial" class="lp-dim note">{{ t('al.partial', { n: num(report.lines) }) }}</p>

        <section class="lp-card block">
          <header class="block__head">
            <h3>{{ t('al.timeline') }}</h3>
            <span class="lp-dim">{{ report.bucket === 'hour' ? t('al.perHour') : t('al.perDay') }}</span>
          </header>
          <MetricChart :series="timelineSeries" :height="220" show-legend :time-label="timeLabel" />
        </section>

        <div class="grid">
          <section class="lp-card block">
            <header class="block__head"><h3>{{ t('al.pages') }}</h3></header>
            <el-table :data="report.pages.slice(0, 15)" size="small" :empty-text="t('al.none')">
              <el-table-column :label="t('al.url')" min-width="200" show-overflow-tooltip>
                <template #default="{ row }"><span class="lp-mono">{{ row.key }}</span></template>
              </el-table-column>
              <el-table-column :label="t('al.count')" width="90" align="right">
                <template #default="{ row }">{{ num(row.count) }}</template>
              </el-table-column>
              <el-table-column :label="t('al.traffic')" width="90" align="right">
                <template #default="{ row }">{{ formatBytes(row.bytes) }}</template>
              </el-table-column>
            </el-table>
          </section>

          <section class="lp-card block">
            <header class="block__head"><h3>{{ t('al.ips') }}</h3></header>
            <el-table :data="report.ips.slice(0, 15)" size="small" :empty-text="t('al.none')">
              <el-table-column label="IP" min-width="150">
                <template #default="{ row }">
                  <span class="lp-mono">{{ row.ip }}</span>
                  <span v-if="row.attacks" class="lp-pill lp-pill--danger ip-tag">{{ t('al.suspicious') }}</span>
                </template>
              </el-table-column>
              <el-table-column :label="t('al.count')" width="80" align="right">
                <template #default="{ row }">{{ num(row.count) }}</template>
              </el-table-column>
              <el-table-column :label="t('al.errors')" width="70" align="right">
                <template #default="{ row }">{{ num(row.errors) }}</template>
              </el-table-column>
              <el-table-column :label="t('al.traffic')" width="86" align="right">
                <template #default="{ row }">{{ formatBytes(row.bytes) }}</template>
              </el-table-column>
              <el-table-column width="64" align="right">
                <template #default="{ row }">
                  <el-button link type="danger" size="small" @click="ban(row.ip)">{{ t('al.ban') }}</el-button>
                </template>
              </el-table-column>
            </el-table>
          </section>

          <section class="lp-card block">
            <header class="block__head"><h3>{{ t('al.status') }}</h3></header>
            <div class="bars">
              <div v-for="row in report.status" :key="row.key" class="bars__row">
                <span class="lp-mono bars__key" :class="`code-${row.key[0]}`">{{ row.key }}</span>
                <div class="bars__track"><div class="bars__fill" :class="`code-${row.key[0]}`" :style="{ width: share(row.count) + '%' }" /></div>
                <span class="bars__num">{{ num(row.count) }}</span>
              </div>
              <p v-if="!report.status.length" class="lp-dim">{{ t('al.none') }}</p>
            </div>
          </section>

          <section class="lp-card block">
            <header class="block__head"><h3>{{ t('al.referers') }}</h3></header>
            <div class="bars">
              <div v-for="row in report.referers.slice(0, 10)" :key="row.key" class="bars__row">
                <span class="bars__key bars__key--wide" :title="row.key">{{ row.key }}</span>
                <div class="bars__track"><div class="bars__fill" :style="{ width: relShare(row.count, report.referers) + '%' }" /></div>
                <span class="bars__num">{{ num(row.count) }}</span>
              </div>
              <p v-if="!report.referers.length" class="lp-dim">{{ t('al.noReferers') }}</p>
            </div>
          </section>

          <section class="lp-card block">
            <header class="block__head"><h3>{{ t('al.clients') }}</h3></header>
            <div class="split">
              <div class="bars">
                <div v-for="row in report.browsers.slice(0, 7)" :key="row.key" class="bars__row">
                  <span class="bars__key bars__key--wide">{{ row.key === 'Other' ? t('al.other') : row.key }}</span>
                  <div class="bars__track"><div class="bars__fill" :style="{ width: relShare(row.count, report.browsers) + '%' }" /></div>
                  <span class="bars__num">{{ pctOf(row.count, humans) }}</span>
                </div>
              </div>
              <div class="bars">
                <div v-for="row in report.systems.slice(0, 7)" :key="row.key" class="bars__row">
                  <span class="bars__key bars__key--wide">{{ row.key === 'Other' ? t('al.other') : row.key }}</span>
                  <div class="bars__track"><div class="bars__fill bars__fill--alt" :style="{ width: relShare(row.count, report.systems) + '%' }" /></div>
                  <span class="bars__num">{{ pctOf(row.count, humans) }}</span>
                </div>
              </div>
            </div>
            <p v-if="!report.browsers.length" class="lp-dim">{{ t('al.none') }}</p>
          </section>

          <section class="lp-card block">
            <header class="block__head"><h3>{{ t('al.spiders') }}</h3></header>
            <div class="bars">
              <div v-for="row in report.spiders.slice(0, 10)" :key="row.key" class="bars__row">
                <span class="bars__key bars__key--wide">{{ row.key === 'Other bots / scripts' ? t('al.otherBots') : row.key }}</span>
                <div class="bars__track"><div class="bars__fill bars__fill--muted" :style="{ width: relShare(row.count, report.spiders) + '%' }" /></div>
                <span class="bars__num">{{ num(row.count) }}</span>
              </div>
              <p v-if="!report.spiders.length" class="lp-dim">{{ t('al.none') }}</p>
            </div>
          </section>

          <section class="lp-card block">
            <header class="block__head"><h3>{{ t('al.notFound') }}</h3></header>
            <el-table :data="report.not_found.slice(0, 10)" size="small" :empty-text="t('al.none')">
              <el-table-column :label="t('al.url')" min-width="200" show-overflow-tooltip>
                <template #default="{ row }"><span class="lp-mono">{{ row.key }}</span></template>
              </el-table-column>
              <el-table-column :label="t('al.count')" width="90" align="right">
                <template #default="{ row }">{{ num(row.count) }}</template>
              </el-table-column>
            </el-table>
          </section>

          <section class="lp-card block">
            <header class="block__head"><h3>{{ t('al.serverErrors') }}</h3></header>
            <el-table :data="report.server_errors.slice(0, 10)" size="small" :empty-text="t('al.none')">
              <el-table-column :label="t('al.url')" min-width="200" show-overflow-tooltip>
                <template #default="{ row }"><span class="lp-mono">{{ row.key }}</span></template>
              </el-table-column>
              <el-table-column :label="t('al.count')" width="90" align="right">
                <template #default="{ row }">{{ num(row.count) }}</template>
              </el-table-column>
            </el-table>
          </section>
        </div>

        <section class="lp-card block">
          <header class="block__head">
            <h3>{{ t('al.slow') }}</h3>
            <span class="lp-dim">{{ t('al.slowHint') }}</span>
          </header>
          <el-table :data="report.slow" size="small" :empty-text="t('al.noSlow')">
            <el-table-column :label="t('al.time')" width="196" class-name="nowrap">
              <template #default="{ row }">{{ formatFullTime(row.time) }}</template>
            </el-table-column>
            <el-table-column :label="t('al.url')" min-width="260" show-overflow-tooltip>
              <template #default="{ row }"><span class="lp-mono">{{ row.method }} {{ row.url }}</span></template>
            </el-table-column>
            <el-table-column label="IP" width="150"><template #default="{ row }"><span class="lp-mono">{{ row.ip }}</span></template></el-table-column>
            <el-table-column :label="t('al.statusCol')" width="70" align="center" prop="status" />
            <el-table-column :label="t('al.duration')" width="90" align="right">
              <template #default="{ row }"><b>{{ (row.ms / 1000).toFixed(2) }}s</b></template>
            </el-table-column>
          </el-table>
        </section>

        <section class="lp-card block">
          <header class="block__head">
            <h3>{{ t('al.attackTitle') }}</h3>
            <span class="kinds">
              <span v-for="k in report.attack_kinds" :key="k.key" class="lp-pill lp-pill--warn">{{ t(`al.kind.${k.key}`) }} {{ num(k.count) }}</span>
            </span>
          </header>
          <el-table :data="report.attacks.slice(0, 50)" size="small" :empty-text="t('al.noAttacks')">
            <el-table-column :label="t('al.time')" width="196" class-name="nowrap">
              <template #default="{ row }">{{ formatFullTime(row.time) }}</template>
            </el-table-column>
            <el-table-column :label="t('al.kindCol')" width="124">
              <template #default="{ row }"><span class="lp-pill lp-pill--warn">{{ t(`al.kind.${row.kind}`) }}</span></template>
            </el-table-column>
            <el-table-column :label="t('al.url')" min-width="260" show-overflow-tooltip>
              <template #default="{ row }"><span class="lp-mono">{{ row.method }} {{ row.url }}</span></template>
            </el-table-column>
            <el-table-column label="IP" width="150"><template #default="{ row }"><span class="lp-mono">{{ row.ip }}</span></template></el-table-column>
            <el-table-column :label="t('al.statusCol')" width="70" align="center" prop="status" />
            <el-table-column width="64" align="right">
              <template #default="{ row }">
                <el-button link type="danger" size="small" @click="ban(row.ip)">{{ t('al.ban') }}</el-button>
              </template>
            </el-table-column>
          </el-table>
        </section>
      </template>
    </template>

    <!-- Raw log -->
    <template v-else>
      <div class="raw-bar">
        <el-input v-model="filter" size="small" clearable :placeholder="t('al.filter')" class="raw-bar__filter" @keyup.enter="loadTail" @clear="loadTail" />
        <el-select v-model="tailLines" size="small" class="raw-bar__lines" @change="loadTail">
          <el-option v-for="n in [100, 200, 500, 1000, 2000]" :key="n" :value="n" :label="t('al.lines', { n })" />
        </el-select>
        <span class="lp-dim">{{ t('al.newestFirst') }}</span>
      </div>
      <div v-loading="loading" class="raw">
        <div v-for="(line, i) in lines" :key="i" class="raw__line" :class="lineClass(line)">{{ line }}</div>
        <p v-if="!lines.length && !loading" class="lp-dim raw__empty">{{ t('al.noLines') }}</p>
      </div>
    </template>

    <el-dialog v-model="settingsVisible" :title="t('al.settings')" width="440px" append-to-body>
      <el-form label-position="top">
        <el-form-item :label="t('al.keepDays')">
          <el-input-number v-model="settingsForm.keep_days" :min="1" :max="365" />
          <div class="lp-dim form-hint">{{ t('al.keepDaysHint') }}</div>
        </el-form-item>
        <el-form-item :label="t('al.maxMb')">
          <el-input-number v-model="settingsForm.max_mb" :min="10" :max="10240" :step="50" />
          <div class="lp-dim form-hint">{{ t('al.maxMbHint') }}</div>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="settingsVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="saveSettings">{{ t('save') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Delete, MoreFilled, Refresh, Setting } from '@element-plus/icons-vue'
import {
  banDefenseIp, clearAccessLog, getAccessLogReport, getAccessLogSettings, getAccessLogTail, saveAccessLogSettings,
} from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes, formatFullTime, intlLocale } from '@/utils/format'
import MetricChart, { type Series } from '@/components/MetricChart.vue'
import StatTile from '@/components/StatTile.vue'

const props = defineProps<{ siteId: number }>()
const { t } = useI18n()

const ranges = ['today', 'yesterday', '7d', '30d', 'month']
const view = ref<'report' | 'raw'>('report')
const range = ref('today')
const loading = ref(false)
const report = ref<any>(null)
const usage = ref<any>(null)
const lines = ref<string[]>([])
const filter = ref('')
const tailLines = ref(200)
const settingsVisible = ref(false)
const settingsForm = ref({ keep_days: 30, max_mb: 200 })
const saving = ref(false)

const nf = computed(() => new Intl.NumberFormat(intlLocale()))
const num = (n: number) => nf.value.format(n || 0)
const pct = (n: number) => {
  const total = report.value?.totals.requests || 0
  return total ? `${((n / total) * 100).toFixed(1)}%` : '0%'
}
const humans = computed(() => (report.value ? report.value.totals.requests - report.value.totals.bots : 0))
const pctOf = (n: number, total: number) => (total ? `${((n / total) * 100).toFixed(1)}%` : '0%')
const perVisitor = computed(() => {
  const r = report.value?.totals
  return r && r.visitors ? (r.requests / r.visitors).toFixed(1) : '0'
})
const share = (n: number) => {
  const max = Math.max(1, ...(report.value?.status || []).map((r: any) => r.count))
  return Math.max(2, (n / max) * 100)
}
const relShare = (n: number, rows: any[]) => Math.max(2, (n / Math.max(1, rows[0]?.count || 1)) * 100)

function cssVar(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}
const timelineSeries = computed<Series[]>(() => {
  const points = report.value?.timeline || []
  return [
    { name: t('al.requests'), color: cssVar('--lp-series-1'), data: points.map((p: any) => [p.t * 1000, p.requests]) },
    { name: t('al.visitors'), color: cssVar('--lp-series-2'), data: points.map((p: any) => [p.t * 1000, p.visitors]) },
    { name: t('al.errors'), color: cssVar('--lp-danger'), data: points.map((p: any) => [p.t * 1000, p.errors]) },
  ]
})
function timeLabel(ms: number) {
  const d = new Date(ms)
  return report.value?.bucket === 'day'
    ? d.toLocaleDateString(intlLocale(), { month: 'short', day: 'numeric', weekday: 'short' })
    : d.toLocaleString(intlLocale(), { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

async function load() {
  loading.value = true
  try {
    const res: any = await getAccessLogReport(props.siteId, { range: range.value })
    if (res.success) {
      report.value = res.data
      usage.value = res.data.usage
    } else {
      ElMessage.error(res.message)
    }
  } catch {
    /* shown by the request layer */
  } finally {
    loading.value = false
  }
}

async function loadTail() {
  loading.value = true
  try {
    const res: any = await getAccessLogTail(props.siteId, { lines: tailLines.value, q: filter.value })
    lines.value = res.success ? res.data : []
  } catch {
    lines.value = []
  } finally {
    loading.value = false
  }
}

/** Colors a raw line by its status code. */
function lineClass(line: string) {
  const m = line.match(/" (\d{3}) /)
  if (!m) return ''
  return m[1] >= '500' ? 'is-5xx' : m[1] >= '400' ? 'is-4xx' : ''
}

async function ban(ip: string) {
  try {
    await ElMessageBox.confirm(t('al.banConfirm', { ip }), t('al.ban'), { type: 'warning' })
  } catch {
    return
  }
  const res: any = await banDefenseIp({ ip, reason: t('al.banReason') })
  if (res.success) ElMessage.success(t('al.banned', { ip }))
  else ElMessage.error(res.message)
}

async function onMore(command: string) {
  if (command === 'settings') {
    const res: any = await getAccessLogSettings()
    if (res.success) settingsForm.value = res.data
    settingsVisible.value = true
    return
  }
  let archives = false
  try {
    await ElMessageBox.confirm(t('al.clearConfirm'), t('al.clear'), {
      type: 'warning',
      distinguishCancelAndClose: true,
      confirmButtonText: t('al.clearCurrent'),
      cancelButtonText: t('al.clearAll'),
    })
  } catch (action) {
    if (action !== 'cancel') return
    archives = true
  }
  const res: any = await clearAccessLog(props.siteId, archives)
  if (res.success) {
    ElMessage.success(t('al.cleared'))
    view.value === 'report' ? load() : loadTail()
  } else {
    ElMessage.error(res.message)
  }
}

async function saveSettings() {
  saving.value = true
  try {
    const res: any = await saveAccessLogSettings(settingsForm.value)
    if (res.success) {
      ElMessage.success(t('al.saved'))
      settingsVisible.value = false
    } else {
      ElMessage.error(res.message)
    }
  } finally {
    saving.value = false
  }
}

watch(view, (v) => (v === 'report' ? (report.value ? null : load()) : loadTail()))
onMounted(load)
</script>

<style scoped lang="scss">
.site-logs { display: flex; flex-direction: column; gap: var(--lp-4); }
.bar { display: flex; align-items: center; gap: var(--lp-3); flex-wrap: wrap; }
.bar__spacer { flex: 1; }
.bar__usage { font-size: var(--lp-text-xs); }
.skeleton { padding: var(--lp-4) 0; }
.note { font-size: var(--lp-text-xs); margin: 0; }
.kpis { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: var(--lp-3); }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(440px, 100%), 1fr)); gap: var(--lp-4); }
.block { padding: var(--lp-4); min-width: 0; }
.block__head {
  display: flex; align-items: baseline; justify-content: space-between; gap: var(--lp-3); flex-wrap: wrap;
  margin-bottom: var(--lp-3);
  h3 { margin: 0; font-size: var(--lp-text-sm); font-weight: 600; }
  .lp-dim { font-size: var(--lp-text-xs); }
}
.kinds { display: flex; gap: 6px; flex-wrap: wrap; }
.ip-tag { margin-left: 6px; }
.bars { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
.bars__row { display: grid; grid-template-columns: auto 1fr auto; align-items: center; gap: var(--lp-3); font-size: var(--lp-text-sm); }
.bars__key { min-width: 36px; }
.bars__key--wide { width: 128px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bars__track { height: 8px; border-radius: 4px; background: var(--lp-surface-sunken, var(--lp-line-soft)); overflow: hidden; }
.bars__fill { height: 100%; border-radius: 4px; background: var(--lp-series-1); }
.bars__fill--alt { background: var(--lp-series-2); }
.bars__fill--muted { background: var(--lp-text-soft); opacity: 0.6; }
.bars__fill.code-2 { background: var(--lp-ok); }
.bars__fill.code-3 { background: var(--lp-series-1); }
.bars__fill.code-4 { background: var(--lp-warn); }
.bars__fill.code-5 { background: var(--lp-danger); }
.bars__key.code-4 { color: var(--lp-warn); }
.bars__key.code-5 { color: var(--lp-danger); }
.bars__num { font-variant-numeric: tabular-nums; color: var(--lp-text-soft); min-width: 48px; text-align: right; }
.split { display: grid; grid-template-columns: 1fr 1fr; gap: var(--lp-5); }
@media (max-width: 640px) { .split { grid-template-columns: 1fr; } }
.raw-bar { display: flex; align-items: center; gap: var(--lp-3); flex-wrap: wrap; }
.raw-bar__filter { width: 280px; max-width: 100%; }
.raw-bar__lines { width: 120px; }
.raw {
  min-height: 200px;
  max-height: 62vh;
  overflow: auto;
  padding: var(--lp-3);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  background: var(--lp-surface-sunken, var(--lp-surface));
  font-family: var(--lp-font-mono, ui-monospace, monospace);
  font-size: 12px;
  line-height: 1.7;
}
.raw__line { white-space: pre; }
.raw__line.is-4xx { color: var(--lp-warn); }
.raw__line.is-5xx { color: var(--lp-danger); }
.raw__empty { font-family: inherit; }
.site-logs :deep(.nowrap .cell) { white-space: nowrap; }
.form-hint { font-size: var(--lp-text-xs); margin-top: 4px; line-height: 1.5; width: 100%; }
</style>
