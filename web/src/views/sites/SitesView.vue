<template>
  <div class="sites-view">
    <div class="top-nav">
      <div class="category-nav">
        <button
          type="button"
          class="category-item"
          :class="{ active: activeCategory === 'sites' }"
          @click="setActiveCategory('sites')"
        >
          {{ t('si.tabSite') }}
        </button>
        <button
          type="button"
          class="category-item"
          :class="{ active: activeCategory === 'certs' }"
          @click="setActiveCategory('certs')"
        >
          {{ t('si.tabCert') }}
        </button>
        <button
          type="button"
          class="category-item"
          :class="{ active: activeCategory === 'openresty' }"
          @click="setActiveCategory('openresty')"
        >
          {{ t('si.tabOpenresty') }}
        </button>
      </div>
      <el-button
        v-if="activeCategory === 'sites'"
        type="primary"
        :icon="Plus"
        :disabled="!canManageSites"
        @click="showCreate = true"
      >
        {{ t('si.addSite') }}
      </el-button>
    </div>

    <el-card
      v-if="activeCategory === 'sites' && !canManageSites"
      shadow="hover"
      class="openresty-guard-card"
      v-loading="statusLoading || installingOpenresty"
    >
      <div class="openresty-guard">
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
          <el-button @click="setActiveCategory('openresty')">{{ t('si.openrestyGoSettings') }}</el-button>
          <el-button @click="loadOpenRestyStatus(true)">{{ t('refresh') }}</el-button>
        </div>
      </div>
    </el-card>
    <el-card v-else-if="activeCategory === 'sites'" shadow="hover">
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
              <el-tooltip :content="t('ct.sslSettings')" :show-after="250">
                <el-button
                  size="small"
                  circle
                  class="action-btn"
                  :class="{ 'action-success': row.ssl_enabled }"
                  @click="openSslDialog(row)"
                >
                  <el-icon><Lock /></el-icon>
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
    <el-card v-else-if="activeCategory === 'openresty'" shadow="hover" class="openresty-settings-card">
      <div class="openresty-settings-header">
        <div class="openresty-settings-title">{{ t('st.openrestySettings') }}</div>
        <div class="openresty-settings-actions">
          <el-button size="small" @click="loadOpenRestyStatus(true)">{{ t('refresh') }}</el-button>
          <el-button
            size="small"
            type="primary"
            :disabled="!openrestyStatus.supported || openrestyStatus.installed"
            :loading="installingOpenresty"
            @click="handleQuickInstallOpenResty"
          >
            {{ t('st.quickInstallOpenresty') }}
          </el-button>
          <el-button size="small" :disabled="!installLog" @click="installLogDialogVisible = true">
            {{ t('st.viewInstallLog') }}
          </el-button>
        </div>
      </div>
      <div class="openresty-status-box" v-loading="statusLoading || installingOpenresty">
        <div class="openresty-status-tags">
          <el-tag :type="openrestyStatus.installed ? 'success' : 'danger'" effect="dark" size="small">
            {{ openrestyStatus.installed ? t('st.openrestyInstalled') : t('st.openrestyNotInstalled') }}
          </el-tag>
          <el-tag :type="openrestyForm.openresty_enabled ? 'success' : 'info'" size="small">
            {{ t('st.openrestyEnabled') }}: {{ openrestyForm.openresty_enabled ? 'ON' : 'OFF' }}
          </el-tag>
          <el-tag v-if="openrestyStatus.version" type="info" size="small">
            {{ openrestyStatus.version }}
          </el-tag>
        </div>
        <div class="openresty-status-message">{{ openrestyStatus.message || '-' }}</div>
        <div class="openresty-status-meta">
          <span>{{ t('st.openrestyCurrentBin') }}: {{ openrestyStatus.nginx_bin || '-' }}</span>
          <span>{{ t('st.openrestyCurrentConf') }}: {{ openrestyStatus.nginx_conf || '-' }}</span>
          <span>{{ t('st.openrestyCurrentSitesDir') }}: {{ openrestyStatus.sites_dir || '-' }}</span>
        </div>
      </div>
      <el-alert
        v-if="!openrestyStatus.installed"
        type="warning"
        show-icon
        :closable="false"
        :title="t('si.openrestyRequiredTitle')"
        :description="t('si.openrestyRequiredDesc')"
      />
      <el-form :model="openrestyForm" label-width="120px" label-position="left" class="openresty-settings-form">
        <el-form-item :label="t('st.openrestyEnabled')">
          <el-switch v-model="openrestyForm.openresty_enabled" />
        </el-form-item>
        <el-form-item :label="t('st.openrestyBin')">
          <el-input v-model="openrestyForm.openresty_bin" :placeholder="t('st.openrestyBinPlaceholder')" />
        </el-form-item>
        <el-form-item :label="t('st.openrestySitesDir')">
          <el-input v-model="openrestyForm.openresty_sites_dir" :placeholder="t('st.openrestySitesDirPlaceholder')" />
        </el-form-item>
        <el-form-item :label="t('st.openrestyConfPath')">
          <el-input v-model="openrestyForm.openresty_conf_path" :placeholder="t('st.openrestyConfPathPlaceholder')" />
        </el-form-item>
        <el-form-item :label="t('st.phpFpmAddr')">
          <el-input v-model="openrestyForm.php_fpm_addr" :placeholder="t('st.phpFpmAddrPlaceholder')" />
        </el-form-item>
        <el-form-item :label="t('st.openrestyIpv6')">
          <el-switch v-model="openrestyForm.openresty_listen_ipv6" />
        </el-form-item>
        <el-form-item>
          <el-button type="primary" :loading="savingOpenresty" @click="saveOpenrestySettings">
            {{ t('st.saveSettings') }}
          </el-button>
        </el-form-item>
      </el-form>
    </el-card>
    <CertPanel v-else ref="certPanelRef" :sites="certSiteOptions" />

    <el-dialog v-model="showCreate" :title="t('si.addSite')" width="500px" v-drag :modal="false" :close-on-click-modal="false">
      <el-form :model="createForm" label-width="80px">
        <el-form-item :label="t('si.siteName')">
          <el-input v-model="createForm.name" />
        </el-form-item>
        <el-form-item :label="t('si.domain')">
          <div class="domain-editor" @click="handleDomainEditorClick" @dblclick="handleDomainEditorDblClick">
            <template v-if="!domainTextMode">
              <el-tag
                v-for="domain in createForm.domains"
                :key="domain"
                closable
                size="small"
                :class="[
                  'domain-tag',
                  { 'domain-tag-conflict': domainConflictMap[domain] },
                ]"
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
                @paste="handleDomainInputPaste"
              />
            </template>
            <textarea
              v-else
              ref="domainTextRef"
              v-model="domainTextBuffer"
              class="domain-textarea"
              spellcheck="false"
              @keydown="handleDomainTextKeydown"
              @blur="applyDomainTextMode"
            />
          </div>
          <div class="form-tip">{{ t('si.domainInputTip') }}</div>
        </el-form-item>
        <el-form-item :label="t('si.rootDir')">
          <div class="root-dir-row">
            <el-input v-model="createForm.doc_root" placeholder="/www/wwwroot" />
            <el-tooltip :content="t('si.chooseDir')" :show-after="250">
              <el-button circle class="dir-picker-btn" @click="showDirPicker = true">
                <el-icon><FolderOpened /></el-icon>
              </el-button>
            </el-tooltip>
          </div>
          <div class="form-tip">{{ t('si.rootDirTip') }}</div>
        </el-form-item>

        <el-form-item :label="t('si.siteType')">
          <el-select v-model="createForm.site_type" style="width: 100%">
            <el-option v-for="option in siteTypeOptions" :key="option.value" :label="option.label" :value="option.value" />
          </el-select>
          <div class="form-tip">{{ siteTypeTip }}</div>
        </el-form-item>

        <!-- App-backed types proxy to a managed application, so pick which one. -->
        <el-form-item v-if="isAppSiteType" :label="t('si.linkedApp')">
          <el-select v-model="createForm.app_id" style="width: 100%" :placeholder="t('si.selectApp')" clearable>
            <el-option
              v-for="app in matchingApps"
              :key="app.id"
              :label="`${app.name} — :${app.port}`"
              :value="app.id"
            />
          </el-select>
          <div class="form-tip">
            {{ matchingApps.length ? t('si.linkedAppTip') : t('si.noMatchingApp') }}
          </div>
        </el-form-item>

        <el-form-item v-if="createForm.site_type === 'proxy'" :label="t('si.proxyAddress')">
          <el-input v-model="createForm.proxy_pass" placeholder="http://127.0.0.1:8080" class="lp-mono" />
        </el-form-item>

        <el-form-item :label="t('si.rewriteRules')">
          <el-input
            v-model="createForm.rewrite_rules"
            type="textarea"
            :rows="3"
            class="lp-mono"
            :placeholder="'rewrite ^/(.*)$ /index.php/$1 last;'"
          />
          <div class="rewrite-presets">
            <button
              v-for="preset in rewritePresets"
              :key="preset.label"
              type="button"
              class="preset-chip"
              @click="createForm.rewrite_rules = preset.value"
            >{{ preset.label }}</button>
          </div>
          <div class="form-tip">{{ t('si.rewriteTip') }}</div>
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

    <el-dialog v-model="sslVisible" :title="sslTitle" width="520px">
      <el-form label-position="top">
        <el-form-item :label="t('ct.certificate')">
          <el-select v-model="sslForm.cert_id" clearable style="width: 100%" :placeholder="t('ct.noCertBound')">
            <el-option
              v-for="cert in sslCertOptions"
              :key="cert.id"
              :label="`${cert.name} — ${cert.domains} (${cert.days_remaining}d)`"
              :value="cert.id"
            />
          </el-select>
          <div class="form-tip">{{ t('ct.bindHint') }}</div>
        </el-form-item>

        <el-form-item v-if="sslForm.cert_id">
          <el-switch v-model="sslForm.force_https" :active-text="t('ct.forceHttps')" />
          <div class="form-tip">{{ t('ct.forceHttpsHint') }}</div>
        </el-form-item>

        <el-form-item v-if="sslForm.cert_id">
          <el-switch v-model="sslForm.hsts" :active-text="t('ct.hsts')" />
          <div class="form-tip">{{ t('ct.hstsHint') }}</div>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="sslVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="savingSsl" @click="submitSsl">{{ t('ct.save') }}</el-button>
      </template>
    </el-dialog>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from '@/i18n'
