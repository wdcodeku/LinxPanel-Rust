<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('si.title') }}</h1>
        <p class="lp-page-sub">{{ t('si.pageSub') }}</p>
      </div>
      <div class="lp-page-actions">
        <el-button
          v-if="activeCategory === 'sites'"
          type="primary"
          :icon="Plus"
          :disabled="!canManageSites"
          @click="openCreate"
        >{{ t('si.addSite') }}</el-button>
      </div>
    </div>

    <el-tabs :model-value="activeCategory" class="tabs" @tab-change="(name: any) => setActiveCategory(name)">
      <!-- ============ Sites ============ -->
      <el-tab-pane :label="t('si.tabSite')" name="sites">
        <!-- OpenResty missing: say what to do, with one button. -->
        <section v-if="!canManageSites && statusChecked" class="lp-card guard">
          <div class="guard__icon"><el-icon :size="26"><WarningFilled /></el-icon></div>
          <div class="guard__body">
            <h2 class="guard__title">{{ t('si.openrestyRequiredTitle') }}</h2>
            <p class="guard__desc">{{ t('si.guardDesc') }}</p>
            <div class="guard__actions">
              <el-button
                type="primary"
                :disabled="!openrestyStatus.supported"
                :loading="installingOpenresty"
                @click="handleQuickInstallOpenResty"
              >{{ t('si.quickInstallOpenresty') }}</el-button>
              <el-button @click="setActiveCategory('openresty')">{{ t('si.openrestyGoSettings') }}</el-button>
            </div>
          </div>
        </section>

        <section v-else class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title">
              <el-icon><Connection /></el-icon>{{ t('si.tabSite') }}
              <span v-if="sitesLoaded" class="count">{{ sites.length }}</span>
            </h2>
            <el-input
              v-if="sites.length"
              v-model="keyword"
              :prefix-icon="Search"
              size="small"
              clearable
              :placeholder="t('si.search')"
              class="search"
            />
          </header>

          <div v-if="!sitesLoaded" class="lp-card-body"><el-skeleton animated :rows="4" /></div>

          <div v-else-if="!sites.length" class="empty">
            <el-icon :size="36" class="empty__icon"><Connection /></el-icon>
            <p class="empty__title">{{ t('si.emptyTitle') }}</p>
            <p class="lp-dim empty__hint">{{ t('si.emptyHint') }}</p>
            <div class="empty__actions">
              <el-button type="primary" :icon="Plus" @click="openCreate">{{ t('si.addSite') }}</el-button>
              <el-button @click="router.push('/appstore')">{{ t('si.fromAppStore') }}</el-button>
            </div>
          </div>

          <el-table v-else :data="filteredSites" v-loading="loading" :empty-text="t('si.noMatch')" class="site-table">
            <el-table-column :label="t('si.site')" min-width="240">
              <template #default="{ row }">
                <div class="site-cell">
                  <span class="dot" :class="row.status === 'running' ? 'dot--ok' : 'dot--off'"></span>
                  <div class="site-cell__text">
                    <a class="site-cell__name" @click="router.push(`/sites/${row.id}`)">{{ primaryDomain(row) }}</a>
                    <span class="site-cell__sub">
                      <template v-if="getHiddenDomainCount(row.domains, 1)">{{ t('si.moreDomains', { n: getHiddenDomainCount(row.domains, 1) }) }}</template>
                      <template v-if="row.name && row.name !== primaryDomain(row)"> · {{ row.name }}</template>
                    </span>
                  </div>
                </div>
              </template>
            </el-table-column>
            <el-table-column :label="t('si.siteType')" width="120">
              <template #default="{ row }">
                <span class="lp-pill" :class="typePill(row.site_type)">{{ typeLabel(row.site_type) }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('si.siteDir')" min-width="200">
              <template #default="{ row }">
                <a class="dir-link lp-mono" :title="t('si.openSiteDir')" @click="openSiteDir(row)">
                  <el-icon><FolderOpened /></el-icon><span class="lp-truncate">{{ row.doc_root || '/' }}</span>
                </a>
              </template>
            </el-table-column>
            <el-table-column label="SSL" width="110">
              <template #default="{ row }">
                <button type="button" class="ssl-chip" :class="{ 'is-on': row.ssl_enabled }" @click="openSslDialog(row)">
                  <el-icon><Lock /></el-icon>{{ row.ssl_enabled ? t('si.sslOn') : t('si.sslOff') }}
                </button>
              </template>
            </el-table-column>
            <el-table-column :label="t('si.status')" width="100">
              <template #default="{ row }">
                <el-switch
                  :model-value="row.status === 'running'"
                  :loading="toggling === row.id"
                  @change="(v: string | number | boolean) => toggleSite(row, v === true)"
                />
              </template>
            </el-table-column>
            <el-table-column :label="t('actions')" width="190" align="right">
              <template #default="{ row }">
                <el-button link type="primary" size="small" @click="openDomain(primaryDomain(row))">{{ t('si.visit') }}</el-button>
                <el-button link type="primary" size="small" @click="router.push(`/sites/${row.id}`)">{{ t('si.config') }}</el-button>
                <el-button link type="danger" size="small" @click="handleDelete(row)">{{ t('delete') }}</el-button>
              </template>
            </el-table-column>
          </el-table>
        </section>
      </el-tab-pane>

      <!-- ============ Certificates ============ -->
      <el-tab-pane :label="t('si.tabCert')" name="certs" lazy>
        <CertPanel ref="certPanelRef" :sites="certSiteOptions" />
      </el-tab-pane>

      <!-- ============ OpenResty ============ -->
      <el-tab-pane :label="t('si.tabOpenresty')" name="openresty">
        <section class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Setting /></el-icon>{{ t('st.openrestySettings') }}</h2>
            <div class="head-tools">
              <el-button size="small" @click="loadOpenRestyStatus(true)">{{ t('refresh') }}</el-button>
              <el-button
                size="small"
                type="primary"
                :disabled="!openrestyStatus.supported || openrestyStatus.installed"
                :loading="installingOpenresty"
                @click="handleQuickInstallOpenResty"
              >{{ t('st.quickInstallOpenresty') }}</el-button>
            </div>
          </header>
          <div class="lp-card-body" v-loading="statusLoading">
            <div class="or-status">
              <span class="lp-pill" :class="openrestyStatus.installed ? 'lp-pill--ok' : 'lp-pill--danger'">
                <span class="lp-pill__dot"></span>{{ openrestyStatus.installed ? t('st.openrestyInstalled') : t('st.openrestyNotInstalled') }}
              </span>
              <span v-if="openrestyStatus.version" class="lp-mono lp-dim">{{ openrestyStatus.version }}</span>
            </div>
            <dl class="or-meta">
              <div><dt>{{ t('st.openrestyCurrentBin') }}</dt><dd class="lp-mono">{{ openrestyStatus.nginx_bin || '—' }}</dd></div>
              <div><dt>{{ t('st.openrestyCurrentConf') }}</dt><dd class="lp-mono">{{ openrestyStatus.nginx_conf || '—' }}</dd></div>
              <div><dt>{{ t('st.openrestyCurrentSitesDir') }}</dt><dd class="lp-mono">{{ openrestyStatus.sites_dir || '—' }}</dd></div>
            </dl>
            <el-form :model="openrestyForm" label-position="top" class="or-form">
              <el-form-item :label="t('st.openrestyEnabled')">
                <el-switch v-model="openrestyForm.openresty_enabled" />
              </el-form-item>
              <el-form-item :label="t('st.openrestyBin')">
                <el-input v-model="openrestyForm.openresty_bin" class="lp-mono" :placeholder="t('st.openrestyBinPlaceholder')" />
              </el-form-item>
              <el-form-item :label="t('st.openrestySitesDir')">
                <el-input v-model="openrestyForm.openresty_sites_dir" class="lp-mono" :placeholder="t('st.openrestySitesDirPlaceholder')" />
              </el-form-item>
              <el-form-item :label="t('st.openrestyConfPath')">
                <el-input v-model="openrestyForm.openresty_conf_path" class="lp-mono" :placeholder="t('st.openrestyConfPathPlaceholder')" />
              </el-form-item>
              <el-form-item :label="t('st.phpFpmAddr')">
                <el-input v-model="openrestyForm.php_fpm_addr" class="lp-mono" :placeholder="t('st.phpFpmAddrPlaceholder')" />
              </el-form-item>
              <el-form-item :label="t('st.openrestyIpv6')">
                <el-switch v-model="openrestyForm.openresty_listen_ipv6" />
              </el-form-item>
              <el-button type="primary" :loading="savingOpenresty" @click="saveOpenrestySettings">{{ t('st.saveSettings') }}</el-button>
            </el-form>
          </div>
        </section>

        <!-- What a visitor gets for a host no site is bound to (e.g. the bare IP). -->
        <section class="lp-card default-site">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Position /></el-icon>{{ t('si.defaultSite') }}</h2>
          </header>
          <div class="lp-card-body">
            <p class="ds-intro">{{ t('si.defaultSiteDesc') }}</p>
            <div class="ds-modes" role="radiogroup">
              <button
                v-for="option in defaultSiteModes"
                :key="option.value"
                type="button"
                role="radio"
                class="ds-mode"
                :class="{ 'is-active': defaultSite.mode === option.value }"
                :aria-checked="defaultSite.mode === option.value"
                @click="defaultSite.mode = option.value"
              >
                <span class="ds-mode__radio"></span>
                <span class="ds-mode__text">
                  <span class="ds-mode__label">{{ option.label }}</span>
                  <span class="ds-mode__desc">{{ option.desc }}</span>
                </span>
              </button>
            </div>
            <el-collapse-transition>
              <div v-show="defaultSite.mode === 'page'" class="ds-html">
                <div class="ds-html__label">{{ t('si.defaultSiteHtml') }}</div>
                <el-input
                  v-model="defaultSite.html"
                  type="textarea"
                  :rows="6"
                  class="lp-mono"
                  :placeholder="t('si.defaultSiteHtmlHint')"
                />
              </div>
            </el-collapse-transition>
            <el-button
              type="primary"
              :loading="savingDefaultSite"
              :disabled="!openrestyStatus.installed"
              @click="saveDefaultSite"
            >{{ t('si.defaultSiteApply') }}</el-button>
            <span v-if="!openrestyStatus.installed" class="ds-note">{{ t('st.openrestyNeedInstallFirst') }}</span>
          </div>
        </section>
      </el-tab-pane>
    </el-tabs>

    <!-- ============ Create ============ -->
    <el-dialog v-model="showCreate" :title="t('si.addSite')" width="640px" :close-on-click-modal="false" append-to-body @opened="focusDomainInput">
      <div class="kinds">
        <button
          v-for="kind in siteKinds"
          :key="kind.value"
          type="button"
          class="kind"
          :class="{ 'is-active': siteKind === kind.value }"
          @click="selectKind(kind.value)"
        >
          <el-icon :size="20" class="kind__icon"><component :is="kind.icon" /></el-icon>
          <span class="kind__label">{{ kind.label }}</span>
          <span class="kind__desc">{{ kind.desc }}</span>
        </button>
      </div>

      <el-form :model="createForm" label-position="top" class="create-form">
        <el-form-item :label="t('si.domain')" required>
          <div class="domain-editor" @click="handleDomainEditorClick" @dblclick="handleDomainEditorDblClick">
            <template v-if="!domainTextMode">
              <el-tag
                v-for="(domain, index) in createForm.domains"
                :key="domain"
                closable
                size="small"
                :type="index === 0 ? 'primary' : 'info'"
                :class="['domain-tag', { 'domain-tag-conflict': domainConflictMap[domain] }]"
                @close="removeDomain(domain)"
              >{{ domain }}</el-tag>
              <input
                ref="domainInputRef"
                v-model="domainInput"
                class="domain-input"
                :placeholder="createForm.domains.length ? '' : 'example.com'"
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
          <div class="form-tip">{{ t('si.domainTipNew') }}</div>
        </el-form-item>

        <el-form-item v-if="siteKind === 'app'" :label="t('si.runtime')">
          <el-radio-group v-model="createForm.site_type">
            <el-radio-button v-for="r in appRuntimes" :key="r.value" :value="r.value">{{ r.label }}</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item v-if="siteKind === 'app'" :label="t('si.linkedApp')" required>
          <el-select v-model="createForm.app_id" style="width: 100%" :placeholder="t('si.selectApp')" clearable>
            <el-option v-for="app in matchingApps" :key="app.id" :label="`${app.name} — :${app.port}`" :value="app.id" />
          </el-select>
          <div class="form-tip">
            {{ matchingApps.length ? t('si.linkedAppTip') : t('si.noMatchingApp') }}
            <el-button v-if="!matchingApps.length" link type="primary" size="small" @click="router.push('/runtime')">{{ t('si.goRuntime') }}</el-button>
          </div>
        </el-form-item>

        <el-form-item v-if="siteKind === 'proxy'" :label="t('si.proxyAddress')" required>
          <el-input v-model="createForm.proxy_pass" placeholder="http://127.0.0.1:8080" class="lp-mono" />
          <div class="form-tip">{{ t('si.proxyTip') }}</div>
        </el-form-item>

        <el-form-item v-if="siteKind === 'static' || siteKind === 'php'" :label="t('si.siteDir')">
          <div class="root-dir-row">
            <el-input v-model="createForm.doc_root" class="lp-mono" placeholder="/www/wwwroot/example.com" @input="docRootEdited = true" />
            <el-button :icon="FolderOpened" @click="showDirPicker = true" />
          </div>
          <div class="form-tip">{{ t('si.rootDirTipNew') }}</div>
        </el-form-item>

        <button type="button" class="advanced-toggle" @click="showAdvanced = !showAdvanced">
          <el-icon><ArrowRight v-if="!showAdvanced" /><ArrowDown v-else /></el-icon>{{ t('si.advanced') }}
        </button>
        <template v-if="showAdvanced">
          <el-form-item :label="t('si.siteName')">
            <el-input v-model="createForm.name" :placeholder="createForm.domains[0] || t('si.siteNameAuto')" />
          </el-form-item>
          <el-form-item v-if="siteKind === 'static' || siteKind === 'php'" :label="t('si.rewriteRules')">
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
        </template>
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
      append-to-body
    >
      <div class="dir-picker-shell">
        <DirPicker :init-path="createForm.doc_root || '/'" @select="handleSelectDir" />
      </div>
    </el-dialog>

    <el-dialog v-model="sslVisible" :title="sslTitle" width="520px" append-to-body>
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
  </div>
</template>


<script setup lang="ts">
import { ref, reactive, computed, onMounted, nextTick, watch } from 'vue'
import { usePage } from '@/utils/page'
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
  setDefaultSite,
  getApps,
  getCerts,
  bindSiteSsl,
} from '@/api'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Plus, FolderOpened, Setting, WarningFilled, Lock, Search, Connection, ArrowRight, ArrowDown,
  Document, Files, Position, Cpu,
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
// First loads done: until then placeholders show, never a false "empty".
const sitesLoaded = ref(false)
const statusChecked = ref(false)
const keyword = ref('')
const toggling = ref<number | null>(null)
const showAdvanced = ref(false)
const docRootEdited = ref(false)
type SiteKind = 'static' | 'php' | 'proxy' | 'app'
const siteKind = ref<SiteKind>('static')
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

