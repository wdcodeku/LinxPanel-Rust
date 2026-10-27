<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.databases') }}</h1>
        <p class="lp-page-sub">{{ t('dbm.subtitle') }}</p>
      </div>
      <div class="lp-page-actions">
        <el-button size="small" :icon="Refresh" :loading="loading" @click="loadAll">{{ t('refresh') }}</el-button>
        <el-button
          v-if="current?.manageable && engine !== 'redis'"
          type="primary"
          size="small"
          :icon="Plus"
          @click="openCreate"
        >{{ t('dbm.create') }}</el-button>
      </div>
    </div>

    <!-- Engines -->
    <div class="engine-grid">
      <article
        v-for="item in engines"
        :key="item.engine"
        class="engine-card"
        :class="{ 'is-active': item.engine === engine }"
        @click="selectEngine(item.engine)"
      >
        <div class="engine-card__head">
          <span class="engine-logo" :data-engine="item.engine">{{ logo(item.engine) }}</span>
          <div class="engine-card__title">
            <span class="engine-card__name">{{ item.display_name }}</span>
            <span class="lp-pill" :class="item.running ? 'lp-pill--ok' : ''">
              <span class="lp-pill__dot"></span>
              {{ item.running ? t('dbm.running') : item.installed ? t('dbm.stopped') : t('dbm.notInstalled') }}
            </span>
          </div>
        </div>
        <div class="engine-card__meta lp-mono lp-truncate" :title="item.version">
          {{ item.installed ? item.version : t('dbm.installHint') }}
        </div>
        <div class="engine-card__foot" @click.stop>
          <span class="engine-card__port lp-num">{{ item.installed ? `:${item.port}` : '' }}</span>
          <el-button
            v-if="!item.installed"
            type="primary"
            size="small"
            plain
            :loading="busy === `install-${item.engine}`"
            @click="install(item.engine)"
          >{{ t('dbm.install') }}</el-button>
          <template v-else>
            <el-button
              v-if="item.engine === 'mysql' && item.running && !item.manageable"
              size="small"
              type="warning"
              plain
              @click="adminDialog = true"
            >{{ t('dbm.connect') }}</el-button>
            <el-button
              v-if="item.engine === 'redis'"
              size="small"
              @click="openRedisPassword(item)"
            >{{ t('dbm.redisPassword') }}</el-button>
            <el-button
              size="small"
              :loading="busy === `restart-${item.engine}`"
              @click="restart(item.engine)"
            >{{ item.running ? t('dbm.restart') : t('dbm.start') }}</el-button>
          </template>
        </div>
      </article>
    </div>

    <!-- Databases -->
    <section v-if="engine !== 'redis'" class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Coin /></el-icon>{{ current?.display_name }}</h2>
      </header>
      <el-alert
        v-if="current && !current.manageable"
        :title="current.installed ? current.message : t('dbm.installFirst')"
        type="info"
        :closable="false"
        show-icon
        class="engine-alert"
      />
      <el-table v-else v-loading="listLoading" :data="rows" :empty-text="t('dbm.empty')">
        <el-table-column :label="t('dbm.name')" min-width="140">
          <template #default="{ row }">
            <span class="lp-mono">{{ row.name }}</span>
            <el-tag v-if="!row.managed" size="small" type="info" class="ml">{{ t('dbm.external') }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column :label="t('dbm.user')" min-width="120">
          <template #default="{ row }"><span class="lp-mono">{{ row.username || '—' }}</span></template>
        </el-table-column>
        <el-table-column :label="t('dbm.password')" min-width="220">
          <template #default="{ row }">
            <template v-if="row.password">
              <span class="lp-mono">{{ shown[row.name] ? row.password : '••••••••••' }}</span>
              <el-button link size="small" :icon="shown[row.name] ? Hide : View" @click="shown[row.name] = !shown[row.name]" />
              <el-button link size="small" :icon="CopyDocument" @click="copy(row.password)" />
            </template>
            <span v-else class="dim">—</span>
          </template>
        </el-table-column>
        <el-table-column v-if="engine === 'mysql'" :label="t('dbm.host')" width="120">
          <template #default="{ row }"><span class="lp-mono">{{ row.host || '—' }}</span></template>
        </el-table-column>
        <el-table-column :label="t('dbm.note')" min-width="120" show-overflow-tooltip>
          <template #default="{ row }">{{ row.note || '' }}</template>
        </el-table-column>
        <el-table-column :label="t('actions')" width="230" fixed="right">
          <template #default="{ row }">
            <el-button link type="primary" size="small" @click="openBackups(row)">{{ t('dbm.backups') }}</el-button>
            <el-button v-if="row.managed" link type="primary" size="small" @click="resetPassword(row)">{{ t('dbm.changePassword') }}</el-button>
            <el-button link type="danger" size="small" @click="remove(row)">{{ t('delete') }}</el-button>
          </template>
        </el-table-column>
      </el-table>
    </section>

    <section v-else-if="current" class="lp-card">
      <div class="lp-card-body">
        <p class="section-note">{{ t('dbm.redisHint') }}</p>
        <dl class="kv">
          <div><dt>{{ t('dbm.address') }}</dt><dd class="lp-mono">127.0.0.1:{{ current.port }}</dd></div>
          <div><dt>{{ t('dbm.password') }}</dt><dd class="lp-mono">{{ current.redis_password || t('dbm.noPassword') }}</dd></div>
        </dl>
      </div>
    </section>

    <!-- Create -->
    <el-dialog v-model="createDialog" :title="t('dbm.create')" width="520px" append-to-body>
      <el-form label-position="top" @submit.prevent="submitCreate">
        <el-form-item :label="t('dbm.name')">
          <el-input v-model="form.name" class="lp-mono" placeholder="mysite" @input="syncUser" />
          <span class="hint">{{ t('dbm.nameHint') }}</span>
        </el-form-item>
        <el-form-item :label="t('dbm.user')">
          <el-input v-model="form.username" class="lp-mono" @input="userEdited = true" />
        </el-form-item>
        <el-form-item :label="t('dbm.password')">
          <div class="inline-row">
            <el-input v-model="form.password" class="lp-mono" />
            <el-button @click="form.password = randomPassword()">{{ t('dbm.generate') }}</el-button>
          </div>
        </el-form-item>
        <el-form-item v-if="engine === 'mysql'" :label="t('dbm.host')">
          <el-select v-model="form.hostMode" style="width: 100%">
            <el-option :label="t('dbm.hostLocal')" value="localhost" />
            <el-option :label="t('dbm.hostAny')" value="%" />
            <el-option :label="t('dbm.hostCustom')" value="custom" />
          </el-select>
          <el-input v-if="form.hostMode === 'custom'" v-model="form.host" class="lp-mono mt" placeholder="203.0.113.7" />
          <span v-if="form.hostMode !== 'localhost'" class="hint">{{ t('dbm.hostRemoteHint') }}</span>
        </el-form-item>
        <el-form-item :label="t('dbm.note')">
          <el-input v-model="form.note" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="createDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="busy === 'create'" @click="submitCreate">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>

    <!-- Connect an existing MySQL -->
    <el-dialog v-model="adminDialog" :title="t('dbm.connect')" width="460px" append-to-body>
      <p class="section-note">{{ t('dbm.connectHint') }}</p>
      <el-form label-position="top">
        <el-form-item :label="t('dbm.user')"><el-input v-model="adminForm.user" class="lp-mono" /></el-form-item>
        <el-form-item :label="t('dbm.password')">
          <el-input v-model="adminForm.password" type="password" show-password />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="adminDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="busy === 'admin'" @click="submitAdmin">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>

    <!-- Redis password -->
    <el-dialog v-model="redisDialog" :title="t('dbm.redisPassword')" width="460px" append-to-body>
      <el-form label-position="top">
        <el-form-item :label="t('dbm.password')">
          <div class="inline-row">
            <el-input v-model="redisPassword" class="lp-mono" clearable />
            <el-button @click="redisPassword = randomPassword()">{{ t('dbm.generate') }}</el-button>
          </div>
          <span class="hint">{{ t('dbm.redisPasswordHint') }}</span>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="redisDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="busy === 'redis'" @click="submitRedis">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>

    <!-- Backups -->
    <el-drawer v-model="backupDrawer" :title="`${t('dbm.backups')} · ${backupTarget?.name || ''}`" size="560px" append-to-body>
      <div class="drawer-actions">
        <el-button type="primary" size="small" :loading="busy === 'backup'" @click="backupNow">{{ t('dbm.backupNow') }}</el-button>
        <span class="hint hint--inline">{{ t('dbm.backupHint') }}</span>
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

    <OperationLogDialog
      v-model="logDialog.visible"
      :title="logDialog.title"
      :log="logDialog.log"
      :status="logDialog.status"
      :loading="logDialog.status === 'running'"
      :show-clear="false"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Coin, CopyDocument, Hide, Plus, Refresh, View } from '@element-plus/icons-vue'
