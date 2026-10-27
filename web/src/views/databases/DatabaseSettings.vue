<template>
  <el-drawer
    :model-value="modelValue"
    :size="drawerSize"
    append-to-body
    class="dbs-drawer"
    @update:model-value="(v: boolean) => emit('update:modelValue', v)"
    @open="load"
  >
    <template #header>
      <div class="dbs-head">
        <span class="dbs-head__title">{{ t('dbs.title', { name: displayName }) }}</span>
        <span class="dbs-head__meta">
          <span v-if="s?.version" class="lp-mono">{{ s.version }}</span>
          <span v-if="s?.data_dir" class="lp-mono">{{ s.data_dir }}</span>
          <span v-if="s?.data_size != null">{{ t('dbs.dataSize', { size: formatBytes(s.data_size) }) }}</span>
        </span>
      </div>
    </template>

    <el-skeleton v-if="!s" animated :rows="8" />
    <el-alert v-else-if="!s.running" :title="t('dbs.notRunning')" type="warning" :closable="false" show-icon />

    <el-tabs v-else v-model="tab" class="dbs-tabs">
      <!-- ===== Connection ===== -->
      <el-tab-pane :label="t('dbs.tabConn')" name="conn">
        <div class="row">
          <div class="row__text">
            <div class="row__title">{{ t('dbs.remote') }}</div>
            <div class="row__desc">{{ t(`dbs.remoteDesc.${engine}`) }}</div>
          </div>
          <el-switch
            :model-value="s.remote.enabled"
            :loading="busy === 'remote'"
            :disabled="engine === 'redis' && !s.superuser_password && !s.remote.enabled"
            @change="(v: string | number | boolean) => toggleRemote(v === true)"
          />
        </div>
        <el-alert
          v-if="engine === 'redis' && !s.superuser_password"
          :title="t('dbs.redisNeedsPassword')"
          type="info"
          :closable="false"
          show-icon
          class="block"
        />
        <div v-if="remoteAsk" class="ask">
          <div class="ask__label">{{ t('dbs.allowFrom') }}</div>
          <div class="inline-row">
            <el-input v-model="remoteFrom" class="lp-mono" :placeholder="t('dbs.allowFromPlaceholder')" />
            <el-button v-if="network?.your_ip" @click="remoteFrom = network.your_ip">{{ t('dbs.useMyIp') }}</el-button>
          </div>
          <p class="hint">{{ t('dbs.allowFromHint') }}</p>
          <div class="ask__actions">
            <el-button @click="remoteAsk = false">{{ t('cancel') }}</el-button>
            <el-button type="primary" :loading="busy === 'remote'" @click="submitRemote(true)">{{ t('dbs.remoteEnable') }}</el-button>
          </div>
        </div>

        <h3 class="section">{{ t('dbs.diagnosis') }}</h3>
        <ul v-if="network" class="checks">
          <li v-for="check in checks" :key="check.key" class="check">
            <span class="check__icon" :class="`is-${check.level}`">
              <el-icon><CircleCheckFilled v-if="check.level === 'ok'" /><WarningFilled v-else-if="check.level === 'warn'" /><CircleCloseFilled v-else /></el-icon>
            </span>
            <span class="check__text">
              <span class="check__title">{{ check.title }}</span>
              <span class="check__detail">{{ check.detail }}</span>
            </span>
          </li>
        </ul>
        <el-skeleton v-else animated :rows="3" />
        <div class="tip"><el-icon><InfoFilled /></el-icon><span>{{ t('dbm.sshTip') }}</span></div>
      </el-tab-pane>

      <!-- ===== Superuser password ===== -->
      <el-tab-pane :label="t('dbs.tabPassword')" name="password">
        <div class="kv">
          <div><dt>{{ t('dbs.superuser') }}</dt><dd class="lp-mono">{{ superuserLabel }}</dd></div>
          <div>
            <dt>{{ t('dbs.currentPassword') }}</dt>
            <dd>
              <template v-if="s.superuser_password">
                <span class="lp-mono">{{ showPassword ? s.superuser_password : '••••••••••' }}</span>
                <el-button link size="small" :icon="showPassword ? Hide : View" @click="showPassword = !showPassword" />
                <el-button link size="small" :icon="CopyDocument" @click="copy(s.superuser_password)" />
              </template>
              <span v-else class="lp-dim">{{ engine === 'redis' ? t('dbs.noRedisPassword') : t('dbs.unknownPassword') }}</span>
            </dd>
          </div>
        </div>
        <div class="field">
          <div class="field__label">{{ t('dbs.newPassword') }}</div>
          <div class="inline-row">
            <el-input v-model="newPassword" class="lp-mono" :placeholder="engine === 'redis' ? t('dbs.redisEmptyHint') : ''" />
            <el-button @click="newPassword = randomPassword()">{{ t('dbm.generate') }}</el-button>
          </div>
          <p class="hint">{{ t(`dbs.passwordNote.${engine}`) }}</p>
        </div>
        <el-button type="primary" :loading="busy === 'password'" :disabled="engine !== 'redis' && !newPassword.trim()" @click="savePassword">
          {{ t('dbs.savePassword') }}
        </el-button>
      </el-tab-pane>

      <!-- ===== Performance ===== -->
      <el-tab-pane :label="t('dbs.tabParams')" name="params">
        <div class="params">
          <div v-for="p in s.params" :key="p.key" class="param">
            <div class="param__text">
              <div class="param__name">{{ t(`dbs.param.${p.key}`) }} <span class="lp-mono lp-dim param__key">{{ p.key }}</span></div>
              <div class="param__desc">{{ t(`dbs.paramHelp.${p.key}`) }}</div>
            </div>
            <div class="param__input">
              <template v-if="p.kind === 'size'">
                <el-input-number v-model="paramForm[p.key].num" :min="0" :step="paramForm[p.key].unit === 'G' ? 1 : 16" controls-position="right" class="num" />
                <el-select v-model="paramForm[p.key].unit" class="unit">
                  <el-option label="MB" value="M" />
                  <el-option label="GB" value="G" />
                </el-select>
              </template>
              <el-switch v-else-if="p.kind === 'bool'" v-model="paramForm[p.key].bool" />
              <el-select v-else-if="p.kind === 'enum'" v-model="paramForm[p.key].text" class="enum">
                <el-option v-for="o in p.options" :key="o" :label="o" :value="o" />
              </el-select>
              <el-input-number v-else v-model="paramForm[p.key].num" :min="0" controls-position="right" class="num" />
            </div>
          </div>
        </div>
        <div class="save-bar">
          <span class="hint">{{ t('dbs.restartNote') }}</span>
          <el-button type="primary" :loading="busy === 'params'" :disabled="!paramsChanged" @click="saveParams">{{ t('dbs.saveRestart') }}</el-button>
        </div>
      </el-tab-pane>

      <!-- ===== Logs ===== -->
      <el-tab-pane :label="t('dbs.tabLogs')" name="logs">
        <p class="intro">{{ t('dbs.logsIntro') }}</p>
        <div v-for="log in s.logs" :key="log.kind" class="log">
          <div class="log__head">
            <div class="log__text">
              <div class="log__name">
                {{ t(`dbs.log.${log.kind}`) }}
                <span v-if="log.size != null" class="lp-pill" :class="log.size > 1 << 30 ? 'lp-pill--warn' : ''">{{ formatBytes(log.size) }}</span>
              </div>
              <div class="log__desc">{{ t(`dbs.logHelp.${engine}.${log.kind}`) }}</div>
              <div v-if="log.path" class="log__path lp-mono">{{ log.path }}</div>
            </div>
            <el-switch v-if="log.switchable" v-model="logForm[log.kind].enabled" />
          </div>
          <div v-if="log.threshold != null && logForm[log.kind]" class="log__threshold">
            <span>{{ t(`dbs.threshold.${engine}.${log.kind}`) }}</span>
            <el-input v-model="logForm[log.kind].threshold" size="small" class="threshold lp-mono" />
          </div>
          <div class="log__actions">
            <el-button v-if="log.path" link type="primary" size="small" @click="viewLog(log.kind)">{{ t('dbs.viewLog') }}</el-button>
            <el-button v-if="log.path && log.kind !== 'binlog'" link size="small" @click="clearLog(log.kind)">{{ t('dbs.clearLog') }}</el-button>
            <el-button v-if="log.kind === 'binlog' && log.enabled" link size="small" :loading="busy === 'purge'" @click="purge">{{ t('dbs.purgeBinlog') }}</el-button>
          </div>
        </div>
        <div class="save-bar">
          <span class="hint">{{ t('dbs.restartNote') }}</span>
          <el-button type="primary" :loading="busy === 'logs'" :disabled="!logsChanged" @click="saveLogs">{{ t('dbs.saveRestart') }}</el-button>
        </div>
      </el-tab-pane>

      <!-- ===== Config files ===== -->
      <el-tab-pane :label="t('dbs.tabConfig')" name="config">
        <div class="cfg-files">
          <el-radio-group v-if="configFiles.length > 1" v-model="configFile" size="small" @change="loadConfig">
            <el-radio-button v-for="f in configFiles" :key="f.id" :value="f.id">{{ t(`dbs.file.${f.id}`) }}</el-radio-button>
          </el-radio-group>
          <span class="lp-mono lp-dim cfg-path">{{ configPath }}</span>
        </div>
        <el-input v-model="configText" type="textarea" :rows="22" class="cfg-editor" spellcheck="false" />
        <div class="save-bar">
          <span class="hint">{{ t('dbs.configNote') }}</span>
          <el-button :loading="busy === 'config-load'" @click="loadConfig">{{ t('dbs.reload') }}</el-button>
          <el-button type="primary" :loading="busy === 'config'" :disabled="configText === configOriginal" @click="saveConfig">{{ t('dbs.saveRestart') }}</el-button>
        </div>
      </el-tab-pane>
    </el-tabs>

    <el-dialog v-model="logDialog" :title="logTitle" width="860px" append-to-body top="6vh">
      <pre class="log-pre">{{ logText || t('rt.noLogs') }}</pre>
      <template #footer>
        <el-button size="small" :loading="busy === 'log'" @click="viewLog(logKind)">{{ t('refresh') }}</el-button>
        <el-button size="small" @click="logDialog = false">{{ t('close') }}</el-button>
      </template>
    </el-dialog>
  </el-drawer>
