<template>
  <div class="site-detail" v-loading="loading">
    <div class="page-header">
      <div class="page-title">
        <el-button :icon="ArrowLeft" text @click="$router.push('/sites')" />
        <h2>{{ site.name }}</h2>
        <el-tag :type="site.status === 'running' ? 'success' : 'danger'" size="small" effect="dark">
          {{ site.status === 'running' ? 'Running' : 'Stopped' }}
        </el-tag>
      </div>
      <div>
        <el-button v-if="site.status==='stopped'" type="success" @click="handleStart">Start</el-button>
        <el-button v-else type="warning" @click="handleStop">Stop</el-button>
      </div>
    </div>

    <el-tabs v-model="activeTab" type="border-card">
      <el-tab-pane label="基本信息" name="info">
        <el-form :model="editForm" label-width="100px" style="max-width:600px;margin-top:16px">
          <el-form-item label="Site Name">
            <el-input v-model="editForm.name" />
          </el-form-item>
          <el-form-item label="Type">
            <el-radio-group v-model="editForm.site_type">
              <el-radio value="static">静态</el-radio>
              <el-radio value="proxy">反向代理</el-radio>
              <el-radio value="php">PHP</el-radio>
            </el-radio-group>
          </el-form-item>
          <el-form-item label="Root Dir">
            <el-input v-model="editForm.doc_root" />
          </el-form-item>
          <el-form-item v-if="editForm.site_type==='proxy'" label="代理地址">
            <el-input v-model="editForm.proxy_pass" />
          </el-form-item>
          <el-form-item>
            <el-button type="primary" @click="handleSave">保存</el-button>
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="Domain Management" name="domains">
        <div style="margin-bottom:16px">
          <el-input v-model="newDomain" placeholder="输入Domain" style="width:300px;margin-right:8px" @keyup.enter="handleAddDomain" />
          <el-button type="primary" @click="handleAddDomain">添加</el-button>
        </div>
        <el-table :data="domains" stripe>
          <el-table-column prop="domain" label="Domain" />
          <el-table-column prop="is_primary" label="主Domain" width="100">
            <template #default="{ row }">
              <el-tag v-if="row.is_primary" type="success" size="small">主</el-tag>
            </template>
          </el-table-column>
          <el-table-column prop="created_at" label="添加时间" width="180" />
          <el-table-column label="Actions" width="120">
            <template #default="{ row }">
              <el-button type="danger" size="small" text @click="handleRemoveDomain(row.id)">Delete</el-button>
            </template>
          </el-table-column>
        </el-table>
      </el-tab-pane>

      <el-tab-pane label="配置文件" name="config">
        <el-input v-model="editForm.extra_config" type="textarea" :rows="16" placeholder="自定义 Nginx 配置..." />
        <el-button type="primary" style="margin-top:12px" @click="handleSave">Save Config</el-button>
      </el-tab-pane>
    </el-tabs>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { getSite, updateSite, startSite, stopSite, getDomains, addDomain, removeDomain } from '@/api'
import { ElMessage } from 'element-plus'
import { ArrowLeft } from '@element-plus/icons-vue'

const route = useRoute()
const siteId = Number(route.params.id)
const loading = ref(false)
const activeTab = ref('info')
const site = ref<any>({})
const domains = ref<any[]>([])
const newDomain = ref('')

const editForm = reactive({
  name: '',
  site_type: 'static',
  doc_root: '',
  proxy_pass: '',
  extra_config: '',
})

onMounted(async () => {
  loading.value = true
  try {
    const [siteRes, domainRes]: any[] = await Promise.all([getSite(siteId), getDomains(siteId)])
    if (siteRes.success) {
      site.value = siteRes.data
      Object.assign(editForm, {
        name: siteRes.data.name,
        site_type: siteRes.data.site_type,
        doc_root: siteRes.data.doc_root,
        proxy_pass: siteRes.data.proxy_pass || '',
        extra_config: siteRes.data.extra_config || '',
      })
    }
    if (domainRes.success) domains.value = domainRes.data
  } finally {
    loading.value = false
  }
})

async function handleSave() {
  const res: any = await updateSite(siteId, editForm)
  if (res.success) {
    site.value = res.data
    ElMessage.success('保存成功')
  }
}

async function handleStart() {
  const res: any = await startSite(siteId)
  if (res.success) { site.value.status = 'running'; ElMessage.success('已Start') }
}

async function handleStop() {
  const res: any = await stopSite(siteId)
  if (res.success) { site.value.status = 'stopped'; ElMessage.success('Stopped') }
}

async function handleAddDomain() {
  if (!newDomain.value) return
  const res: any = await addDomain(siteId, { domain: newDomain.value })
  if (res.success) {
    domains.value.push(res.data)
    newDomain.value = ''
    ElMessage.success('添加成功')
  }
}

async function handleRemoveDomain(id: number) {
  const res: any = await removeDomain(siteId, id)
  if (res.success) {
    domains.value = domains.value.filter((d: any) => d.id !== id)
    ElMessage.success('Delete成功')
  }
}
</script>

<style scoped lang="scss">
.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}
.page-title {
  display: flex;
  align-items: center;
  gap: 8px;
  h2 { font-size: 20px; font-weight: 600; }
}
</style>