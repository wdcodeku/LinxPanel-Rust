<template>
  <el-drawer
    :model-value="modelValue"
    :title="t('rb.title')"
    size="min(880px, 96vw)"
    append-to-body
    @update:model-value="(v: boolean) => emit('update:modelValue', v)"
  >
    <div class="rb">
      <div class="rb-bar">
        <el-switch v-model="settings.enabled" :active-text="t('rb.enabled')" @change="saveSettings" />
        <span class="rb-keep">
          {{ t('rb.keep') }}
          <el-input-number v-model="settings.keep_days" :min="0" :max="3650" size="small" controls-position="right" class="rb-days" @change="saveSettings" />
          {{ t('rb.days') }}
        </span>
        <span class="rb-spacer"></span>
        <el-button size="small" :icon="Refresh" :loading="loading" @click="load" />
        <el-button size="small" :disabled="!selected.length" @click="restore(selected)">{{ t('rb.restoreN', { n: selected.length }) }}</el-button>
        <el-button size="small" type="danger" plain :disabled="!selected.length" @click="purge(selected)">{{ t('rb.purgeN', { n: selected.length }) }}</el-button>
        <el-button size="small" type="danger" :disabled="!items.length" @click="emptyBin">{{ t('rb.empty') }}</el-button>
      </div>
      <p class="rb-hint">{{ settings.keep_days ? t('rb.hintDays', { n: settings.keep_days }) : t('rb.hintForever') }}</p>

      <el-input v-model="keyword" size="small" clearable :prefix-icon="Search" :placeholder="t('rb.search')" class="rb-search" />
      <el-table
        :data="filtered"
        size="small"
        height="calc(100vh - 260px)"
        v-loading="loading"
        :empty-text="t('rb.emptyText')"
        @selection-change="(rows: Item[]) => (selected = rows)"
      >
        <el-table-column type="selection" width="38" />
        <el-table-column :label="t('fm.name')" min-width="200">
          <template #default="{ row }">
            <span class="rb-name">
              <el-icon :color="row.is_dir ? 'var(--lp-warning)' : 'var(--lp-text-dim)'"><Folder v-if="row.is_dir" /><Document v-else /></el-icon>
              <span class="lp-truncate" :title="row.name">{{ row.name }}</span>
            </span>
          </template>
        </el-table-column>
        <el-table-column :label="t('rb.from')" min-width="220">
          <template #default="{ row }"><span class="lp-mono rb-path lp-truncate" :title="row.original">{{ parentOf(row.original) }}</span></template>
        </el-table-column>
        <el-table-column :label="t('fm.size')" width="96" align="right">
          <template #default="{ row }">{{ row.size === null ? '—' : formatBytes(row.size) }}</template>
        </el-table-column>
        <el-table-column :label="t('rb.deletedAt')" width="150">
          <template #default="{ row }"><span :title="formatFullTime(epoch(row.deleted_at))">{{ formatShortTime(epoch(row.deleted_at)) }}</span></template>
        </el-table-column>
        <el-table-column :label="t('actions')" width="130" fixed="right">
          <template #default="{ row }">
            <el-button link type="primary" size="small" @click="restore([row])">{{ t('rb.restore') }}</el-button>
            <el-button link type="danger" size="small" @click="purge([row])">{{ t('delete') }}</el-button>
          </template>
        </el-table-column>
      </el-table>
      <p class="rb-total">{{ t('rb.total', { n: items.length, size: formatBytes(totalSize) }) }}</p>
    </div>
  </el-drawer>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Document, Folder, Refresh, Search } from '@element-plus/icons-vue'
import { emptyRecycleBin, getRecycleBin, purgeRecycled, restoreRecycled, saveRecycleSettings } from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes, formatFullTime, formatShortTime } from '@/utils/format'

type Item = { id: number; name: string; original: string; stored: string; is_dir: boolean; size: number | null; deleted_at: string }

