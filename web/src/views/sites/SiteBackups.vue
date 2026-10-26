<template>
  <div class="site-backups">
    <div class="site-backups__bar">
      <el-button type="primary" size="small" :loading="busy === 'backup'" @click="backupNow">{{ t('sb.backupNow') }}</el-button>
      <el-button size="small" @click="router.push({ path: '/cron', query: { template: 'site', site: String(siteId) } })">
        {{ t('sb.schedule') }}
      </el-button>
      <span class="hint">{{ t('sb.hint') }}</span>
    </div>
    <el-table v-loading="loading" :data="backups" :empty-text="t('dbm.noBackups')" size="small">
      <el-table-column :label="t('dbm.file')" min-width="240">
        <template #default="{ row }"><span class="lp-mono">{{ row.file }}</span></template>
      </el-table-column>
      <el-table-column :label="t('dbm.size')" width="100">
        <template #default="{ row }">{{ formatBytes(row.size) }}</template>
      </el-table-column>
      <el-table-column :label="t('as.createdAt')" width="170" prop="created_at" />
      <el-table-column :label="t('actions')" width="170">
        <template #default="{ row }">
          <el-button link type="primary" size="small" @click="downloadFileApi(row.path, row.file)">{{ t('dbm.download') }}</el-button>
          <el-button link type="warning" size="small" :loading="busy === row.file" @click="restore(row)">{{ t('dbm.restore') }}</el-button>
          <el-button link type="danger" size="small" @click="remove(row)">{{ t('delete') }}</el-button>
        </template>
      </el-table-column>
    </el-table>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  backupSite, deleteSiteBackup, downloadFileApi, listSiteBackups, restoreSiteBackup,
} from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes } from '@/utils/format'

const props = defineProps<{ siteId: number }>()
const { t } = useI18n()
const router = useRouter()
const backups = ref<any[]>([])
const loading = ref(false)
const busy = ref('')

async function load() {
  loading.value = true
  try {
    const res: any = await listSiteBackups(props.siteId)
    backups.value = res.success ? res.data : []
  } catch {
    backups.value = []
  } finally {
    loading.value = false
  }
}

async function backupNow() {
  busy.value = 'backup'
  try {
    const res: any = await backupSite(props.siteId)
    if (res.success) ElMessage.success(t('dbm.backedUp'))
    await load()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

async function restore(row: any) {
  try {
    await ElMessageBox.confirm(t('sb.restoreConfirm', { file: row.file }), t('dbm.restore'), { type: 'warning' })
  } catch {
    return
  }
  busy.value = row.file
  try {
    const res: any = await restoreSiteBackup(props.siteId, row.file)
    if (res.success) ElMessage.success(t('dbm.restored'))
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

async function remove(row: any) {
  try {
    await ElMessageBox.confirm(t('dbm.deleteBackupConfirm', { file: row.file }), t('delete'), { type: 'warning' })
  } catch {
    return
  }
  const res: any = await deleteSiteBackup(props.siteId, row.file).catch(() => null)
  if (res?.success) await load()
}

onMounted(load)
</script>

<style scoped lang="scss">
.site-backups__bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--lp-2);
  margin-bottom: var(--lp-3);
}
.hint { font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
</style>