const siteKinds = computed(() => [
  { value: 'static' as SiteKind, label: t('si.kindStatic'), desc: t('si.kindStaticDesc'), icon: Document },
  { value: 'php' as SiteKind, label: t('si.kindPhp'), desc: t('si.kindPhpDesc'), icon: Files },
  { value: 'proxy' as SiteKind, label: t('si.kindProxy'), desc: t('si.kindProxyDesc'), icon: Position },
  { value: 'app' as SiteKind, label: t('si.kindApp'), desc: t('si.kindAppDesc'), icon: Cpu },
])
const appRuntimes = computed(() => siteTypeOptions.value.filter((o) => APP_SITE_TYPES.includes(o.value)))

function selectKind(kind: SiteKind) {
  siteKind.value = kind
  createForm.site_type = kind === 'app' ? 'node' : kind
  createForm.app_id = undefined
}

function openCreate() {
  clearCreateDomainState()
  Object.assign(createForm, {
    name: '', doc_root: '', site_type: 'static', app_id: undefined, proxy_pass: '', rewrite_rules: '',
  })
  siteKind.value = 'static'
  docRootEdited.value = false
  showAdvanced.value = false
  showCreate.value = true
}

// The directory follows the main domain until edited by hand.
watch(() => createForm.domains[0], (domain) => {
  if (docRootEdited.value) return
  createForm.doc_root = domain ? `/www/wwwroot/${domain.split(':')[0]}` : ''
})

