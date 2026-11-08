<template>
  <section class="lp-card pv">
    <header class="lp-card-head">
      <h2 class="lp-card-title">
        {{ t('pv.title') }}
        <span class="count">{{ data?.installed.length ?? 0 }}</span>
      </h2>
      <div class="pv-head-actions">
        <el-dropdown v-if="data?.multi" trigger="click" :disabled="!installable.length" @command="installVersion">
          <el-button type="primary" size="small" :icon="Plus" :loading="busy === 'install'">{{ t('pv.install') }}</el-button>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item v-for="v in installable" :key="v" :command="v">PHP {{ v }}{{ parseFloat(v) <= 8.1 ? ` · ${t('pv.eol')}` : '' }}</el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>
    </header>
    <p class="pv-note">{{ data?.multi ? t('pv.hint') : t('pv.singleOnly') }}</p>

    <el-table v-if="data?.installed.length" :data="data.installed" size="small">
      <el-table-column :label="t('pv.version')" min-width="170">
        <template #default="{ row }">
          <div class="pv-ver">
            <span class="pv-ver__num">PHP {{ row.version }}</span>
            <span class="lp-dim lp-mono pv-ver__patch">{{ row.patch }}</span>
            <el-tag v-if="row.version === data.default" size="small" type="success">{{ t('pv.default') }}</el-tag>
          </div>
        </template>
      </el-table-column>
      <el-table-column :label="t('pv.fpm')" width="110">
        <template #default="{ row }">
          <span class="lp-pill" :class="row.running ? 'lp-pill--ok' : 'lp-pill--danger'">{{ row.running ? t('pv.running') : t('pv.stopped') }}</span>
        </template>
      </el-table-column>
      <el-table-column :label="t('pv.sites')" width="90" align="center">
        <template #default="{ row }">{{ siteCount(row.version) }}</template>
      </el-table-column>
      <el-table-column :label="t('pv.socket')" min-width="200">
        <template #default="{ row }"><span class="lp-mono pv-sock">{{ row.listen }}</span></template>
      </el-table-column>
      <el-table-column :label="t('actions')" width="330" fixed="right">
        <template #default="{ row }">
          <el-button v-if="row.version !== data.default" link type="primary" size="small" @click="makeDefault(row.version)">{{ t('pv.setDefault') }}</el-button>
          <el-button link type="primary" size="small" @click="openExtensions(row.version)">{{ t('pv.extensions') }}</el-button>
          <el-button link type="primary" size="small" @click="openSettings(row.version)">{{ t('pv.settings') }}</el-button>
          <el-button link type="primary" size="small" @click="editIni(row)">php.ini</el-button>
          <el-button link type="primary" size="small" :loading="busy === `restart-${row.version}`" @click="restart(row.version)">{{ t('pv.restart') }}</el-button>
          <el-button v-if="data.multi" link type="danger" size="small" @click="uninstall(row.version)">{{ t('pv.uninstall') }}</el-button>
        </template>
      </el-table-column>
    </el-table>

    <!-- Extensions -->
    <el-drawer v-model="extOpen" :title="t('pv.extTitle', { v: extVersion })" size="min(560px, 96vw)" append-to-body>
      <el-input v-model="extKeyword" size="small" clearable :prefix-icon="Search" :placeholder="t('pv.extSearch')" class="pv-ext-search" />
      <div v-loading="extLoading" class="pv-ext-list">
        <div v-for="e in filteredExt" :key="e.name" class="pv-ext">
          <span class="lp-mono pv-ext__name">{{ e.name }}</span>
          <span v-if="e.loaded" class="lp-pill lp-pill--ok">{{ t('pv.loaded') }}</span>
          <span class="pv-ext__spacer"></span>
          <el-button v-if="e.installed" size="small" type="danger" plain :loading="busy === `ext-${e.name}`" @click="toggleExt(e, false)">{{ t('pv.uninstall') }}</el-button>
          <el-button v-else size="small" type="primary" plain :loading="busy === `ext-${e.name}`" @click="toggleExt(e, true)">{{ t('pv.installExt') }}</el-button>
        </div>
        <el-empty v-if="!extLoading && !filteredExt.length" :image-size="56" />
      </div>
    </el-drawer>

    <!-- Settings -->
    <el-dialog v-model="setOpen" :title="t('pv.setTitle', { v: setVersion })" width="640px" append-to-body top="5vh">
      <el-form v-loading="setLoading" label-position="top" class="pv-form" @submit.prevent>
        <h4 class="pv-h">php.ini</h4>
        <div class="pv-grid">
          <el-form-item v-for="key in SIZE_KEYS" :key="key" :label="key">
            <el-input v-model="ini[key]" class="lp-mono" placeholder="128M" />
          </el-form-item>
          <el-form-item v-for="key in NUM_KEYS" :key="key" :label="key">
            <el-input v-model="ini[key]" class="lp-mono" />
          </el-form-item>
          <el-form-item label="date.timezone">
            <el-input v-model="ini['date.timezone']" class="lp-mono" placeholder="Asia/Shanghai" />
          </el-form-item>
          <el-form-item label="display_errors">
            <el-switch v-model="ini.display_errors" active-value="On" inactive-value="Off" />
          </el-form-item>
          <el-form-item label="short_open_tag">
            <el-switch v-model="ini.short_open_tag" active-value="On" inactive-value="Off" />
          </el-form-item>
        </div>
        <el-form-item label="disable_functions">
          <el-input v-model="ini.disable_functions" type="textarea" :rows="2" class="lp-mono" placeholder="exec,passthru,shell_exec,system" />
          <span class="hint">{{ t('pv.disableHint') }}</span>
        </el-form-item>
        <h4 class="pv-h">{{ t('pv.pool') }}</h4>
        <div class="pv-grid">
          <el-form-item label="pm">
            <el-select v-model="pool.pm">
              <el-option value="dynamic" label="dynamic" />
              <el-option value="static" label="static" />
              <el-option value="ondemand" label="ondemand" />
            </el-select>
          </el-form-item>
          <el-form-item v-for="key in POOL_NUM" :key="key" :label="key">
            <el-input v-model="pool[key]" class="lp-mono" />
          </el-form-item>
        </div>
        <span class="hint">{{ t('pv.poolHint') }}</span>
      </el-form>
      <template #footer>
        <el-button @click="setOpen = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="saveSettings">{{ t('save') }}</el-button>
      </template>
    </el-dialog>

    <FileEditor v-if="iniMounted" v-model="iniOpen" :path="iniPath" @saved="restart(iniVersion)" />
  </section>
