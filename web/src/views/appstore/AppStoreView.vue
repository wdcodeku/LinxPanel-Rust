<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.appstore') }}</h1>
        <p class="lp-page-sub">{{ t('as.subtitle') }}</p>
      </div>
      <div class="lp-page-actions">
        <el-button size="small" :icon="Refresh" :loading="loading" @click="load">{{ t('refresh') }}</el-button>
      </div>
    </div>

    <div v-if="catalog" class="prereq-bar">
      <span class="prereq-bar__label">{{ t('as.environment') }}</span>
      <router-link v-for="item in prereqItems" :key="item.key" :to="item.to" class="prereq" :class="{ 'is-ok': item.ok }">
        <el-icon><CircleCheckFilled v-if="item.ok" /><WarningFilled v-else /></el-icon>
        {{ item.label }}
      </router-link>
    </div>

    <el-tabs v-model="tab">
      <el-tab-pane :label="t('as.apps')" name="apps">
        <div class="app-grid">
          <article v-for="app in catalog?.apps || []" :key="app.key" class="app-card">
            <div class="app-card__head">
              <span class="app-logo" :data-app="app.key">{{ app.name.slice(0, 2) }}</span>
              <div class="app-card__title">
                <span class="app-card__name">{{ app.name }}</span>
                <a :href="app.homepage" target="_blank" rel="noopener" class="app-card__link">{{ app.homepage.replace('https://', '') }}</a>
              </div>
            </div>
            <p class="app-card__desc">{{ t(`as.desc.${app.key}`) }}</p>
            <div class="app-card__needs">
              <el-tag size="small" type="info">OpenResty</el-tag>
              <el-tag size="small" type="info">PHP</el-tag>
              <el-tag v-if="app.needs_mysql" size="small" type="info">MySQL</el-tag>
            </div>
            <div class="app-card__foot">
              <span v-if="missingFor(app).length" class="app-card__missing">
                {{ t('as.missing', { items: missingFor(app).join(' / ') }) }}
              </span>
              <el-button type="primary" size="small" :disabled="missingFor(app).length > 0" @click="openDeploy(app)">
                {{ t('as.deploy') }}
              </el-button>
            </div>
          </article>
        </div>
      </el-tab-pane>

      <el-tab-pane :label="t('as.installed')" name="installed">
        <section class="lp-card">
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

    <OperationLogDialog
      v-model="logDialog.visible"
      :title="logDialog.title"
      :description="logDialog.description"
      :log="logDialog.log"
      :status="logDialog.status"
      :loading="logDialog.status === 'running'"
      :show-clear="false"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessageBox } from 'element-plus'
import { CircleCheckFilled, Refresh, WarningFilled } from '@element-plus/icons-vue'
import OperationLogDialog from '@/components/OperationLogDialog.vue'
import { deployApp, getAppCatalog, getInstalledApps } from '@/api'
import { useI18n } from '@/i18n'

interface AppDef { key: string; name: string; needs_mysql: boolean; homepage: string }
interface Catalog { apps: AppDef[]; prerequisites: { openresty: boolean; php: boolean; mysql: boolean } }

const { t, locale } = useI18n()
const router = useRouter()
const tab = ref('apps')
const loading = ref(false)
const catalog = ref<Catalog | null>(null)
const installed = ref<any[]>([])

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
    const [catalogRes, installedRes]: any[] = await Promise.all([getAppCatalog(), getInstalledApps()])
    if (catalogRes.success) catalog.value = catalogRes.data
    if (installedRes.success) installed.value = installedRes.data
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}

// --- Deploy ---
const deployDialog = ref(false)
const deploying = ref(false)
const target = ref<AppDef | null>(null)
const docRootEdited = ref(false)
const dbEdited = ref(false)
const form = reactive({ domains: '', docRoot: '', dbName: '', sourceUrl: '' })
const logDialog = reactive({
  visible: false, title: '', description: '', log: '', status: 'idle' as 'idle' | 'running' | 'success' | 'error',
})

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
  if (!domains.length) return
  deploying.value = true
  Object.assign(logDialog, {
    visible: true,
    title: t('as.deployTitle', { name: target.value.name }),
    description: '',
    log: t('as.deploying'),
    status: 'running',
  })
  deployDialog.value = false
  try {
    const res: any = await deployApp({
      app: target.value.key,
      domains,
      doc_root: form.docRoot.trim(),
      db_name: form.dbName.trim(),
      source_url: form.sourceUrl.trim(),
      locale: locale.value,
    })
    if (!res.success) {
      logDialog.log = res.message
      logDialog.status = 'error'
      return
    }
    const result = res.data
    logDialog.log = result.log
    logDialog.status = result.success ? 'success' : 'error'
    if (result.success) {
      logDialog.description = t('as.doneHint', { url: result.url })
      const db = result.database
      ElMessageBox.alert(
        [
          `${t('as.siteUrl')}: ${result.url}`,
          db ? `${t('as.dbName')}: ${db.name}` : '',
          db ? `${t('dbm.user')}: ${db.username}` : '',
          db ? `${t('dbm.password')}: ${db.password}` : '',
          db ? `${t('as.dbHostLabel')}: localhost` : '',
          '',
          t(`as.next.${target.value.key}`),
        ].filter((line, i, all) => line || all[i + 1]).join('\n'),
        t('as.deployed'),
        { customClass: 'credential-box' },
      )
      await load()
    }
  } catch {
    logDialog.status = 'error'
  } finally {
    deploying.value = false
  }
}

onMounted(load)
</script>

<style scoped lang="scss">
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
}
.app-card__head { display: flex; align-items: center; gap: var(--lp-3); }
.app-logo {
  display: grid;
  place-items: center;
  width: 40px;
  height: 40px;
  border-radius: var(--lp-r-md);
  font-weight: 700;
  color: #fff;
  background: #21759b;
  &[data-app='typecho'] { background: #467b96; }
  &[data-app='adminer'] { background: #34495e; }
}
.app-card__title { display: flex; flex-direction: column; min-width: 0; }
.app-card__name { font-weight: 600; color: var(--lp-text); }
.app-card__link { font-size: var(--lp-text-xs); color: var(--lp-text-dim); text-decoration: none; }
.app-card__desc { margin: 0; font-size: var(--lp-text-sm); color: var(--lp-text-dim); line-height: 1.6; flex: 1; }
.app-card__needs { display: flex; gap: var(--lp-1); flex-wrap: wrap; }
.app-card__foot { display: flex; align-items: center; justify-content: flex-end; gap: var(--lp-2); }
.app-card__missing { margin-right: auto; font-size: var(--lp-text-xs); color: var(--lp-warn); }

.hint {
  display: block;
  flex-basis: 100%;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
}
</style>