import OperationLogDialog from '@/components/OperationLogDialog.vue'
import {
  backupDatabase, changeDatabasePassword, createDatabase, deleteDatabase, deleteDatabaseBackup,
  downloadFileApi, getDbEngines, installDbEngine, listDatabaseBackups, listDatabases,
  restartDbEngine, restoreDatabase, setMysqlAdmin, setRedisPassword,
} from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes } from '@/utils/format'

interface EngineInfo {
  engine: 'mysql' | 'postgresql' | 'redis'
  display_name: string
  installed: boolean
  version: string
  running: boolean
  port: number
  manageable: boolean
  message: string
  redis_password?: string
}
interface DbRow {
  name: string
  username: string | null
  password: string | null
  host: string | null
  note: string | null
  managed: boolean
}

const { t } = useI18n()
const engines = ref<EngineInfo[]>([])
const engine = ref<EngineInfo['engine']>('mysql')
const rows = ref<DbRow[]>([])
const loading = ref(false)
const listLoading = ref(false)
const busy = ref('')
const shown = reactive<Record<string, boolean>>({})
const logDialog = reactive({ visible: false, title: '', log: '', status: 'idle' as 'idle' | 'running' | 'success' | 'error' })

const current = computed(() => engines.value.find((e) => e.engine === engine.value))