</template>

<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus, Search } from '@element-plus/icons-vue'
import {
  getPhpExtensions, getPhpOverview, getPhpSettings, installPhp, restartPhp, savePhpSettings, setPhpDefault, setPhpExtension, uninstallPhp,
} from '@/api'
import { useI18n } from '@/i18n'

const FileEditor = defineAsyncComponent(() => import('@/components/editor/FileEditor.vue'))
type Installed = { version: string; patch: string; running: boolean; listen: string; ini: string; service: string }
type Overview = { multi: boolean; versions: string[]; installed: Installed[]; default: string; sites: Record<string, number> }
type Ext = { name: string; package: string; installed: boolean; loaded: boolean }

const SIZE_KEYS = ['memory_limit', 'upload_max_filesize', 'post_max_size']
const NUM_KEYS = ['max_execution_time', 'max_input_time', 'max_input_vars']
const POOL_NUM = ['pm.max_children', 'pm.start_servers', 'pm.min_spare_servers', 'pm.max_spare_servers', 'pm.max_requests']

const emit = defineEmits<{ (e: 'changed'): void }>()
const { t } = useI18n()
const data = ref<Overview | null>(null)
const busy = ref('')

const installable = computed(() => {
  const have = new Set((data.value?.installed || []).map((i) => i.version))
  return (data.value?.versions || []).filter((v) => !have.has(v))
})
function siteCount(version: string) {
  const s = data.value?.sites || {}
  return (s[version] || 0) + (version === data.value?.default ? s[''] || 0 : 0)
}

async function load() {
  const res: any = await getPhpOverview().catch(() => null)
  if (res?.success) data.value = res.data
}