import CertPanel from './CertPanel.vue'
import {
  getSites,
  createSite,
  deleteSite,
  startSite,
  stopSite,
  getSettings,
  saveSettingsApi,
  getOpenRestyStatus,
  quickInstallOpenResty,
  getApps,
  getCerts,
  bindSiteSsl,
} from '@/api'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Plus, FolderOpened, VideoPlay, VideoPause, Setting, Delete, WarningFilled, Lock,
} from '@element-plus/icons-vue'

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const activeCategory = ref<'sites' | 'certs' | 'openresty'>('sites')
const sites = ref<any[]>([])
const loading = ref(false)
const showCreate = ref(false)
const showDirPicker = ref(false)
const creating = ref(false)
const statusLoading = ref(false)
const savingOpenresty = ref(false)
const installingOpenresty = ref(false)
const installLogDialogVisible = ref(false)
const installLogStatus = ref<'idle' | 'running' | 'success' | 'error'>('idle')
const installLog = ref('')
const domainInputRef = ref<HTMLInputElement>()
const domainTextRef = ref<HTMLTextAreaElement>()
const domainInput = ref('')
const domainTextMode = ref(false)
const domainTextBuffer = ref('')
const domainConflictMap = reactive<Record<string, boolean>>({})
const openrestyStatus = ref<any>({
  supported: true,
  installed: false,
  message: '',
})
const canManageSites = computed(() => Boolean(openrestyStatus.value?.installed))

