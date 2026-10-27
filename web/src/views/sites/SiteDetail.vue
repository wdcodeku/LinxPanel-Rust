<template>
  <div class="site-detail" v-loading="loading">
    <el-card v-if="!canManageSites" shadow="hover" class="guard-card">
      <div class="guard-title">
        <el-icon><WarningFilled /></el-icon>
        <span>{{ t('si.openrestyRequiredTitle') }}</span>
      </div>
      <div class="guard-desc">{{ openrestyStatus.message || t('si.openrestyRequiredDesc') }}</div>
      <div class="guard-actions">
        <el-button
          type="primary"
          :disabled="!openrestyStatus.supported"
          :loading="installingOpenresty"
          @click="handleQuickInstallOpenResty"
        >
          {{ t('si.quickInstallOpenresty') }}
        </el-button>
        <el-button :disabled="!installLog" @click="installLogDialogVisible = true">
          {{ t('st.viewInstallLog') }}
        </el-button>
        <el-button @click="router.push({ path: '/sites', query: { tab: 'openresty' } })">{{ t('si.openrestyGoSettings') }}</el-button>
        <el-button @click="router.push('/sites')">{{ t('sites') }}</el-button>
      </div>
    </el-card>
    <template v-else>
    <div class="detail-header">
      <div class="title-row">
        <el-button :icon="ArrowLeft" text class="back-btn" @click="$router.push('/sites')" />
        <h2 class="site-name">{{ site.name || '-' }}</h2>
        <el-tag :type="site.status === 'running' ? 'success' : 'danger'" size="small" effect="light">
          {{ site.status === 'running' ? t('si.running') : t('si.stopped') }}
        </el-tag>
      </div>
      <div class="header-actions">
        <el-button v-if="site.status==='stopped'" type="success" @click="handleStart">{{ t('si.start') }}</el-button>
        <el-button v-else type="warning" @click="handleStop">{{ t('si.stop') }}</el-button>
      </div>
    </div>

    <div class="section-nav">
      <button
        type="button"
        class="section-item"
        :class="{ active: activeTab === 'info' }"
        @click="activeTab = 'info'"
      >
        {{ t('si.detailInfoTab') }}
      </button>
      <button
        type="button"
        class="section-item"
        :class="{ active: activeTab === 'domains' }"
        @click="activeTab = 'domains'"
      >
        {{ t('si.detailDomainTab') }}
      </button>
      <button
        type="button"
        class="section-item"
        :class="{ active: activeTab === 'config' }"
        @click="activeTab = 'config'"
      >
        {{ t('si.detailConfigTab') }}
      </button>
      <button
        type="button"
        class="section-item"
        :class="{ active: activeTab === 'backup' }"
        @click="activeTab = 'backup'"
      >
        {{ t('sb.tab') }}
      </button>
    </div>

    <el-card v-if="activeTab === 'info'" shadow="hover" class="detail-card">
      <el-form :model="editForm" label-width="100px" class="info-form">
        <el-form-item :label="t('si.siteName')">
          <el-input v-model="editForm.name" />
        </el-form-item>
        <el-form-item :label="t('si.siteType')">
          <el-radio-group v-model="editForm.site_type">
            <el-radio value="static">{{ t('si.typeStatic') }}</el-radio>
            <el-radio value="proxy">{{ t('si.typeProxy') }}</el-radio>
            <el-radio value="php">{{ t('si.typePhp') }}</el-radio>
          </el-radio-group>
        </el-form-item>
        <el-form-item :label="t('si.rootDir')">
          <el-input v-model="editForm.doc_root" />
        </el-form-item>
        <el-form-item :label="t('si.defaultIndexFiles')">
          <el-input
            v-model="editForm.index_files"
            :placeholder="t('si.defaultIndexFilesPlaceholder')"
          />
          <div class="form-tip">{{ t('si.defaultIndexFilesTip') }}</div>
        </el-form-item>
        <el-form-item v-if="editForm.site_type==='proxy'" :label="t('si.proxyAddress')">
          <el-input v-model="editForm.proxy_pass" />
        </el-form-item>
        <el-form-item :label="t('si.stoppedPageHtml')">
          <el-input
            v-model="editForm.stopped_page_html"
            type="textarea"
            :rows="6"
            :placeholder="t('si.stoppedPageHtmlPlaceholder')"
          />
          <div class="form-tip">{{ t('si.stoppedPageHtmlTip') }}</div>
        </el-form-item>
        <el-form-item>
          <el-button type="primary" @click="handleSave">{{ t('save') }}</el-button>
        </el-form-item>
      </el-form>
    </el-card>

    <el-card v-else-if="activeTab === 'domains'" shadow="hover" class="detail-card">
      <div class="domain-toolbar">
        <el-input
          v-model="newDomain"
          :placeholder="t('si.domainPlaceholderWithPort')"
          class="domain-input"
          @keyup.enter="handleAddDomain"
        />
        <el-button type="primary" @click="handleAddDomain">{{ t('create') }}</el-button>
      </div>
      <div class="domain-tip">{{ t('si.domainWithPortTip') }}</div>
      <el-table :data="domains" stripe class="domain-table">
        <el-table-column :label="t('si.domainOrder')" width="96" align="left" header-align="left">
          <template #default="{ $index }">
            <div
              class="domain-order-cell"
              :class="{
                'is-dragging': draggingDomainIndex === $index,
                'is-drop-target': draggingDomainIndex !== null && dragOverDomainIndex === $index && draggingDomainIndex !== $index,
              }"
              @dragover.prevent="handleDomainDragOver($index, $event)"
              @drop="handleDomainDrop($index, $event)"
            >
              <el-tooltip :content="t('si.dragToSort')" :show-after="250">
                <button
                  type="button"
                  class="drag-handle-btn"
                  draggable="true"
                  @dragstart="handleDomainDragStart($index, $event)"
                  @dragend="handleDomainDragEnd"
                >
                  <span class="drag-grip">⋮⋮</span>
                </button>
              </el-tooltip>
              <span class="order-text">{{ $index + 1 }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column
          :label="t('si.domain')"
          min-width="220"
          align="left"
          header-align="left"
          class-name="domain-col"
          header-class-name="domain-col-header"
        >
          <template #default="{ row }">
            <div v-if="editingDomainId === row.id" class="domain-edit-row">
              <el-input
                v-model="editingDomainValue"
                size="small"
                :placeholder="t('si.domainPlaceholderWithPort')"
                @keydown.enter.prevent="handleSaveEditDomain(row)"
                @blur="handleEditInputBlur"
              />
              <el-button
                size="small"
                circle
                class="action-btn action-primary"
                @mousedown.prevent="markEditActionMouseDown"
                @click="handleSaveEditDomain(row)"
              >
                <el-icon><Check /></el-icon>
              </el-button>
              <el-button
                size="small"
                circle
                class="action-btn"
                @mousedown.prevent="markEditActionMouseDown"
                @click="cancelEditDomain"
              >
                <el-icon><Close /></el-icon>
              </el-button>
            </div>
            <div v-else class="domain-display-row">
              <el-link
                type="primary"
                class="domain-link"
                :href="toDomainUrl(row.domain)"
                target="_blank"
                rel="noopener noreferrer"
              >
                {{ getDomainHost(row.domain) }}
              </el-link>
              <el-tooltip :content="t('si.editDomain')" :show-after="250">
                <el-button size="small" circle class="action-btn domain-edit-btn" @click="startEditDomain(row)">
                  <el-icon><Edit /></el-icon>
                </el-button>
              </el-tooltip>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('si.domainPort')" width="120" align="center">
          <template #default="{ row }">
            <span class="port-text">{{ getDomainPort(row.domain) || '-' }}</span>
          </template>
        </el-table-column>
        <el-table-column prop="created_at" :label="t('si.domainAddedAt')" width="180" />
        <el-table-column width="74" align="right" header-align="right">
          <template #header></template>
          <template #default="{ row }">
            <div class="domain-actions">
              <el-tooltip :content="t('delete')" :show-after="250">
                <el-button size="small" circle class="action-btn action-danger" @click="handleRemoveDomain(row.id)">
                  <el-icon><Delete /></el-icon>
                </el-button>
              </el-tooltip>
            </div>
          </template>
        </el-table-column>
      </el-table>
    </el-card>

    <el-card v-else-if="activeTab === 'backup'" shadow="hover" class="detail-card">
      <SiteBackups :site-id="siteId" />
    </el-card>

    <el-card v-else shadow="hover" class="detail-card">
      <el-input v-model="editForm.extra_config" type="textarea" :rows="16" :placeholder="t('si.configPlaceholder')" />
      <div class="config-actions">
        <el-button type="primary" @click="handleSave">{{ t('si.saveConfig') }}</el-button>
      </div>
    </el-card>
    </template>

    <OperationLogDialog
      v-model="installLogDialogVisible"
      :title="t('st.openrestyInstallLog')"
      :description="openrestyStatus.message || t('st.openrestyInstallStarting')"
      :status="installLogStatus"
      :loading="installingOpenresty"
      :log="installLog"
      @clear="installLog = ''"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from '@/i18n'
import {
  getSite,
  updateSite,
  startSite,
  stopSite,
  getDomains,
  addDomain,
  reorderDomains,
  updateDomain,
  removeDomain,
  getOpenRestyStatus,
  quickInstallOpenResty,
} from '@/api'
import { ElMessage, ElMessageBox } from 'element-plus'
import { ArrowLeft, Edit, Delete, Check, Close, WarningFilled } from '@element-plus/icons-vue'
import SiteBackups from './SiteBackups.vue'

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const siteId = Number(route.params.id)
const loading = ref(false)
const activeTab = ref('info')
const site = ref<any>({})
const domains = ref<any[]>([])
const newDomain = ref('')
const editingDomainId = ref<number | null>(null)
const editingDomainValue = ref('')
const suppressNextEditBlur = ref(false)
const draggingDomainIndex = ref<number | null>(null)
const dragOverDomainIndex = ref<number | null>(null)
const installingOpenresty = ref(false)
const installLogDialogVisible = ref(false)
const installLogStatus = ref<'idle' | 'running' | 'success' | 'error'>('idle')
const installLog = ref('')
const openrestyStatus = ref<any>({
  supported: true,
  installed: false,
  message: '',
})
const canManageSites = ref(false)

const editForm = reactive({
  name: '',
  site_type: 'static',
  doc_root: '',
  index_files: '',
  proxy_pass: '',
  stopped_page_html: '',
  extra_config: '',
})

onMounted(async () => {
  loading.value = true
  try {
    await loadOpenRestyStatus()
    if (canManageSites.value) {
      await Promise.all([loadSite(), loadDomains()])
    }
  } finally {
    loading.value = false
  }
})

async function loadOpenRestyStatus() {
  const res: any = await getOpenRestyStatus()
  if (res.success && res.data) {
    openrestyStatus.value = {
      supported: !!res.data.supported,
      installed: !!res.data.installed,
      message: String(res.data.message || ''),
      ...res.data,
    }
    canManageSites.value = !!openrestyStatus.value.installed
  } else {
    canManageSites.value = false
  }
}

async function handleQuickInstallOpenResty() {
  if (!openrestyStatus.value?.supported) {
    ElMessage.warning(t('st.openrestyUnsupportedInstall'))
    return
  }
  try {
    await ElMessageBox.confirm(t('st.openrestyInstallConfirm'), t('confirm'), { type: 'warning' })
  } catch {
    return
  }

  installLogStatus.value = 'running'
  installLog.value = `[${new Date().toLocaleString()}] ${t('st.openrestyInstallStarting')}\n`

  installingOpenresty.value = true
  try {
    const res: any = await quickInstallOpenResty()
    if (res.success && res.data) {
      installLog.value += String(res.data.log || '')
      if (!installLog.value.endsWith('\n')) installLog.value += '\n'
      installLogStatus.value = 'success'
      await loadOpenRestyStatus()
      if (canManageSites.value) {
        await Promise.all([loadSite(), loadDomains()])
      }
      ElMessage.success(t('st.openrestyInstallSuccess'))
    } else if (res.message) {
      installLog.value += `${res.message}\n`
      installLogStatus.value = 'error'
    }
  } catch (error: any) {
    const message = String(error?.message || t('api.requestFailed'))
    installLog.value += `${message}\n`
    installLogStatus.value = 'error'
  } finally {
    installingOpenresty.value = false
  }
}

async function loadSite() {
  const siteRes: any = await getSite(siteId)
  if (siteRes.success && siteRes.data) {
    site.value = siteRes.data
    Object.assign(editForm, {
      name: siteRes.data.name,
      site_type: siteRes.data.site_type,
      doc_root: siteRes.data.doc_root,
      index_files: siteRes.data.index_files || '',
      proxy_pass: siteRes.data.proxy_pass || '',
      stopped_page_html: siteRes.data.stopped_page_html || '',
      extra_config: siteRes.data.extra_config || '',
    })
  }
}

async function loadDomains() {
  const domainRes: any = await getDomains(siteId)
  if (domainRes.success) domains.value = domainRes.data || []
}

function normalizeDomainInput(value: string) {
  return value
    .trim()
    .replace(/^[a-z]+:\/\//i, '')
    .replace(/\/.*$/, '')
}

function splitDomain(value: string) {
  const domain = String(value || '').trim()
  const match = domain.match(/^(.*):(\d{1,5})$/)
  if (!match) return { host: domain, port: '' }
  return { host: match[1], port: match[2] }
}

function getDomainHost(value: string) {
  return splitDomain(value).host
}

function getDomainPort(value: string) {
  return splitDomain(value).port
}

function toDomainUrl(value: string) {
  const domain = String(value || '').trim()
  if (!domain) return '#'
  if (/^https?:\/\//i.test(domain)) return domain
  return `http://${domain}`
}

async function handleSave() {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  const res: any = await updateSite(siteId, editForm)
  if (res.success) {
    site.value = res.data
    Object.assign(editForm, {
      name: res.data?.name || '',
      site_type: res.data?.site_type || 'static',
      doc_root: res.data?.doc_root || '',
      index_files: res.data?.index_files || '',
      proxy_pass: res.data?.proxy_pass || '',
      stopped_page_html: res.data?.stopped_page_html || '',
      extra_config: res.data?.extra_config || '',
    })
    ElMessage.success(t('si.saved'))
  }
}

async function handleStart() {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  const res: any = await startSite(siteId)
  if (res.success) { site.value.status = 'running'; ElMessage.success(t('si.started')) }
}

async function handleStop() {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  const res: any = await stopSite(siteId)
  if (res.success) { site.value.status = 'stopped'; ElMessage.success(t('si.stoppedMsg')) }
}

async function handleAddDomain() {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  const domain = normalizeDomainInput(newDomain.value)
  if (!domain) return
  const res: any = await addDomain(siteId, { domain })
  if (res.success) {
    await loadDomains()
    newDomain.value = ''
    ElMessage.success(t('si.domainAdded'))
  }
}

function startEditDomain(row: any) {
  editingDomainId.value = row.id
  editingDomainValue.value = row.domain
}

function markEditActionMouseDown() {
  suppressNextEditBlur.value = true
}

function handleEditInputBlur() {
  if (suppressNextEditBlur.value) {
    suppressNextEditBlur.value = false
    return
  }
  cancelEditDomain()
}

function cancelEditDomain() {
  suppressNextEditBlur.value = false
  editingDomainId.value = null
  editingDomainValue.value = ''
}

async function handleSaveEditDomain(row: any) {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  suppressNextEditBlur.value = false
  const domain = normalizeDomainInput(editingDomainValue.value)
  if (!domain) return
  const res: any = await updateDomain(siteId, row.id, { domain })
  if (res.success) {
    await loadDomains()
    cancelEditDomain()
    ElMessage.success(t('si.domainUpdated'))
  }
}

function handleDomainDragStart(index: number, event: DragEvent) {
  draggingDomainIndex.value = index
  dragOverDomainIndex.value = index
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', String(domains.value[index]?.id || ''))
  }
}

function handleDomainDragOver(index: number, event: DragEvent) {
  event.preventDefault()
  if (draggingDomainIndex.value === null) return
  if (index === draggingDomainIndex.value) return
  dragOverDomainIndex.value = index
}

function handleDomainDragEnd() {
  draggingDomainIndex.value = null
  dragOverDomainIndex.value = null
}

async function handleDomainDrop(index: number, event: DragEvent) {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  event.preventDefault()
  const from = draggingDomainIndex.value
  if (from === null) {
    handleDomainDragEnd()
    return
  }
  if (from === index) {
    handleDomainDragEnd()
    return
  }

  const next = [...domains.value]
  const [item] = next.splice(from, 1)
  next.splice(index, 0, item)
  domains.value = next
  handleDomainDragEnd()

  const orderedIds = next
    .map((d: any) => Number(d.id))
    .filter((id: number) => Number.isFinite(id))
  if (orderedIds.length !== next.length) {
    await loadDomains()
    ElMessage.error(t('api.domainOrderInvalid'))
    return
  }

  try {
    const res: any = await reorderDomains(siteId, { ordered_ids: orderedIds })
    if (res.success) {
      domains.value = res.data || next
      ElMessage.success(t('si.domainOrderUpdated'))
    } else {
      await loadDomains()
    }
  } catch {
    await loadDomains()
  }
}

async function handleRemoveDomain(id: number) {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  const res: any = await removeDomain(siteId, id)
  if (res.success) {
    if (editingDomainId.value === id) cancelEditDomain()
    await loadDomains()
    ElMessage.success(t('si.domainDeleted'))
  }
}
</script>

<style scoped lang="scss">
.site-detail {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.guard-card {
  border: 1px solid var(--lp-border-light);
}
.guard-title {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 16px;
  font-weight: 600;
  color: var(--lp-text-primary);
  .el-icon {
    color: var(--lp-warning);
  }
}
.guard-desc {
  margin-top: 8px;
  color: var(--lp-text-secondary);
  font-size: 13px;
  line-height: 1.6;
}
.guard-actions {
  margin-top: 12px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.detail-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  padding: 14px 16px;
  background: var(--lp-card-bg);
  border: 1px solid var(--lp-border-light);
  border-radius: var(--lp-radius-lg);
  box-shadow: var(--lp-shadow-sm);
}
.title-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.back-btn {
  color: var(--lp-text-secondary);
  &:hover { color: var(--lp-primary); }
}
.site-name {
  font-size: 20px;
  font-weight: 600;
  color: var(--lp-text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.section-nav {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px;
  border-radius: 12px;
  border: 1px solid var(--lp-border-light);
  background: var(--lp-card-bg);
  box-shadow: var(--lp-shadow-sm);
  width: fit-content;
}
.section-item {
  height: 38px;
  min-width: 100px;
  padding: 0 16px;
  border: 0;
  border-radius: 9px;
  background: transparent;
  color: var(--lp-text-secondary);
  font-size: 14px;
  font-weight: 600;
  line-height: 38px;
  cursor: pointer;
  transition: all var(--lp-transition);
  &:hover {
    color: var(--lp-primary);
    background: var(--lp-primary-bg);
  }
  &.active {
    color: var(--lp-primary);
    background: var(--lp-primary-bg);
    box-shadow: 0 0 0 1px rgba(79,110,247,0.18) inset;
  }
}
.detail-card {
  background: var(--lp-card-bg);
  border: 1px solid var(--lp-border-light);
  border-radius: var(--lp-radius-lg);
  :deep(.el-card__body) { padding: 20px; }
}
.info-form {
  max-width: 640px;
}
.form-tip {
  margin-top: 6px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--lp-text-secondary);
}
.domain-toolbar {
  display: flex;
  gap: 8px;
  margin-bottom: 8px;
  flex-wrap: wrap;
  align-items: center;
}
.domain-input {
  width: 360px;
  max-width: 100%;
}
.domain-tip {
  margin-bottom: 12px;
  font-size: 12px;
  color: var(--lp-text-secondary);
  line-height: 1.5;
}
.domain-table {
  :deep(.el-table__cell) {
    vertical-align: middle;
  }
  :deep(.domain-col .cell),
  :deep(.domain-col-header .cell) {
    text-align: left !important;
    justify-content: flex-start !important;
  }
}
.domain-order-cell {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 30px;
  border-radius: 8px;
  padding: 0 4px;
  transition: background-color var(--lp-transition);
  &.is-drop-target {
    background: var(--lp-primary-bg);
  }
  &.is-dragging {
    opacity: .55;
  }
}
.drag-handle-btn {
  width: 22px;
  height: 22px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--lp-text-placeholder);
  cursor: grab;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  transition: all var(--lp-transition);
  &:hover {
    color: var(--lp-primary);
    background: var(--lp-primary-bg);
  }
  &:active {
    cursor: grabbing;
  }
}
.drag-grip {
  font-size: 13px;
  line-height: 1;
  letter-spacing: 1px;
}
.order-text {
  font-size: 12px;
  color: var(--lp-text-secondary);
  font-family: 'Cascadia Code', 'Fira Code', monospace;
}
.domain-link {
  display: inline-block;
  text-align: left !important;
  flex: 0 1 auto;
  min-width: 0;
  max-width: calc(100% - 38px);
  vertical-align: middle;
  :deep(.el-link__inner) {
    text-align: left !important;
    max-width: 100%;
    display: inline-block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
}
.domain-display-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.domain-edit-btn {
  flex-shrink: 0;
}
.port-text {
  display: inline-block;
  font-family: 'Cascadia Code', 'Fira Code', monospace;
  font-size: 12px;
  color: var(--lp-text-secondary);
}
.domain-edit-row {
  display: flex;
  align-items: center;
  gap: 2px;
  :deep(.el-input__wrapper) {
    min-height: 30px;
  }
  :deep(.el-button + .el-button) {
    margin-left: 0 !important;
  }
  .action-btn {
    width: 28px;
    height: 28px;
    margin-left: 0 !important;
  }
}
.domain-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0;
}
.action-btn {
  width: 30px;
  height: 30px;
  padding: 0;
  border: 1px solid var(--lp-border) !important;
  background: var(--lp-card-bg) !important;
  color: var(--lp-text-secondary) !important;
  &:hover {
    border-color: var(--lp-primary) !important;
    color: var(--lp-primary) !important;
    background: var(--lp-primary-bg) !important;
  }
}
.action-primary {
  border-color: rgba(79, 110, 247, 0.28) !important;
  color: var(--lp-primary) !important;
  background: var(--lp-primary-bg) !important;
}
.action-danger {
  border-color: rgba(255, 77, 79, 0.28) !important;
  color: var(--lp-danger) !important;
  background: var(--lp-danger-bg) !important;
  &:hover {
    border-color: var(--lp-danger) !important;
    color: var(--lp-danger) !important;
    background: var(--lp-danger-bg) !important;
  }
}
.config-actions {
  margin-top: 12px;
}
</style>