const filteredSites = computed(() => {
  const q = keyword.value.trim().toLowerCase()
  if (!q) return sites.value
  return sites.value.filter((site: any) =>
    [site.name, site.domains, site.doc_root].some((v) => String(v || '').toLowerCase().includes(q)),
  )
})

function primaryDomain(row: any) {
  return getDomainList(row.domains)[0] || row.name
}

function typeLabel(type: string) {
  return siteTypeOptions.value.find((o) => o.value === type)?.label || type
}

function typePill(type: string) {
  if (type === 'php') return 'lp-pill--accent'
  if (type === 'proxy') return 'lp-pill--warn'
  if (APP_SITE_TYPES.includes(type)) return 'lp-pill--ok'
  return ''
}

async function toggleSite(row: any, on: boolean) {
  toggling.value = row.id
  try {
    if (on) await handleStart(row.id)
    else await handleStop(row.id)
  } finally {
    toggling.value = null
  }
}
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
  docRootEdited.value = true
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
      defaultSite.mode = data.openresty_default_site === 'close' ? 'close' : 'page'
      defaultSite.html = String(data.openresty_default_site_html ?? '')
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
    statusChecked.value = true
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
  finally {
    loading.value = false
    sitesLoaded.value = true
  }
}

// --- Default site ---
const defaultSite = reactive({ mode: 'page' as 'page' | 'close', html: '' })
const savingDefaultSite = ref(false)
const defaultSiteModes = computed(() => [
  { value: 'page' as const, label: t('si.defaultSitePage'), desc: t('si.defaultSitePageDesc') },
  { value: 'close' as const, label: t('si.defaultSiteClose'), desc: t('si.defaultSiteCloseDesc') },
])

