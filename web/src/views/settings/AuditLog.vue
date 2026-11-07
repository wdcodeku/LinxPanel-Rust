<template>
  <section class="lp-card">
    <header class="lp-card-head">
      <h2 class="lp-card-title"><el-icon><Tickets /></el-icon>{{ t('au.title') }}</h2>
      <div class="au-tools">
        <el-input v-model="q" size="small" clearable :prefix-icon="Search" :placeholder="t('au.search')" class="au-search" @keyup.enter="reload" @clear="reload" />
        <el-checkbox v-model="failedOnly" size="small" @change="reload">{{ t('au.failedOnly') }}</el-checkbox>
        <el-button size="small" :icon="Refresh" :loading="loading" @click="load" />
      </div>
    </header>
    <el-table :data="items" size="small" v-loading="loading" :empty-text="t('au.empty')">
      <el-table-column :label="t('au.time')" width="150">
        <template #default="{ row }"><span :title="formatFullTime(epoch(row.created_at))">{{ formatShortTime(epoch(row.created_at)) }}</span></template>
      </el-table-column>
      <el-table-column :label="t('au.user')" width="140">
        <template #default="{ row }">
          <div class="strong">{{ row.username || '—' }}</div>
          <div class="lp-dim lp-mono au-ip">{{ row.ip }}</div>
        </template>
      </el-table-column>
      <el-table-column :label="t('au.action')" min-width="200">
        <template #default="{ row }">
          <div>{{ actionName(row.method, row.path) }}</div>
          <div class="lp-dim lp-mono au-path">{{ row.method }} {{ row.path }}</div>
        </template>
      </el-table-column>
      <el-table-column :label="t('au.target')" min-width="260">
        <template #default="{ row }"><span class="lp-mono au-target">{{ row.target || '—' }}</span></template>
      </el-table-column>
      <el-table-column :label="t('au.result')" width="150">
        <template #default="{ row }">
          <span class="lp-pill" :class="row.ok ? 'lp-pill--ok' : 'lp-pill--danger'" :title="row.message">{{ row.ok ? t('au.ok') : t('au.failed') }}</span>
          <div v-if="!row.ok && row.message" class="lp-dim au-msg lp-truncate" :title="row.message">{{ row.message }}</div>
        </template>
      </el-table-column>
    </el-table>
    <div class="au-pager">
      <span class="lp-dim">{{ t('au.total', { n: total }) }}</span>
      <el-pagination v-model:current-page="page" :page-size="pageSize" :total="total" layout="prev, pager, next" small @current-change="load" />
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Refresh, Search, Tickets } from '@element-plus/icons-vue'
import { getAuditLog } from '@/api'
import { useI18n } from '@/i18n'
import { formatFullTime, formatShortTime } from '@/utils/format'

const { t } = useI18n()
const items = ref<any[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = 50
const q = ref('')
const failedOnly = ref(false)
const loading = ref(false)

/** Readable names for the most common operations; the path shows for the rest. */
const ACTIONS: [RegExp, string][] = [
  [/^\/auth\/login$/, 'au.a.login'],
  [/^\/files\/delete$/, 'au.a.delete'],
  [/^\/files\/write$/, 'au.a.write'],
  [/^\/files\/(transfer|copy|move)/, 'au.a.transfer'],
  [/^\/files\/rename$/, 'au.a.rename'],
  [/^\/files\/mkdir$/, 'au.a.mkdir'],
  [/^\/files\/(chmod|chown)$/, 'au.a.permissions'],
  [/^\/files\/(extract|compress)$/, 'au.a.archive'],
  [/^\/files\/upload/, 'au.a.upload'],
  [/^\/files\/download-link$/, 'au.a.download'],
  [/^\/files\/exec/, 'au.a.exec'],
  [/^\/files\/recycle/, 'au.a.recycle'],
  [/^\/sites/, 'au.a.sites'],
  [/^\/certs?/, 'au.a.certs'],
  [/^\/(databases?|mysql|postgres|redis|mongo|sqlite)/, 'au.a.databases'],
  [/^\/apps/, 'au.a.apps'],
  [/^\/runtimes/, 'au.a.runtimes'],
  [/^\/appstore/, 'au.a.appstore'],
  [/^\/cron/, 'au.a.cron'],
  [/^\/firewall/, 'au.a.firewall'],
  [/^\/(defense|ipban)/, 'au.a.defense'],
  [/^\/(settings|panel|account|notify)/, 'au.a.settings'],
  [/^\/update/, 'au.a.update'],
  [/^\/(services|processes)/, 'au.a.services'],
  [/^\/backups?/, 'au.a.backups'],
]
function actionName(method: string, path: string) {
  const hit = ACTIONS.find(([re]) => re.test(path))
  const base = hit ? t(hit[1]) : path
  return method === 'DELETE' && hit && !path.includes('delete') ? `${base} · ${t('delete')}` : base
}
function epoch(text: string) {
  const ms = Date.parse(`${text.replace(' ', 'T')}Z`)
  return Number.isNaN(ms) ? 0 : Math.floor(ms / 1000)
}

async function load() {
  loading.value = true
  try {
    const res: any = await getAuditLog({ q: q.value.trim(), failed_only: failedOnly.value, offset: (page.value - 1) * pageSize, limit: pageSize })
    if (res.success) {
      items.value = res.data.items
      total.value = res.data.total
    }
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}
function reload() {
  page.value = 1
  void load()
}
onMounted(load)
defineExpose({ load })
</script>

<style scoped lang="scss">
.au-tools { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.au-search { width: 240px; }
.au-ip, .au-path { font-size: 11px; }
.au-target { font-size: 12px; word-break: break-all; }
.au-msg { font-size: 11px; max-width: 140px; }
.au-pager { display: flex; justify-content: space-between; align-items: center; padding: 10px 16px; flex-wrap: wrap; gap: 8px; }
.strong { font-weight: 600; }
@media (max-width: 760px) { .au-search { width: 100%; } }
</style>