async function installVersion(version: string) {
  busy.value = 'install'
  try {
    const res: any = await installPhp(version)
    if (res.success && res.data?.success) ElMessage.success(t('pv.installed', { v: version }))
  } finally {
    busy.value = ''
    await load()
    emit('changed')
  }
}
async function uninstall(version: string) {
  try {
    await ElMessageBox.confirm(t('pv.uninstallConfirm', { v: version }), t('pv.uninstall'), {
      type: 'warning', confirmButtonText: t('pv.uninstall'), confirmButtonClass: 'el-button--danger',
    })
  } catch {
    return
  }
  const res: any = await uninstallPhp(version).catch(() => null)
  if (res?.success && res.data?.success) ElMessage.success(t('pv.removed', { v: version }))
  await load()
  emit('changed')
}
async function makeDefault(version: string) {
  const res: any = await setPhpDefault(version).catch(() => null)
  if (res?.success) ElMessage.success(t('pv.defaultSet', { v: version }))
  await load()
}
async function restart(version: string) {
  busy.value = `restart-${version}`
  const res: any = await restartPhp(version).catch(() => null)
  busy.value = ''
  if (res?.success) ElMessage.success(t('pv.restarted', { v: version }))
  await load()
}

// Extensions
const extOpen = ref(false)
const extVersion = ref('')
const extList = ref<Ext[]>([])
const extLoading = ref(false)
const extKeyword = ref('')
const filteredExt = computed(() => {
  const q = extKeyword.value.trim().toLowerCase()
  return q ? extList.value.filter((e) => e.name.includes(q)) : extList.value
})
async function loadExt() {
  extLoading.value = true
  const res: any = await getPhpExtensions(extVersion.value).catch(() => null)
  extLoading.value = false
  if (res?.success) extList.value = res.data
}
function openExtensions(version: string) {
  extVersion.value = version
  extKeyword.value = ''
  extList.value = []
  extOpen.value = true
  void loadExt()
}
async function toggleExt(e: Ext, install: boolean) {
  busy.value = `ext-${e.name}`
  try {
    const res: any = await setPhpExtension(extVersion.value, e.name, install)
    if (res.success && res.data?.success) ElMessage.success(t(install ? 'pv.extInstalled' : 'pv.extRemoved', { name: e.name }))
  } finally {
    busy.value = ''
    await loadExt()
  }
}

// Settings
const setOpen = ref(false)
const setVersion = ref('')
const setLoading = ref(false)
const saving = ref(false)
const ini = reactive<Record<string, string>>({})
const pool = reactive<Record<string, string>>({})
async function openSettings(version: string) {
  setVersion.value = version
  setOpen.value = true
  setLoading.value = true
  const res: any = await getPhpSettings(version).catch(() => null)
  setLoading.value = false
  if (res?.success) {
    for (const k of Object.keys(ini)) delete ini[k]
    for (const k of Object.keys(pool)) delete pool[k]
    Object.assign(ini, { display_errors: 'Off', short_open_tag: 'Off', ...res.data.ini })
    Object.assign(pool, res.data.pool)
  }
}
async function saveSettings() {
  saving.value = true
  try {
    const res: any = await savePhpSettings(setVersion.value, { ini: { ...ini }, pool: { ...pool } })
    if (res.success) {
      ElMessage.success(t('pv.saved'))
      setOpen.value = false
    }
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

// php.ini in the code editor
const iniMounted = ref(false)
const iniOpen = ref(false)
const iniPath = ref('')
const iniVersion = ref('')
function editIni(row: Installed) {
  iniPath.value = row.ini
  iniVersion.value = row.version
  iniMounted.value = true
  iniOpen.value = true
}

onMounted(load)
defineExpose({ load })
</script>

<style scoped lang="scss">
.pv { margin-bottom: var(--lp-3); }
.pv-head-actions { display: flex; gap: 8px; }
.pv-note { margin: 0; padding: 0 var(--lp-4) var(--lp-3); font-size: 12px; color: var(--lp-text-dim); }
.pv-ver { display: flex; align-items: center; gap: 8px; }
.pv-ver__num { font-weight: 600; }
.pv-ver__patch { font-size: 12px; }
.pv-sock { font-size: 12px; }
.count { margin-left: 6px; padding: 0 6px; border-radius: 999px; background: var(--lp-muted-soft); font-size: 11px; }
.pv-ext-search { margin-bottom: 10px; }
.pv-ext-list { display: flex; flex-direction: column; gap: 2px; min-height: 120px; }
.pv-ext { display: flex; align-items: center; gap: 8px; padding: 6px 4px; border-bottom: 1px solid var(--lp-line); }
.pv-ext__name { font-size: 13px; }
.pv-ext__spacer { flex: 1; }
.pv-h { margin: 0 0 8px; font-size: 13px; }
.pv-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 0 12px; }
.hint { display: block; margin-top: 4px; font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
</style>