</template>

<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  CircleCheckFilled, CircleCloseFilled, CopyDocument, Hide, InfoFilled, View, WarningFilled,
} from '@element-plus/icons-vue'
import {
  applyDbSettings, clearDbLog, getDbConfig, getDbLog, getDbNetwork, getDbSettings, purgeBinlogs,
  saveDbConfig, setDbRemote, setDbSuperuserPassword,
} from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes } from '@/utils/format'

type Kind = 'mysql' | 'postgresql' | 'redis'
interface Param { key: string; value: string; kind: string; options?: string[] }
interface LogInfo {
  kind: string; enabled: boolean; switchable: boolean; path: string | null; size: number | null; threshold?: string | null
}
interface Settings {
  engine: Kind; flavor: string; version: string; running: boolean; port: number
  data_dir: string | null; data_size: number | null
  remote: { enabled: boolean; needs_password: boolean }
  superuser: string; superuser_password: string | null
  params: Param[]; logs: LogInfo[]
}
interface NetworkInfo {
  port: number
  listeners: { address: string; process: string }[]
  public: boolean
  foreign_listener: string | null
  firewall: { backend: string; enabled: boolean; allowed_from: string[] }
  your_ip: string
}

const props = defineProps<{
  modelValue: boolean
  engine: Kind
  displayName: string
  /** Panel-managed database users, for the remote-access diagnosis. */
  users?: { username: string | null; host: string | null }[]
  initialTab?: string
}>()
const emit = defineEmits<{ 'update:modelValue': [boolean]; changed: [] }>()
const { t } = useI18n()