async function saveDefaultSite() {
  savingDefaultSite.value = true
  try {
    const res: any = await setDefaultSite({ mode: defaultSite.mode, html: defaultSite.html })
    if (res.success) ElMessage.success(t('si.defaultSiteSaved'))
  } catch {
    // Interceptor reports it.
  } finally {
    savingDefaultSite.value = false
  }
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
    name: createForm.name.trim() || createForm.domains[0] || '',
    domains: [...createForm.domains],
    // Empty: the server uses /www/wwwroot/<main domain>.
    doc_root: createForm.doc_root.trim() || undefined,
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
  if (createForm.site_type === 'proxy' && !payload.proxy_pass) {
    ElMessage.warning(t('si.proxyRequired'))
    return
  }
  if (isAppSiteType.value && !payload.app_id) {
    ElMessage.warning(t('si.appRequired'))
    return
  }
  creating.value = true
  try {
    const res: any = await createSite(payload)
    if (res.success) {
      ElMessage.success(t('si.created'))
      showCreate.value = false
      createForm.name = ''
      createForm.doc_root = ''
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

function tabFromRoute() {
  const tab = String(route.query.tab || '').trim()
  if (tab === 'sites' || tab === 'certs' || tab === 'openresty') activeCategory.value = tab
}
onMounted(async () => {
  tabFromRoute()
  await Promise.all([loadOpenrestySettings(), loadOpenRestyStatus(), loadAppOptions()])
  if (canManageSites.value) {
    await loadSites()
  }
})
usePage(async () => {
  tabFromRoute()
  await loadOpenRestyStatus()
  if (canManageSites.value) await loadSites()
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

<style scoped lang="scss">
/* ---------- Redesigned list ---------- */
.tabs { margin-top: calc(var(--lp-2) * -1); }
.count {
  margin-left: 6px;
  padding: 0 7px;
  border-radius: var(--lp-r-full);
  background: var(--lp-muted-soft);
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  font-weight: 500;
}
.search { width: 220px; }
.head-tools { display: flex; gap: var(--lp-2); }

.guard { display: flex; gap: var(--lp-4); padding: var(--lp-5); align-items: flex-start; }
.guard__icon {
  display: grid; place-items: center; flex-shrink: 0;
  width: 48px; height: 48px; border-radius: var(--lp-r-lg);
  background: var(--lp-warn-soft); color: var(--lp-warn);
}
.guard__title { margin: 0 0 4px; font-size: var(--lp-text-md); }
.guard__desc { margin: 0 0 var(--lp-4); color: var(--lp-text-dim); font-size: var(--lp-text-sm); line-height: 1.6; }
.guard__actions { display: flex; gap: var(--lp-2); flex-wrap: wrap; }

.empty { display: flex; flex-direction: column; align-items: center; gap: var(--lp-2); padding: 48px var(--lp-4); text-align: center; }
.empty__icon { color: var(--lp-text-faint); }
.empty__title { margin: 0; font-weight: 600; }
.empty__hint { margin: 0 0 var(--lp-2); font-size: var(--lp-text-sm); max-width: 460px; line-height: 1.6; }
.empty__actions { display: flex; gap: var(--lp-2); }

.site-cell { display: flex; align-items: center; gap: var(--lp-3); min-width: 0; }
.site-cell__text { display: flex; flex-direction: column; min-width: 0; }
.site-cell__name { font-weight: 600; color: var(--lp-text); cursor: pointer; text-decoration: none; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.site-cell__name:hover { color: var(--lp-accent); }
.site-cell__sub { font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
.dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
.dot--ok { background: var(--lp-ok); box-shadow: 0 0 0 3px var(--lp-ok-soft); }
.dot--off { background: var(--lp-text-faint); }
.dir-link { display: inline-flex; align-items: center; gap: 4px; max-width: 100%; color: var(--lp-text-soft); cursor: pointer; font-size: var(--lp-text-xs); }
.dir-link:hover { color: var(--lp-accent); }
.ssl-chip {
  display: inline-flex; align-items: center; gap: 4px;
  padding: 2px 8px; border-radius: var(--lp-r-full);
  border: 1px solid var(--lp-line); background: transparent;
  color: var(--lp-text-dim); font: inherit; font-size: var(--lp-text-xs); cursor: pointer;
  &.is-on { border-color: transparent; background: var(--lp-ok-soft); color: var(--lp-ok); }
  &:hover { border-color: var(--lp-accent); }
}

/* ---------- OpenResty tab ---------- */
.or-status { display: flex; align-items: center; gap: var(--lp-3); margin-bottom: var(--lp-4); }
.or-meta {
  display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: var(--lp-3);
  margin: 0 0 var(--lp-5);
  dt { font-size: var(--lp-text-xs); color: var(--lp-text-dim); margin-bottom: 2px; }
  dd { margin: 0; font-size: var(--lp-text-xs); word-break: break-all; }
}
.or-form { max-width: 560px; }

.default-site { margin-top: var(--lp-4); }
.ds-intro { margin-bottom: var(--lp-4); color: var(--lp-text-dim); max-width: 720px; }
.ds-modes {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: var(--lp-3);
  max-width: 760px;
  margin-bottom: var(--lp-4);
}
.ds-mode {
  display: flex;
  align-items: flex-start;
  gap: var(--lp-3);
  padding: var(--lp-3) var(--lp-4);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r);
  background: var(--lp-surface);
  color: var(--lp-text);
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: border-color var(--lp-fast), background var(--lp-fast);

  &:hover { border-color: var(--lp-line-strong); }
  &.is-active { border-color: var(--lp-accent); background: var(--lp-accent-soft); }
}
.ds-mode__radio {
  width: 16px;
  height: 16px;
  margin-top: 2px;
  flex: none;
  border: 1.5px solid var(--lp-line-strong);
  border-radius: 50%;
  background: var(--lp-surface);
}
.ds-mode.is-active .ds-mode__radio { border: 5px solid var(--lp-accent); }
.ds-mode__text { display: flex; flex-direction: column; gap: 2px; }
.ds-mode__label { font-weight: 600; }
.ds-mode__desc { font-size: var(--lp-text-sm); color: var(--lp-text-dim); line-height: 1.5; }
.ds-html { max-width: 760px; margin-bottom: var(--lp-4); }
.ds-html__label { margin-bottom: 6px; font-size: var(--lp-text-sm); color: var(--lp-text-soft); }
.ds-note { margin-left: var(--lp-3); font-size: var(--lp-text-sm); color: var(--lp-text-dim); }

/* ---------- Create ---------- */
.kinds { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--lp-2); margin-bottom: var(--lp-4); }
.kind {
  display: flex; flex-direction: column; align-items: flex-start; gap: 4px;
  padding: var(--lp-3); border: 1px solid var(--lp-line); border-radius: var(--lp-r-lg);
  background: var(--lp-surface); color: inherit; font: inherit; text-align: left; cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s;
  &:hover { border-color: var(--lp-accent); }
  &.is-active { border-color: var(--lp-accent); box-shadow: 0 0 0 1px var(--lp-accent) inset; background: var(--lp-accent-soft); }
}
.kind__icon { color: var(--lp-accent); }
.kind__label { font-weight: 600; font-size: var(--lp-text-sm); }
.kind__desc { font-size: 11px; color: var(--lp-text-dim); line-height: 1.4; }
.create-form :deep(.el-form-item) { margin-bottom: var(--lp-4); }
.root-dir-row { display: flex; gap: var(--lp-2); width: 100%; .el-input { flex: 1; } }
.advanced-toggle {
  display: inline-flex; align-items: center; gap: 4px; margin: 0 0 var(--lp-3);
  padding: 0; border: 0; background: none; color: var(--lp-text-soft); font: inherit; font-size: var(--lp-text-sm); cursor: pointer;
  &:hover { color: var(--lp-accent); }
}

@media (max-width: 720px) {
  .kinds { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .search { width: 150px; }
  .guard { flex-direction: column; }
}
</style>
