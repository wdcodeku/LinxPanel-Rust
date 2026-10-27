<template>
  <div>
  <section class="lp-card">
    <header class="lp-card-head">
      <h2 class="lp-card-title">
        <el-icon><Coin /></el-icon>{{ t('sql.title') }}
        <span v-if="loaded" class="count">{{ files.length }}</span>
      </h2>
      <div class="head-tools">
        <el-input v-model="keyword" :prefix-icon="Search" size="small" clearable :placeholder="t('sql.search')" class="search" />
        <el-button size="small" :icon="Refresh" :loading="loading" @click="load">{{ t('refresh') }}</el-button>
        <el-button size="small" :icon="Aim" :loading="scanning" @click="discover">{{ t('sql.discover') }}</el-button>
        <el-button size="small" @click="openPath('register')">{{ t('sql.addExisting') }}</el-button>
        <el-button type="primary" size="small" :icon="Plus" @click="openPath('create')">{{ t('sql.create') }}</el-button>
      </div>
    </header>
    <p class="intro">
      <el-icon><InfoFilled /></el-icon>{{ t('sql.intro') }}
    </p>
    <div v-if="!loaded" class="lp-card-body"><el-skeleton animated :rows="3" /></div>
    <div v-else-if="!files.length" class="empty">
      <el-icon :size="34" class="empty__icon"><Coin /></el-icon>
      <p class="empty__title">{{ t('sql.empty') }}</p>
      <p class="lp-dim empty__hint">{{ t('sql.emptyHint') }}</p>
      <div class="empty__actions">
        <el-button :icon="Aim" :loading="scanning" @click="discover">{{ t('sql.discover') }}</el-button>
        <el-button type="primary" :icon="Plus" @click="openPath('create')">{{ t('sql.create') }}</el-button>
      </div>
    </div>
    <el-table v-else :data="filtered" :empty-text="t('dbm.noMatch')">
      <el-table-column :label="t('sql.file')" min-width="280">
        <template #default="{ row }">
          <button type="button" class="db-name" :title="t('dbm.viewTables')" :disabled="!row.exists" @click="openTables(row)">
            <span class="lp-mono strong">{{ baseName(row.path) }}</span>
          </button>
          <el-tag v-if="!row.exists" size="small" type="danger" class="ml">{{ t('sql.missing') }}</el-tag>
          <el-tag v-else-if="row.error" size="small" type="warning" class="ml" :title="row.error">{{ t('sql.unreadable') }}</el-tag>
          <div class="lp-dim lp-mono path">{{ row.path }}</div>
        </template>
      </el-table-column>
      <el-table-column :label="t('dbm.tablesSize')" min-width="130">
        <template #default="{ row }">
          <button v-if="row.exists" type="button" class="db-stats" @click="openTables(row)">
            <template v-if="row.tables !== null">{{ t('dbm.tableCount', { n: row.tables }) }} · </template>{{ formatBytes(row.size + row.wal_size) }}
          </button>
          <span v-else class="lp-dim">—</span>
        </template>
      </el-table-column>
      <el-table-column :label="t('sql.mode')" width="90">
        <template #default="{ row }"><span class="lp-dim lp-mono">{{ row.journal_mode || '—' }}</span></template>
      </el-table-column>
      <el-table-column :label="t('sql.owner')" width="100">
        <template #default="{ row }"><span class="lp-mono">{{ row.owner || '—' }}</span></template>
      </el-table-column>
      <el-table-column :label="t('dbm.updated')" width="160">
        <template #default="{ row }"><span class="lp-dim lp-num">{{ row.modified || '—' }}</span></template>
      </el-table-column>
      <el-table-column :label="t('actions')" width="220" align="right">
        <template #default="{ row }">
          <el-button link type="primary" size="small" @click="openConn(row)">{{ t('dbm.connInfo') }}</el-button>
          <el-button link type="primary" size="small" :disabled="!row.exists" @click="openBackups(row)">{{ t('dbm.backups') }}</el-button>
          <el-button link type="danger" size="small" @click="openRemove(row)">{{ t('sql.remove') }}</el-button>
        </template>
      </el-table-column>
    </el-table>
  </section>

  <!-- Register / create by path -->
  <el-dialog v-model="pathDialog" :title="pathMode === 'create' ? t('sql.create') : t('sql.addExisting')" width="520px" append-to-body>
    <el-form label-position="top" @submit.prevent="submitPath">
      <el-form-item :label="t('sql.path')">
        <el-input v-model="pathValue" class="lp-mono" :placeholder="pathMode === 'create' ? '/www/wwwroot/mysite/data/app.db' : '/www/wwwroot/mysite/database.sqlite'" />
        <span class="hint">{{ pathMode === 'create' ? t('sql.createHint') : t('sql.addHint') }}</span>
      </el-form-item>
    </el-form>
    <template #footer>
      <el-button @click="pathDialog = false">{{ t('cancel') }}</el-button>
      <el-button type="primary" :loading="busy === 'path'" @click="submitPath">{{ t('confirm') }}</el-button>
    </template>
  </el-dialog>

  <!-- Discovered files -->
  <el-dialog v-model="foundDialog" :title="t('sql.discoverTitle')" width="680px" append-to-body>
    <p class="dialog-text">{{ t('sql.discoverHint') }}</p>
    <el-table :data="found" size="small" max-height="380" :empty-text="t('sql.nothingFound')" @selection-change="(s: Found[]) => (picked = s)">
      <el-table-column type="selection" width="40" />
      <el-table-column :label="t('sql.path')" min-width="320">
        <template #default="{ row }"><span class="lp-mono">{{ row.path }}</span></template>
      </el-table-column>
      <el-table-column :label="t('dbm.size')" width="90">
        <template #default="{ row }">{{ formatBytes(row.size) }}</template>
      </el-table-column>
      <el-table-column :label="t('dbm.updated')" width="150">
        <template #default="{ row }"><span class="lp-dim lp-num">{{ row.modified || '—' }}</span></template>
      </el-table-column>
    </el-table>
    <template #footer>
      <el-button @click="foundDialog = false">{{ t('cancel') }}</el-button>
      <el-button type="primary" :disabled="!picked.length" :loading="busy === 'found'" @click="addFound">
        {{ t('sql.addPicked', { n: picked.length }) }}
      </el-button>
    </template>
  </el-dialog>

  <!-- Remove -->
  <el-dialog v-model="removeDialog" :title="t('sql.removeTitle')" width="480px" append-to-body>
    <p class="dialog-text lp-mono">{{ removeTarget?.path }}</p>
    <p class="dialog-text">{{ t('sql.removeText') }}</p>
    <el-checkbox v-model="removeFile">{{ t('sql.removeFile') }}</el-checkbox>
    <el-alert v-if="removeFile" :title="t('sql.removeFileWarn')" type="error" :closable="false" show-icon class="warn" />
    <template #footer>
      <el-button @click="removeDialog = false">{{ t('cancel') }}</el-button>
      <el-button type="danger" :loading="busy === 'remove'" @click="submitRemove">{{ t('sql.remove') }}</el-button>
    </template>
  </el-dialog>

  <!-- Tables -->
  <el-drawer v-model="tablesDrawer" :size="drawerSize" append-to-body>
    <template #header>
      <div class="drawer-head">
        <span class="drawer-head__title lp-mono">{{ target ? baseName(target.path) : '' }}</span>
        <span class="lp-dim">{{ t('dbm.tableCount', { n: tables.filter((r) => r.engine === 'table').length }) }} · {{ target ? formatBytes(target.size) : '' }}</span>
      </div>
    </template>
    <div class="tables-tools">
      <el-input v-model="tableKeyword" :prefix-icon="Search" size="small" clearable :placeholder="t('dbm.searchTables')" class="search" />
      <el-button size="small" :icon="Refresh" :loading="tablesLoading" @click="loadTables">{{ t('refresh') }}</el-button>
    </div>
    <el-skeleton v-if="tablesLoading && !tables.length" animated :rows="6" />
    <el-table v-else :data="filteredTables" size="small" :empty-text="tableKeyword ? t('dbm.noMatch') : t('dbm.noTables')">
      <el-table-column :label="t('dbm.table')" min-width="200">
        <template #default="{ row }">
          <span class="lp-mono strong">{{ row.name }}</span>
          <el-tag v-if="row.engine === 'view'" size="small" type="info" class="ml">{{ t('sql.view') }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column :label="t('dbm.rows')" width="110" align="right">
        <template #default="{ row }"><span class="lp-num">{{ row.rows === null ? '—' : row.rows.toLocaleString() }}</span></template>
      </el-table-column>
      <el-table-column :label="t('dbm.size')" width="100" align="right">
        <template #default="{ row }"><span class="lp-num">{{ row.size ? formatBytes(row.size) : '—' }}</span></template>
      </el-table-column>
    </el-table>
  </el-drawer>

  <!-- Connection info -->
  <el-drawer v-model="connDrawer" :title="`${t('dbm.connInfo')} · ${target ? baseName(target.path) : ''}`" size="520px" append-to-body>
    <template v-if="target">
      <p class="lp-dim conn-p">{{ t('sql.connText') }}</p>
      <div v-for="item in connStrings" :key="item.label" class="snippet">
        <span class="snippet__label">{{ item.label }}</span>
        <code class="snippet__code">{{ item.value }}</code>
        <el-button link size="small" :icon="CopyDocument" @click="copy(item.value)" />
      </div>
      <p class="lp-dim conn-p">{{ t('sql.ownerText', { owner: target.owner || '—' }) }}</p>
    </template>
  </el-drawer>

  <!-- Backups -->
  <el-drawer v-model="backupDrawer" :title="`${t('dbm.backups')} · ${target ? baseName(target.path) : ''}`" size="560px" append-to-body>
    <div class="drawer-actions">
      <el-button type="primary" size="small" :loading="busy === 'backup'" @click="backupNow">{{ t('dbm.backupNow') }}</el-button>
      <span class="hint hint--inline">{{ t('sql.backupHint') }}</span>
    </div>
    <el-table :data="backups" :empty-text="t('dbm.noBackups')" size="small">
      <el-table-column :label="t('dbm.file')" min-width="200">
        <template #default="{ row }"><span class="lp-mono">{{ row.file }}</span></template>
      </el-table-column>
      <el-table-column :label="t('dbm.size')" width="90">
        <template #default="{ row }">{{ formatBytes(row.size) }}</template>
      </el-table-column>
      <el-table-column :label="t('actions')" width="150">
        <template #default="{ row }">
          <el-button link type="primary" size="small" @click="downloadFileApi(row.path, row.file)">{{ t('dbm.download') }}</el-button>
          <el-button link type="warning" size="small" @click="restore(row)">{{ t('dbm.restore') }}</el-button>
          <el-button link type="danger" size="small" @click="removeBackup(row)">{{ t('delete') }}</el-button>
        </template>
      </el-table-column>
    </el-table>
  </el-drawer>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Aim, Coin, CopyDocument, InfoFilled, Plus, Refresh, Search } from '@element-plus/icons-vue'
