<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.appstore') }}</h1>
        <p class="lp-page-sub">{{ t('as.subtitle') }}</p>
      </div>
      <div class="lp-page-actions">
        <el-input v-model="keyword" size="small" clearable :prefix-icon="Search" :placeholder="t('as.search')" class="as-search" />
        <el-button size="small" :icon="Refresh" :loading="loading" @click="load">{{ t('refresh') }}</el-button>
      </div>
    </div>

    <el-tabs v-model="tab">
      <!-- Catalog -->
      <el-tab-pane :label="t('as.apps')" name="apps">
        <div class="as-cats">
          <button
            v-for="cat in categories"
            :key="cat"
            type="button"
            class="as-cat"
            :class="{ 'is-active': category === cat }"
            @click="category = cat"
          >{{ t(`as.cat.${cat}`) }}</button>
        </div>

        <div v-if="catalog && showsSites" class="prereq-bar">
          <span class="prereq-bar__label">{{ t('as.environment') }}</span>
          <router-link v-for="item in prereqItems" :key="item.key" :to="item.to" class="prereq" :class="{ 'is-ok': item.ok }">
            <el-icon><CircleCheckFilled v-if="item.ok" /><WarningFilled v-else /></el-icon>
            {{ item.label }}
          </router-link>
        </div>

        <el-skeleton v-if="!catalog && loading" animated :rows="6" />
        <div class="app-grid">
          <article v-for="card in cards" :key="card.key" class="app-card">
            <div class="app-card__head">
              <span class="app-logo" :style="{ background: LOGO_COLORS[card.key] || '#475569' }">{{ LOGO_TEXT[card.key] || card.name.slice(0, 2) }}</span>
              <div class="app-card__title">
                <span class="app-card__name">{{ card.name }}</span>
                <a :href="card.homepage" target="_blank" rel="noopener" class="app-card__link">{{ card.homepage.replace(/^https?:\/\//, '') }}</a>
              </div>
              <span v-if="card.installedService" class="lp-pill" :class="card.installedService.running ? 'lp-pill--ok' : 'lp-pill--warn'">
                {{ card.installedService.running ? t('as.running') : t('as.stopped') }}
              </span>
            </div>
            <p class="app-card__desc">{{ t(`as.desc.${card.key}`) }}</p>
            <div class="app-card__needs">
              <template v-if="card.kind === 'site'">
                <el-tag size="small" type="info">OpenResty</el-tag>
                <el-tag size="small" type="info">PHP</el-tag>
                <el-tag v-if="card.site?.needs_mysql" size="small" type="info">MySQL</el-tag>
              </template>
              <template v-else>
                <el-tag size="small" type="success">{{ t('as.standalone') }}</el-tag>
                <el-tag v-for="port in card.service?.ports || []" :key="port.role" size="small" type="info">
                  {{ t(`as.port.${port.role}`) }} {{ port.default }}
                </el-tag>
              </template>
            </div>
            <div class="app-card__foot">
              <template v-if="card.kind === 'site'">
                <span v-if="missingFor(card.site!).length" class="app-card__missing">
                  {{ t('as.missing', { items: missingFor(card.site!).join(' / ') }) }}
                </span>
                <el-button type="primary" size="small" :disabled="missingFor(card.site!).length > 0" @click="openDeploy(card.site!)">
                  {{ t('as.deploy') }}
                </el-button>
              </template>
              <template v-else-if="card.installedService">
                <span class="app-card__ver lp-mono">{{ card.installedService.version }}</span>
                <el-button size="small" @click="tab = 'installed'">{{ t('as.manage') }}</el-button>
              </template>
              <el-button v-else type="primary" size="small" @click="openInstall(card.service!)">{{ t('as.install') }}</el-button>
            </div>
          </article>
        </div>
        <el-empty v-if="catalog && !cards.length" :description="t('as.noMatch')" />
      </el-tab-pane>

      <!-- Installed -->
      <el-tab-pane name="installed">
        <template #label>
          {{ t('as.installed') }}
          <span v-if="services.length + installed.length" class="as-count">{{ services.length + installed.length }}</span>
        </template>

        <section class="lp-card as-section">
          <h3 class="as-section__title">{{ t('as.servicesTitle') }}</h3>
          <el-table :data="services" :empty-text="t('as.noServices')">
            <el-table-column :label="t('as.app')" min-width="170">
              <template #default="{ row }">
                <div class="svc-name">
                  <span class="app-logo app-logo--sm" :style="{ background: LOGO_COLORS[row.app] || '#475569' }">{{ LOGO_TEXT[row.app] || row.name.slice(0, 2) }}</span>
                  <div>
                    <div class="strong">{{ row.name }}</div>
                    <div class="lp-dim lp-mono svc-ver">{{ row.version }}</div>
                  </div>
                </div>
              </template>
            </el-table-column>
            <el-table-column :label="t('as.status')" width="120">
              <template #default="{ row }">
                <span v-if="row.missing" class="lp-pill lp-pill--err">{{ t('as.appMissing') }}</span>
                <span v-else class="lp-pill" :class="row.running ? 'lp-pill--ok' : 'lp-pill--warn'" :title="row.last_error">
                  {{ row.running ? t('as.running') : t('as.stopped') }}
                </span>
              </template>
            </el-table-column>
            <el-table-column :label="t('as.access')" min-width="220">
              <template #default="{ row }">
                <a :href="serviceUrl(row)" target="_blank" rel="noopener" class="lp-mono">{{ serviceUrl(row) }}</a>
                <div class="lp-dim svc-ports">
                  <span v-for="(port, i) in portsOf(row)" :key="i">
                    <template v-if="port">{{ t(`as.port.${portRole(row, i)}`) }} {{ port }}</template>
                  </span>
                </div>
              </template>
            </el-table-column>
            <el-table-column :label="t('as.directory')" min-width="180">
              <template #default="{ row }">
                <router-link :to="{ path: '/files', query: { path: row.dir } }" class="lp-mono">{{ row.dir }}</router-link>
              </template>
            </el-table-column>
            <el-table-column :label="t('actions')" width="300" fixed="right">
              <template #default="{ row }">
                <el-button link type="primary" size="small" @click="showInfo(row)">{{ t('as.info.title') }}</el-button>
                <template v-if="!row.missing">
                  <el-button v-if="row.running" link type="warning" size="small" @click="act(row, 'stop')">{{ t('as.stop') }}</el-button>
                  <el-button v-else link type="success" size="small" @click="act(row, 'start')">{{ t('as.start') }}</el-button>
                  <el-button link type="primary" size="small" @click="act(row, 'restart')">{{ t('as.restart') }}</el-button>
                  <el-button link type="primary" size="small" @click="openLogs(row)">{{ t('as.logs') }}</el-button>
                  <el-button link type="primary" size="small" @click="openUpgrade(row)">{{ t('as.upgrade') }}</el-button>
                </template>
                <el-button link type="danger" size="small" @click="openUninstall(row)">{{ t('as.uninstall') }}</el-button>
              </template>
            </el-table-column>
          </el-table>
        </section>

        <section class="lp-card as-section">
          <h3 class="as-section__title">{{ t('as.sitesTitle') }}</h3>
          <el-table :data="installed" :empty-text="t('as.noInstalled')">
            <el-table-column :label="t('as.app')" width="140">
              <template #default="{ row }">{{ appName(row.app) }}</template>
            </el-table-column>
            <el-table-column :label="t('as.domain')" min-width="180">
              <template #default="{ row }">
                <a :href="`http://${row.domain}/`" target="_blank" rel="noopener" class="lp-mono">{{ row.domain }}</a>
              </template>
            </el-table-column>
            <el-table-column :label="t('as.docRoot')" min-width="220">
              <template #default="{ row }"><span class="lp-mono">{{ row.doc_root }}</span></template>
            </el-table-column>
            <el-table-column :label="t('as.database')" width="140">
              <template #default="{ row }"><span class="lp-mono">{{ row.db_name || '—' }}</span></template>
            </el-table-column>
            <el-table-column :label="t('as.createdAt')" width="170" prop="created_at" />
            <el-table-column :label="t('actions')" width="110">
              <template #default="{ row }">
                <el-button link type="primary" size="small" @click="router.push(`/sites/${row.site_id}`)">{{ t('as.manageSite') }}</el-button>
              </template>
            </el-table-column>
          </el-table>
        </section>
      </el-tab-pane>
    </el-tabs>

    <!-- Deploy a website program -->
    <el-dialog v-model="deployDialog" :title="t('as.deployTitle', { name: target?.name || '' })" width="560px" append-to-body>
      <el-form label-position="top" @submit.prevent="submit">
        <el-form-item :label="t('as.domains')">
          <el-input v-model="form.domains" type="textarea" :rows="2" class="lp-mono" placeholder="blog.example.com" @input="syncDefaults" />
          <span class="hint">{{ t('as.domainsHint') }}</span>
        </el-form-item>
        <el-form-item :label="t('as.docRoot')">
          <el-input v-model="form.docRoot" class="lp-mono" @input="docRootEdited = true" />
        </el-form-item>
        <template v-if="target?.needs_mysql">
          <el-form-item :label="t('as.dbName')">
            <el-input v-model="form.dbName" class="lp-mono" @input="dbEdited = true" />
            <span class="hint">{{ t('as.dbHint') }}</span>
          </el-form-item>
        </template>
        <el-collapse>
          <el-collapse-item :title="t('as.advanced')" name="adv">
            <el-form-item :label="t('as.sourceUrl')">
              <el-input v-model="form.sourceUrl" class="lp-mono" :placeholder="t('as.sourceUrlPlaceholder')" />
              <span class="hint">{{ t('as.sourceUrlHint') }}</span>
            </el-form-item>
          </el-collapse-item>
        </el-collapse>
      </el-form>
      <template #footer>
        <el-button @click="deployDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="deploying" @click="submit">{{ t('as.deploy') }}</el-button>
      </template>
    </el-dialog>

    <!-- Install a service -->
    <el-dialog v-model="installDialog" :title="t('as.installTitle', { name: svc?.name || '' })" width="600px" append-to-body top="6vh">
      <el-alert v-if="svc" type="info" :closable="false" show-icon class="svc-note" :title="t(`as.note.${svc.key}`)" />
      <el-form v-if="svc" label-position="top" class="svc-form" @submit.prevent="submitInstall">
        <div class="svc-row">
          <el-form-item :label="t('as.version')" class="svc-grow">
            <el-input v-model="svcForm.version" class="lp-mono" :placeholder="t('as.versionLatest')" />
          </el-form-item>
          <el-form-item v-for="(port, i) in svc.ports" :key="port.role" :label="t(`as.port.${port.role}`)">
            <el-input-number v-model="svcForm.ports[i]" :min="port.optional ? 0 : 1024" :max="65535" controls-position="right" class="svc-port" />
          </el-form-item>
        </div>
        <p v-if="svc.ports.some((p) => p.optional)" class="hint hint--top">{{ t(`as.portHint.${svc.key}`) }}</p>

        <el-form-item :label="t('as.accessMode')">
          <el-radio-group v-model="svcForm.public">
            <el-radio :value="true">{{ t('as.accessPublic') }}</el-radio>
            <el-radio :value="false">{{ t('as.accessDomainOnly') }}</el-radio>
          </el-radio-group>
        </el-form-item>
        <el-form-item :label="svcForm.public ? t('as.domainOptional') : t('as.domain')">
          <el-input v-model="svcForm.domain" class="lp-mono" placeholder="files.example.com" />
          <span class="hint">{{ catalog?.prerequisites.openresty ? t('as.domainProxyHint') : t('as.domainNeedsOpenresty') }}</span>
        </el-form-item>

        <template v-if="svc.root_dir">
          <el-form-item :label="t('as.rootDir')">
            <div class="svc-dir">
              <el-input v-model="svcForm.rootDir" class="lp-mono" />
              <el-button @click="dirPickerOpen = true">{{ t('as.browse') }}</el-button>
            </div>
          </el-form-item>
        </template>
        <el-form-item v-if="svc.user_choice" :label="t('as.runUser')">
          <el-select v-model="svcForm.runUser" filterable class="svc-user">
            <el-option v-for="user in systemUsers" :key="user" :label="user" :value="user" />
          </el-select>
          <span class="hint">{{ t('as.runUserHint') }}</span>
        </el-form-item>
        <el-form-item v-if="catalog?.prerequisites.firewall">
          <el-checkbox v-model="svcForm.openFirewall">{{ t('as.openFirewall', { ports: firewallPorts.join(', ') }) }}</el-checkbox>
        </el-form-item>
        <p v-else class="hint">{{ t('as.cloudFirewallHint', { ports: firewallPorts.join(', ') }) }}</p>

        <el-collapse>
          <el-collapse-item :title="t('as.advanced')" name="adv">
            <el-form-item :label="t('as.sourceUrl')">
              <el-input v-model="svcForm.sourceUrl" class="lp-mono" :placeholder="t('as.svcSourcePlaceholder')" />
              <span class="hint">{{ t('as.svcSourceHint') }}</span>
            </el-form-item>
          </el-collapse-item>
        </el-collapse>
      </el-form>
      <template #footer>
        <el-button @click="installDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="installing" @click="submitInstall">{{ t('as.install') }}</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="dirPickerOpen" :title="t('as.rootDir')" width="760px" destroy-on-close append-to-body>
      <div class="dir-picker-shell">
        <DirPicker :init-path="svcForm.rootDir || '/www/wwwroot'" @select="(path: string) => { svcForm.rootDir = path || svcForm.rootDir; dirPickerOpen = false }" />
      </div>
    </el-dialog>

    <!-- Access details -->
    <el-dialog v-model="infoDialog" :title="infoTitle" width="560px" append-to-body>
      <dl class="svc-info">
        <template v-for="item in infoItems" :key="item.key">
          <dt>{{ t(`as.info.${item.key}`) }}</dt>
          <dd>
            <a v-if="/^https?:\/\//.test(item.value)" :href="item.value" target="_blank" rel="noopener" class="lp-mono">{{ item.value }}</a>
            <span v-else class="lp-mono">{{ item.secret && !revealed[item.key] ? '••••••••••••' : item.value }}</span>
            <el-button v-if="item.secret" link size="small" @click="revealed[item.key] = !revealed[item.key]">
              {{ revealed[item.key] ? t('as.hide') : t('as.show') }}
            </el-button>
            <el-button link size="small" type="primary" @click="copy(item.value)">{{ t('as.copy') }}</el-button>
          </dd>
        </template>
      </dl>
      <p v-if="infoNext" class="hint">{{ infoNext }}</p>
      <template #footer>
        <el-button type="primary" @click="infoDialog = false">{{ t('close') }}</el-button>
      </template>
    </el-dialog>

    <!-- Logs -->
    <el-dialog v-model="logsDialog" :title="logsTitle" width="820px" top="6vh">
      <div v-loading="logsLoading">
        <pre class="svc-log">{{ logsText || t('rt.noLogs') }}</pre>
      </div>
      <template #footer>
        <el-button :icon="Refresh" @click="loadLogs">{{ t('refresh') }}</el-button>
        <el-button type="primary" @click="logsDialog = false">{{ t('close') }}</el-button>
      </template>
    </el-dialog>

    <!-- Upgrade -->
    <el-dialog v-model="upgradeDialog" :title="t('as.upgradeTitle', { name: upgradeTarget?.name || '' })" width="480px" append-to-body>
      <p class="hint hint--top">{{ t('as.upgradeHint', { version: upgradeTarget?.version || '' }) }}</p>
      <el-form label-position="top" @submit.prevent="submitUpgrade">
        <el-form-item :label="t('as.version')">
          <el-input v-model="upgradeForm.version" class="lp-mono" :placeholder="t('as.versionLatest')" />
        </el-form-item>
        <el-form-item :label="t('as.sourceUrl')">
          <el-input v-model="upgradeForm.sourceUrl" class="lp-mono" :placeholder="t('as.svcSourcePlaceholder')" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="upgradeDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="upgrading" @click="submitUpgrade">{{ t('as.upgrade') }}</el-button>
      </template>
    </el-dialog>

    <!-- Uninstall -->
    <el-dialog v-model="uninstallDialog" :title="t('as.uninstallTitle', { name: uninstallTarget?.name || '' })" width="480px" append-to-body>
      <p class="hint hint--top">{{ t('as.uninstallHint') }}</p>
      <el-checkbox v-model="deleteData">{{ t('as.deleteData', { dir: uninstallTarget?.dir || '' }) }}</el-checkbox>
      <template #footer>
        <el-button @click="uninstallDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="danger" :loading="uninstalling" @click="submitUninstall">{{ t('as.uninstall') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { usePage } from '@/utils/page'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { CircleCheckFilled, Refresh, Search, WarningFilled } from '@element-plus/icons-vue'
import DirPicker from '@/components/DirPicker.vue'
import {
  appAction, deployApp, getAppCatalog, getAppLogs, getInstalledApps, getOwnerChoices, getServiceInstalls,
  installService, uninstallService, upgradeService,
} from '@/api'
import { useI18n } from '@/i18n'
import { copyText } from '@/utils/clipboard'

interface AppDef { key: string; name: string; needs_mysql: boolean; homepage: string }
interface PortDef { role: string; default: number; external: boolean; optional: boolean }
interface ServiceDef {
  key: string; name: string; homepage: string; repo: string; category: string
  ports: PortDef[]; default_user: string; user_choice: boolean; root_dir: boolean
}
interface Catalog {
  apps: AppDef[]
  services: ServiceDef[]
  prerequisites: { openresty: boolean; php: boolean; mysql: boolean; firewall: boolean }
}
interface InfoItem { key: string; value: string; secret?: boolean }
interface ServiceRow {
  id: number; app: string; name: string; version: string; dir: string; app_id: number; site_id: number | null
  ports: string; info: string; running: boolean; missing: boolean; last_error: string; domain: string | null
}
type Card = {
  key: string; name: string; homepage: string; kind: 'site' | 'service'; category: string
  site?: AppDef; service?: ServiceDef; installedService?: ServiceRow
}

const LOGO_TEXT: Record<string, string> = {
  wordpress: 'W', typecho: 'T', adminer: 'Ad', phpmyadmin: 'pma', dokuwiki: 'Dw',
  filebrowser: 'FB', frps: 'frp', 'ddns-go': 'DD', gitea: 'Gt',
}
const LOGO_COLORS: Record<string, string> = {
  wordpress: '#21759b', typecho: '#467b96', adminer: '#34495e', phpmyadmin: '#6c78af', dokuwiki: '#a0522d',
  filebrowser: '#40c4ff', frps: '#1f6feb', 'ddns-go': '#0e9f6e', gitea: '#609926',
}
const CATEGORIES = ['all', 'site', 'tools', 'network', 'dev'] as const

const { t, locale } = useI18n()
const router = useRouter()
const tab = ref('apps')
const loading = ref(false)
const catalog = ref<Catalog | null>(null)
const installed = ref<any[]>([])
const services = ref<ServiceRow[]>([])
const keyword = ref('')
const category = ref<(typeof CATEGORIES)[number]>('all')
const categories = CATEGORIES

const cards = computed<Card[]>(() => {
  const c = catalog.value
  if (!c) return []
  const all: Card[] = [
    ...c.apps.map((app) => ({ key: app.key, name: app.name, homepage: app.homepage, kind: 'site' as const, category: 'site', site: app })),
    ...c.services.map((svc) => ({
      key: svc.key, name: svc.name, homepage: svc.homepage, kind: 'service' as const, category: svc.category, service: svc,
      installedService: services.value.find((row) => row.app === svc.key),
    })),
  ]
  const q = keyword.value.trim().toLowerCase()
  return all.filter((card) =>
    (category.value === 'all' || card.category === category.value)
    && (!q || card.name.toLowerCase().includes(q) || card.key.includes(q) || t(`as.desc.${card.key}`).toLowerCase().includes(q)))
})
const showsSites = computed(() => category.value === 'all' || category.value === 'site')

const prereqItems = computed(() => {
  const p = catalog.value?.prerequisites
  return [
    { key: 'openresty', label: 'OpenResty', ok: !!p?.openresty, to: '/sites' },
    { key: 'php', label: 'PHP', ok: !!p?.php, to: '/runtime' },
    { key: 'mysql', label: 'MySQL', ok: !!p?.mysql, to: '/databases' },
  ]
})

function missingFor(app: AppDef) {
  return prereqItems.value
    .filter((item) => !item.ok && (item.key !== 'mysql' || app.needs_mysql))
    .map((item) => item.label)
}

function appName(key: string) {
  return catalog.value?.apps.find((a) => a.key === key)?.name || key
}

async function load() {
  loading.value = true
  try {
    const [catalogRes, installedRes, servicesRes]: any[] = await Promise.all([
      getAppCatalog(), getInstalledApps(), getServiceInstalls(),
    ])
    if (catalogRes.success) catalog.value = catalogRes.data
    if (installedRes.success) installed.value = installedRes.data
    if (servicesRes.success) services.value = servicesRes.data
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}

async function copy(text: string) {
  if (await copyText(text)) ElMessage.success(t('as.copied'))
  else ElMessage.warning(text)
}

// --- Deploy a website program ---
const deployDialog = ref(false)
const deploying = ref(false)
const target = ref<AppDef | null>(null)
const docRootEdited = ref(false)
const dbEdited = ref(false)
const form = reactive({ domains: '', docRoot: '', dbName: '', sourceUrl: '' })

function domainList() {
  return form.domains.split(/[\s,]+/).map((d) => d.trim().toLowerCase()).filter(Boolean)
}

/** Mirrors the server's default: blog.example.com -> blog_example. */
function dbNameFor(domain: string) {
  let name = domain.split('.').slice(0, 2).join('_').toLowerCase().replace(/[^a-z0-9]/g, '_')
  if (!/^[a-z]/.test(name)) name = `db_${name}`
  return name.slice(0, 16).replace(/_+$/, '')
}

function syncDefaults() {
  const primary = domainList()[0] || ''
  if (!docRootEdited.value) form.docRoot = primary ? `/www/wwwroot/${primary}` : ''
  if (!dbEdited.value) form.dbName = primary ? dbNameFor(primary) : ''
}

function openDeploy(app: AppDef) {
  target.value = app
  Object.assign(form, { domains: '', docRoot: '', dbName: '', sourceUrl: '' })
  docRootEdited.value = false
  dbEdited.value = false
  deployDialog.value = true
}

async function submit() {
  if (!target.value) return
  const domains = domainList()
  if (!domains.length) {
    ElMessage.warning(t('as.domainRequired'))
    return
  }
  const app = target.value
  deploying.value = true
  deployDialog.value = false
  try {
    const res: any = await deployApp({
      app: app.key,
      domains,
      doc_root: form.docRoot.trim(),
      db_name: form.dbName.trim(),
      source_url: form.sourceUrl.trim(),
      locale: locale.value,
    })
    if (!res.success || !res.data?.success) return
    const result = res.data
    const db = result.database
    const items: InfoItem[] = [{ key: 'url', value: result.url }]
    if (db) {
      items.push(
        { key: 'dbName', value: db.name },
        { key: 'dbUser', value: db.username },
        { key: 'dbPassword', value: db.password, secret: true },
        { key: 'dbHost', value: 'localhost' },
      )
    }
    openInfo(t('as.deployed'), items, t(`as.next.${app.key}`))
    await load()
  } catch {
    // Interceptor reports it.
  } finally {
    deploying.value = false
  }
}

// --- Install a service ---
const installDialog = ref(false)
const installing = ref(false)
const svc = ref<ServiceDef | null>(null)
const dirPickerOpen = ref(false)
const systemUsers = ref<string[]>([])
const svcForm = reactive({
  version: '', ports: [] as number[], public: true, domain: '', openFirewall: true,
  runUser: '', rootDir: '/www/wwwroot', sourceUrl: '',
})

/** Ports that must be reachable from outside with the chosen access mode. */
const firewallPorts = computed(() =>
  (svc.value?.ports || [])
    .map((spec, i) => ({ spec, port: svcForm.ports[i] }))
    .filter(({ spec, port }) => port > 0 && (spec.external || svcForm.public || !svcForm.domain.trim()))
    .map(({ port }) => port))

async function openInstall(def: ServiceDef) {
  svc.value = def
  Object.assign(svcForm, {
    version: '', ports: def.ports.map((p) => p.default), public: true, domain: '', openFirewall: true,
    runUser: def.default_user || 'root', rootDir: '/www/wwwroot', sourceUrl: '',
  })
  installDialog.value = true
  if (def.user_choice && !systemUsers.value.length) {
    const res: any = await getOwnerChoices().catch(() => null)
    if (res?.success) systemUsers.value = res.data.users || []
  }
}

async function submitInstall() {
  const def = svc.value
  if (!def) return
  const domain = svcForm.domain.trim()
  if (!svcForm.public && !domain) {
    ElMessage.warning(t('as.domainRequired'))
    return
  }
  installing.value = true
  installDialog.value = false
  try {
    const res: any = await installService({
      app: def.key,
      version: svcForm.version.trim(),
      ports: svcForm.ports.map((p) => Number(p) || 0),
      public: svcForm.public,
      domain,
      open_firewall: svcForm.openFirewall,
      run_user: def.user_choice ? svcForm.runUser : '',
      root_dir: svcForm.rootDir.trim(),
      source_url: svcForm.sourceUrl.trim(),
      access_host: window.location.hostname,
      locale: locale.value,
    })
    if (res.success && res.data?.success) {
      openInfo(t('as.installedTitle', { name: def.name, version: res.data.version }), res.data.info, t(`as.after.${def.key}`))
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    installing.value = false
  }
}

// --- Installed services ---
function portsOf(row: ServiceRow): number[] {
  try {
    return JSON.parse(row.ports)
  } catch {
    return []
  }
}
function portRole(row: ServiceRow, i: number) {
  return catalog.value?.services.find((s) => s.key === row.app)?.ports[i]?.role || 'web'
}
function infoOf(row: ServiceRow): InfoItem[] {
  try {
    return JSON.parse(row.info)
  } catch {
    return []
  }
}
function serviceUrl(row: ServiceRow) {
  if (row.domain) return `http://${row.domain}/`
  const host = window.location.hostname.includes(':') ? `[${window.location.hostname}]` : window.location.hostname
  return `http://${host}:${portsOf(row)[0]}/`
}

async function act(row: ServiceRow, action: 'start' | 'stop' | 'restart') {
  const res: any = await appAction(row.app_id, action).catch(() => null)
  if (res?.success) ElMessage.success(t(`as.done.${action}`))
  // The supervisor needs a moment to report the new state.
  setTimeout(load, action === 'stop' ? 300 : 1200)
}

// Access details
const infoDialog = ref(false)
const infoTitle = ref('')
const infoItems = ref<InfoItem[]>([])
const infoNext = ref('')
const revealed = reactive<Record<string, boolean>>({})
function openInfo(title: string, items: InfoItem[], next = '') {
  infoTitle.value = title
  infoItems.value = items
  infoNext.value = next
  for (const key of Object.keys(revealed)) delete revealed[key]
  infoDialog.value = true
}
function showInfo(row: ServiceRow) {
  const items = infoOf(row).map((item) =>
    // The address follows the current domain / port, not the one at install time.
    item.key === 'url' || item.key === 'dashboard' ? { ...item, value: serviceUrl(row) } : item)
  openInfo(`${row.name} ${row.version}`, items, t(`as.after.${row.app}`))
}

// Logs
const logsDialog = ref(false)
const logsLoading = ref(false)
const logsText = ref('')
const logsTitle = ref('')
const logsApp = ref(0)
async function loadLogs() {
  logsLoading.value = true
  try {
    const res: any = await getAppLogs(logsApp.value, 400)
    if (res.success) logsText.value = res.data?.log ?? res.data ?? ''
  } catch {
    // Interceptor reports it.
  } finally {
    logsLoading.value = false
  }
}
function openLogs(row: ServiceRow) {
  logsApp.value = row.app_id
  logsTitle.value = `${row.name} · ${t('as.logs')}`
  logsText.value = ''
  logsDialog.value = true
  void loadLogs()
}

// Upgrade
const upgradeDialog = ref(false)
const upgrading = ref(false)
const upgradeTarget = ref<ServiceRow | null>(null)
const upgradeForm = reactive({ version: '', sourceUrl: '' })
function openUpgrade(row: ServiceRow) {
  upgradeTarget.value = row
  Object.assign(upgradeForm, { version: '', sourceUrl: '' })
  upgradeDialog.value = true
}
async function submitUpgrade() {
  const row = upgradeTarget.value
  if (!row) return
  upgrading.value = true
  upgradeDialog.value = false
  try {
    const res: any = await upgradeService(row.id, row.name, {
      version: upgradeForm.version.trim(),
      source_url: upgradeForm.sourceUrl.trim(),
    })
    if (res.success && res.data?.success) ElMessage.success(t('as.upgraded', { name: row.name, version: res.data.version }))
    await load()
  } catch {
    // Interceptor reports it.
  } finally {
    upgrading.value = false
  }
}

// Uninstall
const uninstallDialog = ref(false)
const uninstalling = ref(false)
const uninstallTarget = ref<ServiceRow | null>(null)
const deleteData = ref(false)
function openUninstall(row: ServiceRow) {
  uninstallTarget.value = row
  deleteData.value = false
  uninstallDialog.value = true
}
async function submitUninstall() {
  const row = uninstallTarget.value
  if (!row) return
  if (deleteData.value) {
    try {
      await ElMessageBox.confirm(t('as.deleteDataConfirm', { dir: row.dir }), t('as.uninstall'), {
        type: 'warning', confirmButtonText: t('as.uninstall'), confirmButtonClass: 'el-button--danger',
      })
    } catch {
      return
    }
  }
  uninstalling.value = true
  try {
    const res: any = await uninstallService(row.id, deleteData.value)
    if (res.success) {
      uninstallDialog.value = false
      ElMessage.success(t('as.uninstalled', { name: row.name }))
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    uninstalling.value = false
  }
}

onMounted(load)
usePage(load)
</script>

<style scoped lang="scss">
.as-search { width: 220px; }
.as-cats { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: var(--lp-4); }
.as-cat {
  padding: 5px 14px;
  border: 1px solid var(--lp-line);
  border-radius: 999px;
  background: var(--lp-surface);
  color: var(--lp-text-dim);
  font: inherit;
  font-size: var(--lp-text-sm);
  cursor: pointer;
  &:hover { color: var(--lp-text); }
  &.is-active { border-color: var(--lp-accent); background: var(--lp-accent-soft); color: var(--lp-accent); }
}
.as-count {
  margin-left: 6px;
  padding: 0 6px;
  border-radius: 999px;
  background: var(--lp-muted-soft);
  font-size: 11px;
}
.prereq-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--lp-2) var(--lp-3);
  margin-bottom: var(--lp-4);
  font-size: var(--lp-text-sm);
}
.prereq-bar__label { color: var(--lp-text-dim); }
.prereq {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--lp-warn);
  text-decoration: none;
  &.is-ok { color: var(--lp-ok); }
}

.app-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: var(--lp-4);
}
.app-card {
  display: flex;
  flex-direction: column;
  gap: var(--lp-3);
  padding: var(--lp-4);
  background: var(--lp-surface);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  transition: border-color 0.15s, box-shadow 0.15s;
  &:hover { border-color: var(--lp-line-strong, var(--lp-line)); box-shadow: 0 4px 16px rgba(15, 23, 42, 0.06); }
}
.app-card__head { display: flex; align-items: center; gap: var(--lp-3); }
.app-logo {
  display: grid;
  place-items: center;
  flex-shrink: 0;
  width: 40px;
  height: 40px;
  border-radius: var(--lp-r-md);
  font-weight: 700;
  font-size: 13px;
  color: #fff;
}
.app-logo--sm { width: 30px; height: 30px; font-size: 11px; border-radius: var(--lp-r-sm); }
.app-card__title { display: flex; flex-direction: column; min-width: 0; flex: 1; }
.app-card__name { font-weight: 600; color: var(--lp-text); }
.app-card__link { font-size: var(--lp-text-xs); color: var(--lp-text-dim); text-decoration: none; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.app-card__desc { margin: 0; font-size: var(--lp-text-sm); color: var(--lp-text-dim); line-height: 1.6; flex: 1; }
.app-card__needs { display: flex; gap: var(--lp-1); flex-wrap: wrap; }
.app-card__foot { display: flex; align-items: center; justify-content: flex-end; gap: var(--lp-2); }
.app-card__missing { margin-right: auto; font-size: var(--lp-text-xs); color: var(--lp-warn); }
.app-card__ver { margin-right: auto; font-size: var(--lp-text-xs); color: var(--lp-text-dim); }

.as-section { margin-bottom: var(--lp-4); }
.as-section__title { margin: 0; padding: var(--lp-3) var(--lp-4); font-size: var(--lp-text-md, 15px); font-weight: 600; }
.svc-name { display: flex; align-items: center; gap: 10px; }
.svc-ver { font-size: 11px; }
.svc-ports { font-size: 11px; display: flex; gap: 10px; }
.strong { font-weight: 600; }

.svc-note { margin-bottom: var(--lp-3); }
.svc-row { display: flex; gap: var(--lp-3); flex-wrap: wrap; }
.svc-grow { flex: 1; min-width: 160px; }
.svc-port { width: 140px; }
.svc-dir { display: flex; gap: var(--lp-2); width: 100%; .el-input { flex: 1; } }
.svc-user { width: 220px; }
.dir-picker-shell { height: 460px; }

.svc-info {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 10px 16px;
  margin: 0;
  dt { color: var(--lp-text-dim); font-size: var(--lp-text-sm); line-height: 24px; }
  dd { margin: 0; display: flex; align-items: center; gap: 6px; flex-wrap: wrap; min-width: 0; word-break: break-all; line-height: 24px; }
}
.svc-log {
  margin: 0;
  max-height: 60vh;
  overflow: auto;
  padding: 12px;
  border-radius: var(--lp-r-sm);
  background: #0f172a;
  color: #e2e8f0;
  font-family: var(--lp-font-mono);
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-all;
}
.hint {
  display: block;
  flex-basis: 100%;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  line-height: 1.6;
}
.hint--top { margin: 0 0 var(--lp-3); }
@media (max-width: 760px) {
  .as-search { width: 100%; }
  .svc-info { grid-template-columns: 1fr; }
}
</style>