function logo(kind: string) {
  return { mysql: 'My', postgresql: 'Pg', redis: 'Rd' }[kind] || '?'
}

function randomPassword(length = 20) {
  const alphabet = 'abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789'
  const bytes = crypto.getRandomValues(new Uint8Array(length))
  return Array.from(bytes, (b) => alphabet[b % alphabet.length]).join('')
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    ElMessage.success(t('dbm.copied'))
  } catch {
    ElMessage.warning(text)
  }
}

async function loadEngines() {
  const res: any = await getDbEngines()
  if (res.success) engines.value = res.data
}

async function loadRows() {
  rows.value = []
  if (!current.value?.manageable || engine.value === 'redis') return
  listLoading.value = true
  try {
    const res: any = await listDatabases(engine.value)
    if (res.success) rows.value = res.data
  } finally {
    listLoading.value = false
  }
}

async function loadAll() {
  loading.value = true
  try {
    await loadEngines()
    await loadRows()
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}

function selectEngine(kind: EngineInfo['engine']) {
  if (engine.value === kind) return
  engine.value = kind
  loadRows()
}

async function install(kind: string) {
  busy.value = `install-${kind}`
  try {
    // The task window shows the live log and the outcome.
    await installDbEngine(kind)
    await loadAll()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

async function restart(kind: string) {
  busy.value = `restart-${kind}`
  try {
    const res: any = await restartDbEngine(kind)
    if (res.success) ElMessage.success(t('dbm.restarted'))
    await loadAll()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Create ---
const createDialog = ref(false)
const userEdited = ref(false)
const form = reactive({ name: '', username: '', password: '', hostMode: 'localhost', host: '', note: '' })

function openCreate() {
  Object.assign(form, { name: '', username: '', password: randomPassword(), hostMode: 'localhost', host: '', note: '' })
  userEdited.value = false
  createDialog.value = true
}

function syncUser() {
  if (!userEdited.value) form.username = form.name
}

async function submitCreate() {
  busy.value = 'create'
  try {
    const res: any = await createDatabase({
      engine: engine.value,
      name: form.name.trim(),
      username: form.username.trim(),
      password: form.password.trim(),
      host: form.hostMode === 'custom' ? form.host.trim() : form.hostMode,
      note: form.note,
    })
    if (res.success) {
      createDialog.value = false
      const d = res.data
      ElMessageBox.alert(
        `${t('dbm.name')}: ${d.name}\n${t('dbm.user')}: ${d.username}\n${t('dbm.password')}: ${d.password}\n${t('dbm.host')}: ${d.host}`,
        t('dbm.created'),
        { customClass: 'credential-box' },
      )
      await loadRows()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

async function remove(row: DbRow) {
  try {
    await ElMessageBox.confirm(t('dbm.deleteConfirm', { name: row.name }), t('delete'), {
      type: 'warning',
      confirmButtonText: t('delete'),
      confirmButtonClass: 'el-button--danger',
    })
  } catch {
    return
  }
  const res: any = await deleteDatabase(engine.value, row.name).catch(() => null)
  if (res?.success) {
    ElMessage.success(t('dbm.deleted'))
    await loadRows()
  }
}

async function resetPassword(row: DbRow) {
  try {
    await ElMessageBox.confirm(t('dbm.passwordConfirm', { name: row.username || '' }), t('dbm.changePassword'), { type: 'warning' })
  } catch {
    return
  }
  const res: any = await changeDatabasePassword(engine.value, row.name).catch(() => null)
  if (res?.success) {
    ElMessageBox.alert(`${t('dbm.password')}: ${res.data.password}`, t('dbm.passwordChanged'), { customClass: 'credential-box' })
    await loadRows()
  }
}

// --- MySQL admin / Redis password ---
const adminDialog = ref(false)
const adminForm = reactive({ user: 'root', password: '' })
async function submitAdmin() {
  busy.value = 'admin'
  try {
    const res: any = await setMysqlAdmin({ ...adminForm })
    if (res.success) {
      adminDialog.value = false
      ElMessage.success(t('dbm.connected'))
      await loadAll()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

const redisDialog = ref(false)
const redisPassword = ref('')
function openRedisPassword(item: EngineInfo) {
  redisPassword.value = item.redis_password || ''
  redisDialog.value = true
}
async function submitRedis() {
  busy.value = 'redis'
  try {
    const res: any = await setRedisPassword(redisPassword.value.trim())
    if (res.success) {
      redisDialog.value = false
      ElMessage.success(t('ps.saved'))
      await loadEngines()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Backups ---
const backupDrawer = ref(false)
const backupTarget = ref<DbRow | null>(null)
const backups = ref<any[]>([])

async function loadBackups() {
  if (!backupTarget.value) return
  const res: any = await listDatabaseBackups(engine.value, backupTarget.value.name).catch(() => null)
  backups.value = res?.success ? res.data : []
}

async function openBackups(row: DbRow) {
  backupTarget.value = row
  backupDrawer.value = true
  await loadBackups()
}

async function backupNow() {
  if (!backupTarget.value) return
  busy.value = 'backup'
  try {
    const res: any = await backupDatabase(engine.value, backupTarget.value.name)
    if (res.success) ElMessage.success(t('dbm.backedUp'))
    await loadBackups()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

async function restore(row: any) {
  if (!backupTarget.value) return
  try {
    await ElMessageBox.confirm(t('dbm.restoreConfirm', { file: row.file }), t('dbm.restore'), { type: 'warning' })
  } catch {
    return
  }
  const loadingMsg = ElMessage.info({ message: t('dbm.restoring'), duration: 0 })
  const res: any = await restoreDatabase(engine.value, backupTarget.value.name, row.file).catch(() => null)
  loadingMsg.close()
  if (res?.success) ElMessage.success(t('dbm.restored'))
}

async function removeBackup(row: any) {
  if (!backupTarget.value) return
  try {
    await ElMessageBox.confirm(t('dbm.deleteBackupConfirm', { file: row.file }), t('delete'), { type: 'warning' })
  } catch {
    return
  }
  const res: any = await deleteDatabaseBackup(engine.value, backupTarget.value.name, row.file).catch(() => null)
  if (res?.success) await loadBackups()
}

onMounted(loadAll)
</script>

<style scoped lang="scss">
.engine-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: var(--lp-4);
  margin-bottom: var(--lp-4);
}
.engine-card {
  display: flex;
  flex-direction: column;
  gap: var(--lp-3);
  padding: var(--lp-4);
  background: var(--lp-surface);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s;

  &:hover { border-color: var(--lp-accent); }
  &.is-active { border-color: var(--lp-accent); box-shadow: 0 0 0 1px var(--lp-accent) inset; }
}
.engine-card__head { display: flex; align-items: center; gap: var(--lp-3); }
.engine-logo {
  display: grid;
  place-items: center;
  width: 38px;
  height: 38px;
  border-radius: var(--lp-r-md);
  font-weight: 700;
  font-size: 13px;
  color: #fff;
  background: #00758f;

  &[data-engine='postgresql'] { background: #336791; }
  &[data-engine='redis'] { background: #d82c20; }
}
.engine-card__title { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.engine-card__name { font-weight: 600; color: var(--lp-text); }
.engine-card__meta { font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
.engine-card__foot { display: flex; align-items: center; justify-content: flex-end; gap: var(--lp-2); flex-wrap: wrap; }
.engine-card__port { margin-right: auto; font-size: var(--lp-text-sm); color: var(--lp-text-dim); }
.engine-alert { margin: var(--lp-4); width: auto; }

.ml { margin-left: var(--lp-2); }
.mt { margin-top: var(--lp-2); }
.dim { color: var(--lp-text-dim); }
.hint {
  display: block;
  flex-basis: 100%;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  &--inline { display: inline; margin: 0 0 0 var(--lp-2); }
}
.section-note { margin-bottom: var(--lp-3); font-size: var(--lp-text-sm); color: var(--lp-text-dim); }
.inline-row {
  display: flex;
  gap: var(--lp-2);
  width: 100%;
  .el-input { flex: 1; min-width: 0; }
}
.drawer-actions { display: flex; align-items: center; margin-bottom: var(--lp-3); }
.kv {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: var(--lp-4);
  margin: 0;
  dt { font-size: var(--lp-text-xs); color: var(--lp-text-dim); margin-bottom: 3px; }
  dd { margin: 0; color: var(--lp-text); }
}
</style>

<style lang="scss">
/* Credential pop-ups: keep line breaks so the values can be copied as shown. */
.credential-box .el-message-box__message { white-space: pre-line; font-family: var(--lp-font-mono, monospace); }
</style>