const openrestyForm = reactive({
  openresty_enabled: false,
  openresty_bin: '/usr/local/openresty/nginx/sbin/nginx',
  openresty_sites_dir: '/usr/local/openresty/nginx/conf/sites-enabled',
  openresty_conf_path: '/usr/local/openresty/nginx/conf/nginx.conf',
  php_fpm_addr: '127.0.0.1:9000',
  openresty_listen_ipv6: true,
})

const createForm = reactive({
  name: '',
  domains: [] as string[],
  doc_root: '/www/wwwroot',
  site_type: 'static',
  app_id: undefined as number | undefined,
  proxy_pass: '',
  rewrite_rules: '',
})

type AppOption = { id: number; name: string; runtime: string; port: number }
const appOptions = ref<AppOption[]>([])

const siteTypeOptions = computed(() => [
  { value: 'static', label: t('si.typeStatic') },
  { value: 'php', label: 'PHP' },
  { value: 'node', label: 'Node.js' },
  { value: 'python', label: 'Python' },
  { value: 'java', label: 'Java' },
  { value: 'go', label: 'Go' },
  { value: 'binary', label: t('si.typeBinary') },
  { value: 'proxy', label: t('si.typeProxy') },
])

const APP_SITE_TYPES = ['node', 'python', 'java', 'go', 'binary']
const isAppSiteType = computed(() => APP_SITE_TYPES.includes(createForm.site_type))