const s = ref<Settings | null>(null)
const network = ref<NetworkInfo | null>(null)
const tab = ref('conn')
const busy = ref('')
const drawerSize = computed(() => (window.innerWidth < 860 ? '100%' : '800px'))

const superuserLabel = computed(() =>
  props.engine === 'mysql' ? 'root@localhost' : props.engine === 'postgresql' ? 'postgres' : 'requirepass',
)

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
    // Clipboard blocked: the value is on screen anyway.
  }
}

// ----- Load -----
async function load() {
  tab.value = props.initialTab || 'conn'
  remoteAsk.value = false
  newPassword.value = ''
  showPassword.value = false
  s.value = null
  network.value = null
  const [res, net]: any[] = await Promise.all([
    getDbSettings(props.engine).catch(() => null),
    getDbNetwork(props.engine).catch(() => null),
  ])
  if (res?.success) {
    s.value = res.data
    resetForms()
  }
  network.value = net?.success ? net.data : null
  void loadConfig()
}

async function reload() {
  const [res, net]: any[] = await Promise.all([
    getDbSettings(props.engine).catch(() => null),
    getDbNetwork(props.engine).catch(() => null),
  ])
  if (res?.success) {
    s.value = res.data
    resetForms()
  }
  network.value = net?.success ? net.data : null
  emit('changed')
}