const props = defineProps<{ modelValue: boolean }>()
const emit = defineEmits<{ (e: 'update:modelValue', v: boolean): void; (e: 'restored', paths: string[]): void }>()
const { t } = useI18n()

const items = ref<Item[]>([])
const selected = ref<Item[]>([])
const loading = ref(false)
const keyword = ref('')
const settings = reactive({ enabled: true, keep_days: 30 })

const filtered = computed(() => {
  const q = keyword.value.trim().toLowerCase()
  return q ? items.value.filter((i) => i.name.toLowerCase().includes(q) || i.original.toLowerCase().includes(q)) : items.value
})
const totalSize = computed(() => items.value.reduce((sum, i) => sum + (i.size || 0), 0))

/** SQLite datetime('now') is UTC "YYYY-MM-DD HH:MM:SS". */
function epoch(text: string) {
  const ms = Date.parse(`${text.replace(' ', 'T')}Z`)
  return Number.isNaN(ms) ? 0 : Math.floor(ms / 1000)
}
function parentOf(path: string) {
  return path.slice(0, path.lastIndexOf('/')) || '/'
}

async function load() {
  loading.value = true
  try {
    const res: any = await getRecycleBin()
    if (res.success) {
      items.value = res.data.items
      Object.assign(settings, res.data.settings)
    }
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}

async function saveSettings() {
  const res: any = await saveRecycleSettings({ enabled: settings.enabled, keep_days: Number(settings.keep_days) || 0 }).catch(() => null)
  if (res?.success) ElMessage.success(t('rb.settingsSaved'))
}

async function restore(rows: Item[]) {
  if (!rows.length) return
  const res: any = await restoreRecycled(rows.map((r) => r.id)).catch(() => null)
  if (res?.success) {
    const paths: string[] = res.data || []
    const renamed = paths.filter((p, i) => p !== rows[i]?.original)
    ElMessage.success(renamed.length ? t('rb.restoredRenamed', { n: paths.length, m: renamed.length }) : t('rb.restoredN', { n: paths.length }))
    emit('restored', paths)
  }
  await load()
}

async function purge(rows: Item[]) {
  if (!rows.length) return
  try {
    await ElMessageBox.confirm(t('rb.purgeConfirm', { n: rows.length }), t('delete'), {
      type: 'warning', confirmButtonText: t('delete'), confirmButtonClass: 'el-button--danger',
    })
  } catch {
    return
  }
  const res: any = await purgeRecycled(rows.map((r) => r.id)).catch(() => null)
  if (res?.success) ElMessage.success(t('rb.purgedN', { n: res.data }))
  await load()
}

async function emptyBin() {
  try {
    await ElMessageBox.confirm(t('rb.emptyConfirm', { n: items.value.length }), t('rb.empty'), {
      type: 'warning', confirmButtonText: t('rb.empty'), confirmButtonClass: 'el-button--danger',
    })
  } catch {
    return
  }
  const res: any = await emptyRecycleBin().catch(() => null)
  if (res?.success) ElMessage.success(t('rb.purgedN', { n: res.data }))
  await load()
}

// Load on every open, including when created already open.
watch(() => props.modelValue, (open) => { if (open) void load() }, { immediate: true })

defineExpose({ load })
</script>

<style scoped lang="scss">
.rb { display: flex; flex-direction: column; gap: 8px; height: 100%; }
.rb-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.rb-keep { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--lp-text-dim); }
.rb-days { width: 96px; }
.rb-spacer { flex: 1; }
.rb-hint { margin: 0; font-size: 12px; color: var(--lp-text-dim); }
.rb-search { width: 260px; }
.rb-name { display: inline-flex; align-items: center; gap: 6px; min-width: 0; max-width: 100%; }
.rb-path { font-size: 12px; color: var(--lp-text-dim); display: block; }
.rb-total { margin: 0; font-size: 12px; color: var(--lp-text-dim); }
</style>
