<template>
  <div class="sites-view">
    <div class="page-header">
      <h2>{{ t('si.title') }}</h2>
      <el-button type="primary" :icon="Plus" @click="showCreate = true">{{ t('si.addSite') }}</el-button>
    </div>

    <el-card shadow="hover">
      <el-table :data="sites" v-loading="loading" stripe style="width:100%">
        <el-table-column prop="name" :label="t('si.siteName')" min-width="140">
          <template #default="{ row }">
            <el-link type="primary" @click="$router.push(`/sites/${row.id}`)">{{ row.name }}</el-link>
          </template>
        </el-table-column>
        <el-table-column prop="domains" :label="t('si.domain')" min-width="200">
          <template #default="{ row }">
            <el-tag v-for="d in row.domains.split(',')" :key="d" size="small" style="margin:2px">{{ d }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="site_type" :label="t('si.siteType')" width="100">
          <template #default="{ row }">
            <el-tag :type="row.site_type === 'proxy' ? 'warning' : 'info'" size="small">{{ row.site_type }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="status" :label="t('si.status')" width="100">
          <template #default="{ row }">
            <el-tag :type="row.status === 'running' ? 'success' : 'danger'" size="small" effect="dark">
              {{ row.status === 'running' ? t('si.running') : t('si.stopped') }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="created_at" :label="t('si.createdAt')" width="180" />
        <el-table-column :label="t('si.actions')" width="220" fixed="right">
          <template #default="{ row }">
            <el-button-group size="small">
              <el-button v-if="row.status==='stopped'" type="success" @click="handleStart(row.id)">{{ t('si.start') }}</el-button>
              <el-button v-else type="warning" @click="handleStop(row.id)">{{ t('si.stop') }}</el-button>
              <el-button type="primary" @click="$router.push(`/sites/${row.id}`)">{{ t('si.config') }}</el-button>
              <el-button type="danger" @click="handleDelete(row)">{{ t('delete') }}</el-button>
            </el-button-group>
          </template>
        </el-table-column>
      </el-table>
    </el-card>

    <el-dialog v-model="showCreate" :title="t('si.addSite')" width="500px" v-drag :modal="false" :close-on-click-modal="false">
      <el-form :model="createForm" label-width="80px">
        <el-form-item :label="t('si.siteName')">
          <el-input v-model="createForm.name" />
        </el-form-item>
        <el-form-item :label="t('si.domain')">
          <el-input v-model="createForm.domains" placeholder="example.com" />
        </el-form-item>
        <el-form-item :label="t('si.port')">
          <el-input-number v-model="createForm.port" :min="1" :max="65535" />
        </el-form-item>
        <el-form-item :label="t('si.rootDir')">
          <el-input v-model="createForm.root_dir" placeholder="/www/sites" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="showCreate = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" @click="doCreate" :loading="creating">{{ t('create') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted } from 'vue'
import { useI18n } from '@/i18n'
import { getSites, createSite, deleteSite, startSite, stopSite } from '@/api'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus } from '@element-plus/icons-vue'

const { t } = useI18n()
const sites = ref<any[]>([])
const loading = ref(false)
const showCreate = ref(false)
const creating = ref(false)

const createForm = reactive({
  name: '',
  domains: '',
  port: 80,
  root_dir: '/www/sites',
  site_type: 'static',
})

async function loadSites() {
  loading.value = true
  try {
    const res: any = await getSites()
    if (res.success) sites.value = res.data || []
  } catch {}
  finally { loading.value = false }
}

async function doCreate() {
  creating.value = true
  try {
    const res: any = await createSite(createForm)
    if (res.success) {
      ElMessage.success(t('si.created'))
      showCreate.value = false
      loadSites()
    } else ElMessage.error(res.message)
  } finally { creating.value = false }
}

async function handleDelete(row: any) {
  await ElMessageBox.confirm(t('si.confirmDelete', { name: row.name }), t('confirm'), { type: 'warning' })
  const res: any = await deleteSite(row.id)
  if (res.success) { ElMessage.success(t('si.deleted')); loadSites() }
  else ElMessage.error(res.message)
}

async function handleStart(id: number) {
  const res: any = await startSite(id)
  if (res.success) loadSites()
  else ElMessage.error(res.message)
}

async function handleStop(id: number) {
  const res: any = await stopSite(id)
  if (res.success) loadSites()
  else ElMessage.error(res.message)
}

onMounted(() => loadSites())
</script>
<style scoped lang="scss">
.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
  h2 { font-size: 20px; font-weight: 600; }
}
</style>