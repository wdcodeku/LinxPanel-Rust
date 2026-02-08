<template>
  <div class="sites-view">
    <div class="top-nav">
      <div class="category-nav">
        <button
          type="button"
          class="category-item"
          :class="{ active: activeCategory === 'sites' }"
          @click="activeCategory = 'sites'"
        >
          {{ t('si.tabSite') }}
        </button>
        <button
          type="button"
          class="category-item"
          :class="{ active: activeCategory === 'certs' }"
          @click="activeCategory = 'certs'"
        >
          {{ t('si.tabCert') }}
        </button>
      </div>
      <el-button
        v-if="activeCategory === 'sites'"
        type="primary"
        :icon="Plus"
        @click="showCreate = true"
      >
        {{ t('si.addSite') }}
      </el-button>
    </div>

    <el-card v-if="activeCategory === 'sites'" shadow="hover">
      <el-table :data="sites" v-loading="loading" stripe style="width:100%">
        <el-table-column prop="name" :label="t('si.siteName')" min-width="140">
          <template #default="{ row }">
            <el-link type="primary" @click="router.push(`/sites/${row.id}`)">{{ row.name }}</el-link>
          </template>
        </el-table-column>
        <el-table-column prop="domains" :label="t('si.domain')" min-width="200">
          <template #default="{ row }">
            <el-popover
              trigger="hover"
              placement="top-start"
              :width="460"
              popper-class="domain-popper"
            >
              <template #reference>
                <div class="domain-cell">
                  <el-tag
                    v-for="domain in getVisibleDomains(row.domains)"
                    :key="domain"
                    size="small"
                    class="domain-chip"
                    @click.stop="openDomain(domain)"
                  >
                    {{ domain }}
                  </el-tag>
                  <el-tag
                    v-if="getHiddenDomainCount(row.domains) > 0"
                    size="small"
                    type="info"
                    class="domain-more"
                  >
                    +{{ getHiddenDomainCount(row.domains) }}
                  </el-tag>
                </div>
              </template>
              <div class="domain-pop-content">
                <div class="domain-pop-title">{{ t('si.allDomains') }}</div>
                <div class="domain-pop-tags">
                  <a
                    v-for="domain in getDomainList(row.domains)"
                    :key="domain"
                    class="domain-pop-tag-link"
                    :href="toDomainUrl(domain)"
                    target="_blank"
                    rel="noopener noreferrer"
                    @click.stop
                  >
                    <el-tag size="small" class="domain-pop-tag">{{ domain }}</el-tag>
                  </a>
                </div>
              </div>
            </el-popover>
          </template>
        </el-table-column>
        <el-table-column prop="doc_root" :label="t('si.siteDir')" min-width="220">
          <template #default="{ row }">
            <el-link type="primary" class="site-dir-link" @click="openSiteDir(row)">
              <el-icon><FolderOpened /></el-icon>
              <span>{{ row.doc_root || '/' }}</span>
            </el-link>
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
        <el-table-column width="156" fixed="right" align="right" header-align="right">
          <template #header></template>
          <template #default="{ row }">
            <div class="site-actions">
              <el-tooltip :content="row.status === 'stopped' ? t('si.start') : t('si.stop')" :show-after="250">
                <el-button
                  size="small"
                  circle
                  class="action-btn"
                  :class="row.status === 'stopped' ? 'action-success' : 'action-warning'"
                  @click="row.status === 'stopped' ? handleStart(row.id) : handleStop(row.id)"
                >
                  <el-icon><VideoPlay v-if="row.status === 'stopped'" /><VideoPause v-else /></el-icon>
                </el-button>
              </el-tooltip>
              <el-tooltip :content="t('si.config')" :show-after="250">
                <el-button size="small" circle class="action-btn" @click="router.push(`/sites/${row.id}`)">
                  <el-icon><Setting /></el-icon>
                </el-button>
              </el-tooltip>
              <el-tooltip :content="t('delete')" :show-after="250">
                <el-button size="small" circle class="action-btn action-danger" @click="handleDelete(row)">
                  <el-icon><Delete /></el-icon>
                </el-button>
              </el-tooltip>
            </div>
          </template>
        </el-table-column>
      </el-table>
    </el-card>
    <el-card v-else shadow="hover">
      <el-empty :description="t('si.certComingSoon')" />
    </el-card>

    <el-dialog v-model="showCreate" :title="t('si.addSite')" width="500px" v-drag :modal="false" :close-on-click-modal="false">
      <el-form :model="createForm" label-width="80px">
        <el-form-item :label="t('si.siteName')">
          <el-input v-model="createForm.name" />
        </el-form-item>
        <el-form-item :label="t('si.domain')">
          <div class="domain-editor" @click="focusDomainInput">
            <el-tag
              v-for="domain in createForm.domains"
              :key="domain"
              closable
              size="small"
              class="domain-tag"
              @close="removeDomain(domain)"
            >
              {{ domain }}
            </el-tag>
            <input
              ref="domainInputRef"
              v-model="domainInput"
              class="domain-input"
              :placeholder="createForm.domains.length ? '' : t('si.domainInputPlaceholder')"
              @keydown="handleDomainInputKeydown"
              @blur="commitDomainInput"
            />
          </div>
          <div class="form-tip">{{ t('si.domainInputTip') }}</div>
        </el-form-item>
        <el-form-item :label="t('si.rootDir')">
          <div class="root-dir-row">
            <el-input v-model="createForm.doc_root" placeholder="/www/sites/example" />
            <el-tooltip :content="t('si.chooseDir')" :show-after="250">
              <el-button circle class="dir-picker-btn" @click="showDirPicker = true">
                <el-icon><FolderOpened /></el-icon>
              </el-button>
            </el-tooltip>
          </div>
          <div class="form-tip">{{ t('si.rootDirTip') }}</div>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="showCreate = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" @click="doCreate" :loading="creating">{{ t('create') }}</el-button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="showDirPicker"
      :title="t('si.chooseDir')"
      width="760px"
      destroy-on-close
      class="dir-picker-dialog"
      v-drag
      :modal="false"
      :close-on-click-modal="false"
    >
      <div class="dir-picker-shell">
        <DirPicker :init-path="createForm.doc_root || '/'" @select="handleSelectDir" />
      </div>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from '@/i18n'