/** Only offer applications whose runtime matches the chosen site type. */
const matchingApps = computed(() =>
  appOptions.value.filter((app) => app.runtime === createForm.site_type && app.port > 0),
)

const siteTypeTip = computed(() => {
  if (isAppSiteType.value) return t('si.typeAppTip')
  if (createForm.site_type === 'php') return t('si.typePhpTip')
  if (createForm.site_type === 'proxy') return t('si.typeProxyTip')
  return t('si.typeStaticTip')
})

const rewritePresets = computed(() => [
  { label: t('si.presetNone'), value: '' },
  { label: 'WordPress', value: 'rewrite ^/(.*)$ /index.php?$1 last;' },
  { label: 'Laravel / ThinkPHP', value: 'rewrite ^/(.*)$ /index.php/$1 last;' },
  { label: t('si.presetForceHttps'), value: 'if ($scheme = http) { return 301 https://$host$request_uri; }' },
])


// --- Certificates / SSL ---
const certPanelRef = ref()
const sslVisible = ref(false)
const savingSsl = ref(false)
const sslTarget = ref<any>(null)
const sslCertOptions = ref<any[]>([])
const sslForm = reactive({
  cert_id: undefined as number | undefined,
  force_https: false,
  hsts: false,
})

/** The cert panel offers site webroots as ACME challenge directories. */
const certSiteOptions = computed(() =>
  sites.value.map((site: any) => ({ id: site.id, name: site.name, doc_root: site.doc_root })),
)

const sslTitle = computed(() =>
  sslTarget.value ? `${sslTarget.value.name} — ${t('ct.sslSettings')}` : t('ct.sslSettings'),
)

async function openSslDialog(row: any) {
  sslTarget.value = row
  sslForm.cert_id = undefined
  sslForm.force_https = !!row.force_https
  sslForm.hsts = !!row.hsts
  sslVisible.value = true

  try {
    const res: any = await getCerts()
    if (res.success) {
      sslCertOptions.value = res.data || []
      // Preselect whichever certificate this site already points at.
      const current = sslCertOptions.value.find((cert: any) => cert.cert_path === row.ssl_cert_path)
      if (current) sslForm.cert_id = current.id
    }
  } catch {
    // Interceptor reports it.
  }
}