// ----- Remote access -----
const remoteAsk = ref(false)
const remoteFrom = ref('')

function toggleRemote(on: boolean) {
  if (on) {
    remoteFrom.value = ''
    remoteAsk.value = true
  } else {
    void submitRemote(false)
  }
}

async function submitRemote(enabled: boolean) {
  if (!enabled) {
    try {
      await ElMessageBox.confirm(t('dbm.remoteDisableConfirm'), t('dbs.remote'), { type: 'warning' })
    } catch {
      return
    }
  }
  busy.value = 'remote'
  try {
    const res: any = await setDbRemote(props.engine, { enabled, allow_from: remoteFrom.value.trim() })
    if (res.success) {
      remoteAsk.value = false
      ElMessage.success(enabled ? t('dbm.remoteOn') : t('dbm.remoteOff'))
    }
    await reload()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

type Check = { key: string; level: 'ok' | 'warn' | 'bad'; title: string; detail: string }
const checks = computed<Check[]>(() => {
  const n = network.value
  if (!n) return []
  const list: Check[] = []
  const addresses = n.listeners.map((l) => l.address).join(', ') || '—'
  if (n.foreign_listener) {
    list.push({ key: 'port', level: 'bad', title: t('dbm.chkPortTaken', { process: n.foreign_listener }), detail: t('dbm.chkPortTakenDetail') })
  }
  list.push(n.public
    ? { key: 'listen', level: 'ok', title: t('dbm.chkListenPublic'), detail: addresses }
    : { key: 'listen', level: s.value?.remote.enabled ? 'bad' : 'ok', title: t('dbm.chkListenLocal'), detail: t('dbm.chkListenLocalDetail', { addr: addresses }) })
  if (!n.firewall.enabled) {
    list.push({ key: 'fw', level: 'ok', title: t('dbm.chkFwOff'), detail: n.firewall.backend === 'none' ? t('dbm.chkFwNone') : n.firewall.backend })
  } else if (n.firewall.allowed_from.length) {
    list.push({ key: 'fw', level: 'ok', title: t('dbm.chkFwOpen'), detail: n.firewall.allowed_from.join(', ') })
  } else {
    list.push({ key: 'fw', level: n.public ? 'bad' : 'warn', title: t('dbm.chkFwClosed', { port: n.port }), detail: t('dbm.chkFwClosedDetail') })
  }
  if (props.engine === 'mysql' && props.users?.length) {
    const remoteUsers = props.users.filter((r) => r.host && r.host !== 'localhost')
    list.push(remoteUsers.length
      ? { key: 'users', level: 'ok', title: t('dbm.chkUsersRemote', { n: remoteUsers.length }), detail: remoteUsers.map((r) => `${r.username}@${r.host}`).join(', ') }
      : { key: 'users', level: 'warn', title: t('dbm.chkUsersLocal'), detail: t('dbm.chkUsersLocalDetail') })
  }
  if (props.engine === 'redis' && n.public) {
    list.push(s.value?.superuser_password
      ? { key: 'auth', level: 'ok', title: t('dbs.chkRedisAuth'), detail: '' }
      : { key: 'auth', level: 'bad', title: t('dbs.chkRedisNoAuth'), detail: t('dbs.redisNeedsPassword') })
  }
  list.push({ key: 'cloud', level: 'warn', title: t('dbm.chkCloud', { port: n.port }), detail: t('dbm.chkCloudDetail') })
  return list
})

// ----- Password -----
const newPassword = ref('')
const showPassword = ref(false)

async function savePassword() {
  const value = newPassword.value.trim()
  if (props.engine === 'mysql') {
    try {
      await ElMessageBox.confirm(t('dbs.rootConfirm'), t('dbs.tabPassword'), { type: 'warning' })
    } catch {
      return
    }
  }
  busy.value = 'password'
  try {
    const res: any = await setDbSuperuserPassword(props.engine, value)
    if (res.success) {
      ElMessage.success(t('dbs.passwordSaved'))
      newPassword.value = ''
      await reload()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// ----- Parameters -----
type ParamField = { num: number; unit: 'M' | 'G'; bool: boolean; text: string }
const paramForm = reactive<Record<string, ParamField>>({})

function toField(p: Param): ParamField {
  const field: ParamField = { num: 0, unit: 'M', bool: false, text: p.value }
  if (p.kind === 'size') {
    const bytes = Number(p.value) || 0
    const gb = bytes / (1 << 30)
    if (bytes >= 1 << 30 && Number.isInteger(gb)) {
      field.num = gb
      field.unit = 'G'
    } else {
      field.num = Math.round(bytes / (1 << 20))
    }
  } else if (p.kind === 'bool') {
    field.bool = p.value === 'on'
  } else if (p.kind !== 'enum') {
    field.num = Number(p.value) || 0
  }
  return field
}

function fieldValue(p: Param): string {
  const f = paramForm[p.key]
  if (!f) return p.value
  if (p.kind === 'size') return `${f.num}${f.unit}`
  if (p.kind === 'bool') return f.bool ? 'on' : 'off'
  if (p.kind === 'enum') return f.text
  return String(f.num)
}

function changedParams() {
  const out: Record<string, string> = {}
  for (const p of s.value?.params || []) {
    const before = toField(p)
    const f = paramForm[p.key]
    if (!f) continue
    const same = p.kind === 'size'
      ? before.num * (before.unit === 'G' ? 1024 : 1) === f.num * (f.unit === 'G' ? 1024 : 1)
      : p.kind === 'bool' ? before.bool === f.bool
        : p.kind === 'enum' ? before.text === f.text : before.num === f.num
    if (!same) out[p.key] = fieldValue(p)
  }
  return out
}
const paramsChanged = computed(() => Object.keys(changedParams()).length > 0)

async function saveParams() {
  busy.value = 'params'
  try {
    const res: any = await applyDbSettings(props.engine, { params: changedParams() })
    if (res.success) ElMessage.success(t('dbs.applied'))
    await reload()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// ----- Logs -----
const logForm = reactive<Record<string, { enabled: boolean; threshold: string }>>({})

function changedLogs() {
  const out: Record<string, { enabled?: boolean; threshold?: string }> = {}
  for (const log of s.value?.logs || []) {
    const f = logForm[log.kind]
    if (!f) continue
    const change: { enabled?: boolean; threshold?: string } = {}
    if (log.switchable && f.enabled !== log.enabled) change.enabled = f.enabled
    if (log.threshold != null && f.threshold.trim() !== normThreshold(log.threshold)) change.threshold = f.threshold.trim()
    if (Object.keys(change).length) out[log.kind] = change
  }
  return out
}
const logsChanged = computed(() => Object.keys(changedLogs()).length > 0)

async function saveLogs() {
  const changes = changedLogs()
  if (changes.binlog?.enabled === false) {
    try {
      await ElMessageBox.confirm(t('dbs.binlogOffConfirm'), t('dbs.log.binlog'), { type: 'warning' })
    } catch {
      return
    }
  }
  busy.value = 'logs'
  try {
    const res: any = await applyDbSettings(props.engine, { logs: changes })
    if (res.success) ElMessage.success(t('dbs.applied'))
    await reload()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

const logDialog = ref(false)
const logKind = ref('')
const logText = ref('')
const logTitle = computed(() => (logKind.value ? t(`dbs.log.${logKind.value}`) : ''))

async function viewLog(kind: string) {
  logKind.value = kind
  logDialog.value = true
  busy.value = 'log'
  try {
    const res: any = await getDbLog(props.engine, kind, 500)
    logText.value = res.success ? res.data : ''
  } catch {
    logText.value = ''
  } finally {
    busy.value = ''
  }
}

async function clearLog(kind: string) {
  try {
    await ElMessageBox.confirm(t('dbs.clearConfirm', { name: t(`dbs.log.${kind}`) }), t('dbs.clearLog'), { type: 'warning' })
  } catch {
    return
  }
  const res: any = await clearDbLog(props.engine, kind).catch(() => null)
  if (res?.success) {
    ElMessage.success(t('dbs.cleared'))
    await reload()
  }
}

async function purge() {
  try {
    await ElMessageBox.confirm(t('dbs.purgeConfirm'), t('dbs.purgeBinlog'), { type: 'warning' })
  } catch {
    return
  }
  busy.value = 'purge'
  try {
    const res: any = await purgeBinlogs()
    if (res.success) ElMessage.success(t('dbs.purged', { size: formatBytes(res.data || 0) }))
    await reload()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

/** "2.000000" -> "2" (MySQL reports long_query_time with six decimals). */
function normThreshold(value: string) {
  const text = String(value).trim()
  const num = Number(text)
  return text !== '' && Number.isFinite(num) ? String(num) : text
}

function resetForms() {
  for (const key of Object.keys(paramForm)) delete paramForm[key]
  for (const p of s.value?.params || []) paramForm[p.key] = toField(p)
  for (const key of Object.keys(logForm)) delete logForm[key]
  for (const log of s.value?.logs || []) {
    logForm[log.kind] = { enabled: log.enabled, threshold: log.threshold != null ? normThreshold(log.threshold) : '' }
  }
}

// ----- Config files -----
const configFiles = ref<{ id: string; path: string }[]>([])
const configFile = ref('')
const configPath = ref('')
const configText = ref('')
const configOriginal = ref('')

async function loadConfig() {
  busy.value = busy.value || 'config-load'
  try {
    const res: any = await getDbConfig(props.engine, configFile.value || undefined)
    if (res.success) {
      configFiles.value = res.data.files || []
      configFile.value = res.data.file?.id || ''
      configPath.value = res.data.file?.path || ''
      configText.value = res.data.content || ''
      configOriginal.value = configText.value
    }
  } catch {
    // Interceptor reports it.
  } finally {
    if (busy.value === 'config-load') busy.value = ''
  }
}

async function saveConfig() {
  try {
    await ElMessageBox.confirm(t('dbs.configConfirm'), t('dbs.tabConfig'), { type: 'warning' })
  } catch {
    return
  }
  busy.value = 'config'
  try {
    const res: any = await saveDbConfig(props.engine, configFile.value, configText.value)
    if (res.success) {
      ElMessage.success(t('dbs.applied'))
      configOriginal.value = configText.value
    }
    await reload()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}
</script>

<style scoped lang="scss">
.dbs-head { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.dbs-head__title { font-size: var(--lp-text-lg); font-weight: 650; color: var(--lp-text); }
.dbs-head__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 14px;
  font-size: var(--lp-text-sm);
  color: var(--lp-text-dim);
}
.dbs-tabs :deep(.el-tabs__content) { padding-top: var(--lp-2); }

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--lp-4);
  padding: var(--lp-4);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r);
  background: var(--lp-surface);
}
.row__title { font-weight: 600; }
.row__desc { margin-top: 2px; font-size: var(--lp-text-sm); color: var(--lp-text-dim); line-height: 1.6; }
.block { margin-top: var(--lp-3); }

.ask {
  margin-top: var(--lp-3);
  padding: var(--lp-4);
  border: 1px solid var(--lp-accent);
  border-radius: var(--lp-r);
  background: var(--lp-accent-soft);
}
.ask__label { margin-bottom: 6px; font-weight: 600; }
.ask__actions { display: flex; justify-content: flex-end; gap: var(--lp-2); margin-top: var(--lp-3); }

.section { margin: var(--lp-5) 0 var(--lp-3); font-size: var(--lp-text-md); font-weight: 600; }
.checks { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--lp-3); }
.check { display: flex; gap: var(--lp-3); align-items: flex-start; }
.check__icon { margin-top: 2px; font-size: 17px; }
.check__icon.is-ok { color: var(--lp-ok); }
.check__icon.is-warn { color: var(--lp-warn); }
.check__icon.is-bad { color: var(--lp-danger); }
.check__text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.check__title { font-weight: 500; }
.check__detail { font-size: var(--lp-text-sm); color: var(--lp-text-dim); word-break: break-all; }
.tip {
  display: flex;
  gap: var(--lp-2);
  margin-top: var(--lp-4);
  padding: var(--lp-3);
  border-radius: var(--lp-r-sm);
  background: var(--lp-surface-sunken);
  font-size: var(--lp-text-sm);
  color: var(--lp-text-soft);
}

.kv {
  display: grid;
  gap: var(--lp-2);
  margin: 0 0 var(--lp-5);
  padding: var(--lp-4);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r);
  > div { display: flex; align-items: center; gap: var(--lp-4); }
  dt { width: 96px; flex: none; color: var(--lp-text-dim); }
  dd { margin: 0; display: flex; align-items: center; gap: 4px; }
}
.field { margin-bottom: var(--lp-4); }
.field__label { margin-bottom: 6px; font-weight: 500; }
.inline-row { display: flex; gap: var(--lp-2); }
.hint { margin: 6px 0 0; font-size: var(--lp-text-sm); color: var(--lp-text-dim); line-height: 1.6; }
.intro { margin: 0 0 var(--lp-4); color: var(--lp-text-soft); line-height: 1.6; }

.params { display: flex; flex-direction: column; }
.param {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--lp-4);
  padding: var(--lp-3) 0;
  border-bottom: 1px solid var(--lp-line-soft);
}
.param__name { font-weight: 500; }
.param__key { margin-left: 6px; font-size: var(--lp-text-xs); }
.param__desc { margin-top: 2px; font-size: var(--lp-text-sm); color: var(--lp-text-dim); line-height: 1.5; }
.param__input { display: flex; gap: 6px; flex: none; }
.num { width: 150px; }
.unit { width: 78px; }
.enum { width: 180px; }

.log {
  padding: var(--lp-4);
  margin-bottom: var(--lp-3);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r);
}
.log__head { display: flex; align-items: flex-start; justify-content: space-between; gap: var(--lp-4); }
.log__name { display: flex; align-items: center; gap: var(--lp-2); font-weight: 600; }
.log__desc { margin-top: 4px; font-size: var(--lp-text-sm); color: var(--lp-text-dim); line-height: 1.6; }
.log__path { margin-top: 4px; font-size: var(--lp-text-xs); color: var(--lp-text-faint); word-break: break-all; }
.log__threshold { display: flex; align-items: center; gap: var(--lp-2); margin-top: var(--lp-3); font-size: var(--lp-text-sm); }
.threshold { width: 110px; }
.log__actions { display: flex; gap: var(--lp-2); margin-top: var(--lp-2); }
.log__actions:empty { display: none; }

.save-bar {
  position: sticky;
  bottom: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--lp-3);
  margin-top: var(--lp-4);
  padding: var(--lp-3) 0;
  background: var(--lp-surface);
  border-top: 1px solid var(--lp-line-soft);
  .hint { margin: 0 auto 0 0; }
}

.cfg-files { display: flex; align-items: center; flex-wrap: wrap; gap: var(--lp-3); margin-bottom: var(--lp-3); }
.cfg-path { font-size: var(--lp-text-sm); word-break: break-all; }
.cfg-editor :deep(textarea) {
  font-family: var(--lp-font-mono);
  font-size: 12.5px;
  line-height: 1.6;
  white-space: pre;
  overflow-wrap: normal;
  overflow-x: auto;
}
.log-pre {
  max-height: 64vh;
  margin: 0;
  padding: var(--lp-3);
  overflow: auto;
  border-radius: var(--lp-r-sm);
  background: #0f1218;
  color: #d6dbe4;
  font-family: var(--lp-font-mono);
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
  word-break: break-all;
}

@media (max-width: 640px) {
  .param { flex-direction: column; align-items: stretch; }
}
</style>