import { getSites, createSite, deleteSite, startSite, stopSite } from '@/api'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus, FolderOpened, VideoPlay, VideoPause, Setting, Delete } from '@element-plus/icons-vue'

const router = useRouter()
const { t } = useI18n()
const activeCategory = ref<'sites' | 'certs'>('sites')
const sites = ref<any[]>([])
const loading = ref(false)
const showCreate = ref(false)
const showDirPicker = ref(false)
const creating = ref(false)
const domainInputRef = ref<HTMLInputElement>()
const domainInput = ref('')

const createForm = reactive({
  name: '',
  domains: [] as string[],
  doc_root: '/www/sites',
  site_type: 'static',
})

function normalizeDomainToken(token: string) {
  return token
    .trim()
    .replace(/^[a-z]+:\/\//i, '')
    .replace(/\/.*$/, '')
}
function getDomainList(rawDomains: string) {
  return String(rawDomains || '')
    .split(',')
    .map((domain) => domain.trim())
    .filter(Boolean)
}
function getVisibleDomains(rawDomains: string, maxVisible = 2) {
  return getDomainList(rawDomains).slice(0, maxVisible)
}
function getHiddenDomainCount(rawDomains: string, maxVisible = 2) {
  const total = getDomainList(rawDomains).length
  return total > maxVisible ? total - maxVisible : 0
}
function toDomainUrl(domain: string) {
  const value = domain.trim()
  if (!value) return '#'
  if (/^https?:\/\//i.test(value)) return value
  return `http://${value}`
}
function openDomain(domain: string) {
  const url = toDomainUrl(domain)
  if (url === '#') return
  window.open(url, '_blank', 'noopener,noreferrer')
}
function isValidDomainToken(token: string) {
  return token.length > 0 && !/[\s/]/.test(token)
}
function addDomainToken(rawToken: string) {
  const token = normalizeDomainToken(rawToken)
  if (!token) return
  if (!isValidDomainToken(token)) {
    ElMessage.warning(t('si.invalidDomain'))
    return
  }
  if (createForm.domains.some((d) => d.toLowerCase() === token.toLowerCase())) return
  createForm.domains.push(token)
}
function commitDomainInput() {
  if (!domainInput.value.trim()) return
  const tokens = domainInput.value
    .split(/[\s,，;；]+/)
    .map((d) => d.trim())
    .filter(Boolean)
  tokens.forEach(addDomainToken)
  domainInput.value = ''
}
function handleDomainInputKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' || event.key === ',' || event.key === 'Tab') {
    event.preventDefault()
    commitDomainInput()
  }
}
function focusDomainInput() { domainInputRef.value?.focus() }
function removeDomain(domain: string) {
  createForm.domains = createForm.domains.filter((d) => d !== domain)
}
function handleSelectDir(path: string) {
  createForm.doc_root = path || '/'
  showDirPicker.value = false
}

async function loadSites() {
  loading.value = true
  try {
    const res: any = await getSites()
    if (res.success) sites.value = res.data || []
  } catch {}
  finally { loading.value = false }
}