import {
  backupSqlite, createSqlite, deleteSqliteBackup, discoverSqlite, downloadFileApi, getSqliteTables,
  listSqlite, listSqliteBackups, registerSqlite, removeSqlite, restoreSqlite,
} from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes } from '@/utils/format'

interface SqliteFile {
  path: string
  exists: boolean
  size: number
  wal_size: number
  tables: number | null
  journal_mode: string | null
  owner: string | null
  modified: string | null
  error: string | null
}
interface Found { path: string; size: number; modified: string | null }
interface TableRow { name: string; rows: number | null; size: number; engine: string | null }

const { t } = useI18n()
const files = ref<SqliteFile[]>([])
const loaded = ref(false)
const loading = ref(false)
const busy = ref('')
const keyword = ref('')
const target = ref<SqliteFile | null>(null)

const filtered = computed(() => {
  const q = keyword.value.trim().toLowerCase()
  return q ? files.value.filter((f) => f.path.toLowerCase().includes(q)) : files.value
})

function baseName(path: string) {
  return path.slice(path.lastIndexOf('/') + 1)
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    ElMessage.success(t('dbm.copied'))
  } catch {
    ElMessage.warning(text)
  }
}

async function load() {
  loading.value = true
  try {
    const res: any = await listSqlite()
    if (res.success) files.value = res.data || []
    loaded.value = true
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}

// --- Add by path / create ---
const pathDialog = ref(false)
const pathMode = ref<'create' | 'register'>('create')
const pathValue = ref('')
function openPath(mode: 'create' | 'register') {
  pathMode.value = mode
  pathValue.value = ''
  pathDialog.value = true
}
async function submitPath() {
  const path = pathValue.value.trim()
  if (!path) return
  busy.value = 'path'
  try {
    const res: any = pathMode.value === 'create' ? await createSqlite(path) : await registerSqlite(path)
    if (res.success) {
      pathDialog.value = false
      ElMessage.success(pathMode.value === 'create' ? t('sql.created') : t('sql.added'))
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Discover ---
const scanning = ref(false)
const foundDialog = ref(false)
const found = ref<Found[]>([])
const picked = ref<Found[]>([])
async function discover() {
  scanning.value = true
  try {
    const res: any = await discoverSqlite()
    if (res.success) {
      found.value = res.data || []
      picked.value = []
      foundDialog.value = true
    }
  } catch {
    // Interceptor reports it.
  } finally {
    scanning.value = false
  }
}
async function addFound() {
  busy.value = 'found'
  let added = 0
  try {
    for (const item of picked.value) {
      const res: any = await registerSqlite(item.path).catch(() => null)
      if (res?.success) added++
    }
    foundDialog.value = false
    if (added) ElMessage.success(t('sql.addedN', { n: added }))
    await load()
  } finally {
    busy.value = ''
  }
}

// --- Remove ---
const removeDialog = ref(false)
const removeTarget = ref<SqliteFile | null>(null)
const removeFile = ref(false)
function openRemove(row: SqliteFile) {
  removeTarget.value = row
  removeFile.value = false
  removeDialog.value = true
}
async function submitRemove() {
  if (!removeTarget.value) return
  busy.value = 'remove'
  try {
    const res: any = await removeSqlite(removeTarget.value.path, removeFile.value)
    if (res.success) {
      removeDialog.value = false
      ElMessage.success(removeFile.value ? t('sql.deleted') : t('sql.removed'))
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Tables ---
const tablesDrawer = ref(false)
const tables = ref<TableRow[]>([])
const tablesLoading = ref(false)
const tableKeyword = ref('')
const drawerSize = computed(() => (window.innerWidth < 760 ? '100%' : '640px'))
const filteredTables = computed(() => {
  const q = tableKeyword.value.trim().toLowerCase()
  return q ? tables.value.filter((r) => r.name.toLowerCase().includes(q)) : tables.value
})
function openTables(row: SqliteFile) {
  if (target.value?.path !== row.path) tables.value = []
  target.value = row
  tableKeyword.value = ''
  tablesDrawer.value = true
  void loadTables()
}
async function loadTables() {
  if (!target.value) return
  tablesLoading.value = true
  try {
    const res: any = await getSqliteTables(target.value.path)
    if (res.success) tables.value = res.data || []
  } catch {
    // Interceptor reports it.
  } finally {
    tablesLoading.value = false
  }
}

// --- Connection ---
const connDrawer = ref(false)
function openConn(row: SqliteFile) {
  target.value = row
  connDrawer.value = true
}
const connStrings = computed(() => {
  const path = target.value?.path || ''
  return [
    { label: t('sql.path'), value: path },
    { label: 'PHP PDO', value: `sqlite:${path}` },
    { label: 'Python', value: `sqlite3.connect("${path}")` },
    { label: 'Node', value: `new Database("${path}")` },
    { label: 'Go', value: `sql.Open("sqlite3", "${path}")` },
    { label: 'CLI', value: `sqlite3 ${path}` },
  ]
})

// --- Backups ---
const backupDrawer = ref(false)
const backups = ref<any[]>([])
async function loadBackups() {
  if (!target.value) return
  const res: any = await listSqliteBackups(target.value.path).catch(() => null)
  backups.value = res?.success ? res.data : []
}
async function openBackups(row: SqliteFile) {
  target.value = row
  backups.value = []
  backupDrawer.value = true
  await loadBackups()
}
async function backupNow() {
  if (!target.value) return
  busy.value = 'backup'
  try {
    const res: any = await backupSqlite(target.value.path)
    if (res.success) ElMessage.success(t('dbm.backedUp'))
    await loadBackups()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}
async function restore(row: any) {
  if (!target.value) return
  try {
    await ElMessageBox.confirm(t('sql.restoreConfirm', { file: row.file }), t('dbm.restore'), { type: 'warning' })
  } catch {
    return
  }
  const res: any = await restoreSqlite(target.value.path, row.file).catch(() => null)
  if (res?.success) {
    ElMessage.success(t('dbm.restored'))
    await load()
  }
}
async function removeBackup(row: any) {
  if (!target.value) return
  try {
    await ElMessageBox.confirm(t('dbm.deleteBackupConfirm', { file: row.file }), t('delete'), { type: 'warning' })
  } catch {
    return
  }
  const res: any = await deleteSqliteBackup(target.value.path, row.file).catch(() => null)
  if (res?.success) await loadBackups()
}

defineExpose({ load })
load()
</script>

<style scoped lang="scss">
.count {
  margin-left: 6px;
  padding: 0 7px;
  border-radius: var(--lp-r-full);
  background: var(--lp-muted-soft);
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  font-weight: 500;
}
.head-tools { display: flex; gap: var(--lp-2); align-items: center; flex-wrap: wrap; justify-content: flex-end; }
.search { width: 180px; }
.intro {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  padding: var(--lp-2) var(--lp-5);
  border-bottom: 1px solid var(--lp-line-soft);
  font-size: var(--lp-text-sm);
  color: var(--lp-text-dim);
  .el-icon { flex-shrink: 0; }
}
.strong { font-weight: 600; }
.ml { margin-left: var(--lp-2); }
.path { font-size: 11px; margin-top: 2px; word-break: break-all; }
.empty { display: flex; flex-direction: column; align-items: center; gap: var(--lp-2); padding: 40px var(--lp-4); text-align: center; }
.empty__icon { color: var(--lp-text-faint); }
.empty__title { margin: 0; font-weight: 600; }
.empty__hint { margin: 0 0 var(--lp-2); font-size: var(--lp-text-sm); max-width: 520px; line-height: 1.6; }
.empty__actions { display: flex; gap: var(--lp-2); }
.dialog-text { margin: 0 0 var(--lp-3); font-size: var(--lp-text-sm); line-height: 1.6; color: var(--lp-text-soft); word-break: break-all; }
.hint {
  display: block;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  line-height: 1.5;
  &--inline { display: inline; margin: 0 0 0 var(--lp-2); }
}
.warn { margin-top: var(--lp-3); }
.drawer-actions { display: flex; align-items: center; margin-bottom: var(--lp-3); }
.conn-p { font-size: var(--lp-text-xs); line-height: 1.6; margin: 0 0 var(--lp-3); }
.snippet { display: grid; grid-template-columns: 70px 1fr auto; align-items: center; gap: var(--lp-2); margin-bottom: 6px; }
.snippet__label { font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
.snippet__code {
  font-family: var(--lp-font-mono);
  font-size: 11px;
  padding: 4px 8px;
  border-radius: var(--lp-r-sm);
  background: var(--lp-muted-soft);
  overflow-x: auto;
  white-space: nowrap;
}
@media (max-width: 720px) {
  .search { width: 130px; }
}
</style>