async function submitSsl() {
  if (!sslTarget.value) return
  savingSsl.value = true
  try {
    const res: any = await bindSiteSsl(sslTarget.value.id, {
      cert_id: sslForm.cert_id ?? null,
      force_https: sslForm.force_https,
      hsts: sslForm.hsts,
    })
    if (res.success) {
      ElMessage.success(t('ct.sslUpdated'))
      sslVisible.value = false
      await loadSites()
      certPanelRef.value?.load?.()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    savingSsl.value = false
  }
}

async function loadAppOptions() {
  try {
    const res: any = await getApps()
    if (res.success) {
      appOptions.value = (res.data || []).map((app: any) => ({
        id: app.id,
        name: app.name,
        runtime: app.runtime,
        port: app.port,
      }))
    }
  } catch {
    // Non-fatal: the selector simply shows no options.
  }
}

function normalizeDomainToken(token: string) {
  return token
    .trim()
    .replace(/^[a-z]+:\/\//i, '')
    .replace(/\/.*$/, '')
    .replace(/\.+$/, '')
    .toLowerCase()
}
function parseDomainTokens(rawText: string) {
  return String(rawText || '')
    .split(/[\s,，;；]+/)
    .map((item) => item.trim())
    .filter(Boolean)
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
function isDomainBound(domain: string) {
  const normalized = normalizeDomainToken(domain)
  if (!normalized) return false
  for (const site of sites.value) {
    const domains = getDomainList(String(site?.domains || ''))
    for (const existing of domains) {
      if (normalizeDomainToken(existing) === normalized) {
        return true
      }
    }
  }
  return false
}
function refreshCreateDomainConflicts() {
  for (const key of Object.keys(domainConflictMap)) {
    delete domainConflictMap[key]
  }
  for (const domain of createForm.domains) {
    if (isDomainBound(domain)) {
      domainConflictMap[domain] = true
    }
  }
}
function addDomainToken(rawToken: string, silentNotify = false) {
  const token = normalizeDomainToken(rawToken)
  if (!token) return { state: 'empty' as const, token: '' }
  if (!isValidDomainToken(token)) {
    if (!silentNotify) ElMessage.warning(t('si.invalidDomain'))
    return { state: 'invalid' as const, token }
  }
  if (createForm.domains.some((d) => d.toLowerCase() === token.toLowerCase())) {
    return { state: 'duplicate' as const, token }
  }
  createForm.domains.push(token)
  const conflict = isDomainBound(token)
  if (conflict) {
    domainConflictMap[token] = true
    if (!silentNotify) ElMessage.warning(t('api.domainAlreadyExistsWithDetail', { domain: token }))
    return { state: 'conflict' as const, token }
  } else {
    delete domainConflictMap[token]
    return { state: 'added' as const, token }
  }
}
function clearCreateDomainState() {
  createForm.domains = []
  domainInput.value = ''
  domainTextBuffer.value = ''
  domainTextMode.value = false
  for (const key of Object.keys(domainConflictMap)) {
    delete domainConflictMap[key]
  }
}
function addDomainTokens(rawText: string, source: 'paste' | 'input') {
  const tokens = parseDomainTokens(rawText)
  if (!tokens.length) return
  const conflicts: string[] = []
  for (const token of tokens) {
    const result = addDomainToken(token, true)
    if (result.state === 'conflict') {
      conflicts.push(result.token)
    }
  }
  if (conflicts.length > 0) {
    ElMessage.warning(t('api.domainAlreadyExistsWithDetail', { domain: conflicts[0] }))
  }
  if (source === 'paste') return
}
function commitDomainInput() {
  if (!domainInput.value.trim()) return
  addDomainTokens(domainInput.value, 'input')
  domainInput.value = ''
}
function handleDomainInputKeydown(event: KeyboardEvent) {
  const key = event.key.toLowerCase()
  const withModifier = event.ctrlKey || event.metaKey
  if (withModifier && key === 'a') {
    if (createForm.domains.length > 0) {
      event.preventDefault()
      enterDomainTextMode(true)
    }
    return
  }
  if ((event.key === 'Backspace' || event.key === 'Delete') && !domainInput.value.trim()) {
    const lastDomain = createForm.domains[createForm.domains.length - 1]
    if (lastDomain) {
      event.preventDefault()
      removeDomain(lastDomain)
    }
    return
  }
  if (event.key === 'Enter' || event.key === ',' || event.key === 'Tab') {
    event.preventDefault()
    commitDomainInput()
  }
}
function handleDomainEditorClick() {
  if (domainTextMode.value) return
  focusDomainInput()
}
function handleDomainEditorDblClick() {
  if (createForm.domains.length === 0 && !domainInput.value.trim()) return
  enterDomainTextMode(true)
}
function focusDomainInput() { domainInputRef.value?.focus() }
function removeDomain(domain: string) {
  createForm.domains = createForm.domains.filter((d) => d !== domain)
  delete domainConflictMap[domain]
}
function handleDomainInputPaste(event: ClipboardEvent) {
  const text = event.clipboardData?.getData('text') || ''
  if (!text) return
  event.preventDefault()
  addDomainTokens(text, 'paste')
}
function enterDomainTextMode(selectAll = false) {
  const combined = [
    ...createForm.domains,
    ...parseDomainTokens(domainInput.value),
  ]
  const uniqueTokens: string[] = []
  for (const token of combined) {
    const normalized = normalizeDomainToken(token)
    if (!normalized) continue
    if (!uniqueTokens.includes(normalized)) {
      uniqueTokens.push(normalized)
    }
  }
  domainTextBuffer.value = uniqueTokens.join('\n')
  domainInput.value = ''
  domainTextMode.value = true

  nextTick(() => {
    const textarea = domainTextRef.value
    if (!textarea) return
    textarea.focus()
    if (selectAll) {
      textarea.select()
    } else {
      const length = textarea.value.length
      textarea.setSelectionRange(length, length)
    }
  })
}
function applyDomainTextMode() {
  if (!domainTextMode.value) return
  const raw = domainTextBuffer.value
  domainTextMode.value = false
  domainTextBuffer.value = ''
  createForm.domains = []
  for (const key of Object.keys(domainConflictMap)) {
    delete domainConflictMap[key]
  }
  addDomainTokens(raw, 'paste')
}
function handleDomainTextKeydown(event: KeyboardEvent) {
  const withModifier = event.ctrlKey || event.metaKey
  if ((withModifier && event.key.toLowerCase() === 'enter') || event.key === 'Escape') {
    event.preventDefault()
    applyDomainTextMode()
  }
}
function handleSelectDir(path: string) {
  createForm.doc_root = path || '/'
  showDirPicker.value = false
}

function parseBoolLike(value: unknown, fallback: boolean) {
  if (typeof value === 'boolean') return value
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase()
    if (['1', 'true', 'yes', 'on'].includes(text)) return true
    if (['0', 'false', 'no', 'off'].includes(text)) return false
  }
  if (typeof value === 'number') return value !== 0
  return fallback
}

function setActiveCategory(category: 'sites' | 'certs' | 'openresty') {
  activeCategory.value = category
  if (route.query.tab !== category) {
    router.replace({ query: { ...route.query, tab: category } })
  }
}

function applyOpenRestyStatus(status: Record<string, any> | null | undefined) {
  const data = status || {}
  openrestyStatus.value = {
    supported: parseBoolLike(data.supported, true),
    installed: parseBoolLike(data.installed, false),
    message: String(data.message || ''),
    ...data,
  }
}

async function loadOpenrestySettings() {
  try {
    const res: any = await getSettings()
    if (res.success && res.data) {
      const data = res.data as Record<string, any>
      openrestyForm.openresty_enabled = parseBoolLike(data.openresty_enabled, openrestyForm.openresty_enabled)
      openrestyForm.openresty_bin = String(data.openresty_bin ?? openrestyForm.openresty_bin)
      openrestyForm.openresty_sites_dir = String(data.openresty_sites_dir ?? openrestyForm.openresty_sites_dir)
      openrestyForm.openresty_conf_path = String(data.openresty_conf_path ?? openrestyForm.openresty_conf_path)
      openrestyForm.php_fpm_addr = String(data.php_fpm_addr ?? openrestyForm.php_fpm_addr)
      openrestyForm.openresty_listen_ipv6 = parseBoolLike(data.openresty_listen_ipv6, openrestyForm.openresty_listen_ipv6)
    }
  } catch {}
}

async function loadOpenRestyStatus(showError = false) {
  statusLoading.value = true
  try {
    const res: any = await getOpenRestyStatus()
    if (res.success && res.data) {
      applyOpenRestyStatus(res.data)
      openrestyForm.openresty_enabled = parseBoolLike(res.data.enabled, openrestyForm.openresty_enabled)
      if (res.data.nginx_bin) openrestyForm.openresty_bin = String(res.data.nginx_bin)
      if (res.data.sites_dir) openrestyForm.openresty_sites_dir = String(res.data.sites_dir)
      if (res.data.nginx_conf) openrestyForm.openresty_conf_path = String(res.data.nginx_conf)
      if (!openrestyStatus.value.installed) {
        sites.value = []
      }
    } else if (showError) {
      ElMessage.error(res.message || t('api.requestFailed'))
    }
  } catch {
    if (showError) ElMessage.error(t('api.requestFailed'))
  } finally {
    statusLoading.value = false
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
      if (res.data.status) {
        applyOpenRestyStatus(res.data.status)
      }
      await loadOpenrestySettings()
      await loadOpenRestyStatus(true)
      if (canManageSites.value) {
        await loadSites()
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

async function loadSites() {
  if (!canManageSites.value) {
    sites.value = []
    return
  }
  loading.value = true
  try {
    const res: any = await getSites()
    if (res.success) {
      sites.value = res.data || []
      refreshCreateDomainConflicts()
    }
  } catch {}
  finally { loading.value = false }
}

async function saveOpenrestySettings() {
  if (openrestyForm.openresty_enabled && !openrestyStatus.value.installed) {
    ElMessage.warning(t('st.openrestyNeedInstallFirst'))
    return
  }
  savingOpenresty.value = true
  try {
    const res: any = await saveSettingsApi(openrestyForm as Record<string, any>)
    if (res.success) {
      ElMessage.success(t('st.saved'))
      await loadOpenRestyStatus()
    } else {
      ElMessage.error(res.message || t('api.requestFailed'))
    }
  } finally {
    savingOpenresty.value = false
  }
}

async function doCreate() {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  if (domainTextMode.value) {
    applyDomainTextMode()
  }
  commitDomainInput()
  refreshCreateDomainConflicts()
  const conflictDomain = createForm.domains.find((domain) => domainConflictMap[domain])
  if (conflictDomain) {
    ElMessage.warning(t('api.domainAlreadyExistsWithDetail', { domain: conflictDomain }))
    return
  }
  const payload = {
    name: createForm.name.trim(),
    domains: [...createForm.domains],
    doc_root: createForm.doc_root.trim() || '/www/wwwroot',
    app_id: isAppSiteType.value ? createForm.app_id : undefined,
    proxy_pass: createForm.site_type === 'proxy' ? createForm.proxy_pass.trim() : undefined,
    rewrite_rules: createForm.rewrite_rules.trim() || undefined,
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
      createForm.doc_root = '/www/wwwroot'
      createForm.site_type = 'static'
      createForm.app_id = undefined
      createForm.proxy_pass = ''
      createForm.rewrite_rules = ''
      clearCreateDomainState()
      loadSites()
    }
  } finally { creating.value = false }
}

async function handleDelete(row: any) {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
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
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  const res: any = await startSite(id)
  if (res.success) loadSites()
  else ElMessage.error(res.message)
}

async function handleStop(id: number) {
  if (!canManageSites.value) {
    ElMessage.warning(t('api.openrestyRequired'))
    return
  }
  const res: any = await stopSite(id)
  if (res.success) loadSites()
  else ElMessage.error(res.message)
}

onMounted(async () => {
  const initialTab = String(route.query.tab || '').trim()
  if (initialTab === 'sites' || initialTab === 'certs' || initialTab === 'openresty') {
    activeCategory.value = initialTab
  }
  await Promise.all([loadOpenrestySettings(), loadOpenRestyStatus(), loadAppOptions()])
  if (canManageSites.value) {
    await loadSites()
  }
})
</script>
<style scoped lang="scss">
.rewrite-presets {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 6px;
}
.preset-chip {
  padding: 3px 9px;
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-full);
  background: var(--lp-surface);
  color: var(--lp-text-soft);
  font-family: inherit;
  font-size: var(--lp-text-xs);
  cursor: pointer;
  transition: all var(--lp-fast);

  &:hover {
    border-color: var(--lp-accent);
    color: var(--lp-accent);
    background: var(--lp-accent-soft);
  }
}

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
.openresty-guard {
  display: flex;
  flex-direction: column;
  gap: 10px;
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
  color: var(--lp-text-secondary);
  line-height: 1.6;
  font-size: 13px;
}
.guard-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.openresty-settings-card {
  border: 1px solid var(--lp-border-light);
  :deep(.el-card__body) {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
}
.openresty-settings-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}
.openresty-settings-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--lp-text-primary);
}
.openresty-settings-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.openresty-settings-form {
  max-width: 760px;
}
.openresty-status-box {
  width: 100%;
  padding: 12px;
  border-radius: var(--lp-radius);
  border: 1px solid var(--lp-border-light);
  background: var(--lp-bg-page);
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.openresty-status-tags {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.openresty-status-message {
  color: var(--lp-text-primary);
  font-size: 13px;
  line-height: 1.5;
}
.openresty-status-meta {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--lp-text-secondary);
  span {
    word-break: break-all;
  }
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
.domain-textarea {
  width: 100%;
  min-height: 96px;
  border: 0;
  outline: none;
  resize: none;
  background: transparent;
  color: var(--lp-text-primary);
  font-size: 13px;
  line-height: 1.6;
  font-family: 'Cascadia Code', 'Fira Code', monospace;
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
.domain-tag-conflict {
  border-color: rgba(255, 77, 79, 0.48) !important;
  background: var(--lp-danger-bg) !important;
  color: var(--lp-danger) !important;
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