async function doCreate() {
  commitDomainInput()
  const payload = {
    name: createForm.name.trim(),
    domains: [...createForm.domains],
    doc_root: createForm.doc_root.trim() || '/www/sites',
    site_type: createForm.site_type,
  }
  if (!payload.name) {
    ElMessage.warning(t('si.siteNameRequired'))
    return
  }
  if (!payload.domains.length) {
    ElMessage.warning(t('si.domainRequired'))
    return
  }
  creating.value = true
  try {
    const res: any = await createSite(payload)
    if (res.success) {
      ElMessage.success(t('si.created'))
      showCreate.value = false
      createForm.name = ''
      createForm.domains = []
      createForm.doc_root = '/www/sites'
      createForm.site_type = 'static'
      domainInput.value = ''
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
function openSiteDir(row: any) {
  const target = typeof row.doc_root === 'string' && row.doc_root.trim() ? row.doc_root : '/'
  router.push({ path: '/files', query: { path: target } })
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
.sites-view {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.top-nav {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.category-nav {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px;
  border-radius: 12px;
  border: 1px solid var(--lp-border-light);
  background: var(--lp-card-bg);
  box-shadow: var(--lp-shadow-sm);
}
.category-item {
  min-width: 88px;
  height: 40px;
  padding: 0 16px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: var(--lp-text-secondary);
  font-size: 14px;
  font-weight: 600;
  line-height: 40px;
  cursor: pointer;
  transition: all var(--lp-transition);
  &:hover { color: var(--lp-primary); background: var(--lp-primary-bg); }
  &.active {
    color: var(--lp-primary);
    background: var(--lp-primary-bg);
    box-shadow: 0 0 0 1px rgba(79,110,247,0.18) inset;
  }
}
.site-dir-link {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 100%;
  span {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: 'Cascadia Code', 'Fira Code', monospace;
  }
}
.domain-cell {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  max-width: 100%;
  flex-wrap: nowrap;
  overflow: hidden;
}
.domain-chip {
  flex-shrink: 1;
  max-width: 160px;
  cursor: pointer;
  :deep(.el-tag__content) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
}
.domain-more {
  flex-shrink: 0;
}
:deep(.domain-popper) {
  border: 1px solid var(--lp-border) !important;
  background: var(--lp-popover-bg) !important;
}
.domain-pop-content {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 280px;
  overflow-y: auto;
  padding-right: 2px;
}
.domain-pop-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--lp-text-secondary);
  line-height: 1.6;
  margin-bottom: 2px;
}
.domain-pop-tags {
  display: flex;
  align-items: flex-start;
  flex-wrap: wrap;
  gap: 8px;
}
.domain-pop-tag-link {
  text-decoration: none;
  max-width: 100%;
}
.domain-pop-tag {
  max-width: 100%;
  border-color: var(--lp-tag-border) !important;
  background: var(--lp-tag-bg) !important;
  color: var(--lp-tag-text) !important;
  cursor: pointer;
  :deep(.el-tag__content) {
    display: block;
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  &:hover {
    border-color: var(--lp-primary) !important;
    color: var(--lp-primary) !important;
  }
}
.site-actions {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
}
.domain-editor {
  min-height: 36px;
  width: 100%;
  padding: 4px 8px;
  border: 1px solid var(--lp-input-border);
  border-radius: var(--lp-radius);
  background: var(--lp-input-bg);
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  cursor: text;
  transition: border-color var(--lp-transition);
  &:focus-within { border-color: var(--lp-primary); }
}
.domain-tag {
  max-width: 100%;
  :deep(.el-tag__content) {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
}
.domain-input {
  flex: 1;
  min-width: 140px;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--lp-text-primary);
  font-size: 13px;
  line-height: 24px;
}
.form-tip {
  margin-top: 6px;
  color: var(--lp-text-secondary);
  font-size: 12px;
  line-height: 1.4;
}
.root-dir-row {
  display: flex;
  align-items: center;
  gap: 10px;
}
.dir-picker-btn {
  border: 1px solid var(--lp-border) !important;
  background: var(--lp-card-bg) !important;
  color: var(--lp-text-secondary) !important;
  &:hover {
    color: var(--lp-primary) !important;
    border-color: var(--lp-primary) !important;
    background: var(--lp-primary-bg) !important;
  }
}
.dir-picker-shell {
  height: 520px;
  min-height: 520px;
  padding: 4px;
  border-radius: var(--lp-radius-lg);
  background: var(--lp-bg-page);
}
:deep(.dir-picker-dialog .el-dialog__body) {
  padding: 12px 16px 16px;
  background: var(--lp-bg-page);
}
:deep(.dir-picker-dialog .el-dialog) {
  border: 1px solid var(--lp-border-light);
  border-radius: var(--lp-radius-xl);
  overflow: hidden;
}
:deep(.dir-picker-dialog .el-dialog__header) {
  padding: 14px 18px 12px;
  border-bottom: 1px solid var(--lp-border-light);
}
:deep(.dir-picker-dialog .el-dialog__title) {
  font-size: 15px;
  font-weight: 600;
  color: var(--lp-text-primary);
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
    background: var(--lp-primary-bg) !important;
    color: var(--lp-primary) !important;
  }
}
.action-success {
  border-color: rgba(82, 196, 26, 0.28) !important;
  background: var(--lp-success-bg) !important;
  color: var(--lp-success) !important;
}
.action-warning {
  border-color: rgba(250, 173, 20, 0.28) !important;
  background: var(--lp-warning-bg) !important;
  color: var(--lp-warning) !important;
}
.action-danger {
  border-color: rgba(255, 77, 79, 0.28) !important;
  background: var(--lp-danger-bg) !important;
  color: var(--lp-danger) !important;
  &:hover {
    border-color: var(--lp-danger) !important;
    background: var(--lp-danger-bg) !important;
    color: var(--lp-danger) !important;
  }
}
</style>
