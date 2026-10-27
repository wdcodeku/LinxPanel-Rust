<template>
  <div class="setting-stack">
    <el-alert
      v-if="s?.restart_required"
      type="warning"
      :closable="false"
      show-icon
      :title="t('st.restartRequired')"
    >
      <el-button size="small" type="warning" :loading="restarting" @click="restartPanel">
        {{ t('st.restartPanel') }}
      </el-button>
    </el-alert>

    <section v-for="group in groups" :key="group.key" class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title">
          <el-icon><component :is="group.icon" /></el-icon>{{ group.title }}
        </h2>
        <span v-if="group.key === 'access' && s" class="lp-pill" :class="s.ssl_active ? 'lp-pill--ok' : ''">
          <span class="lp-pill__dot"></span>
          {{ s.ssl_active ? 'HTTPS' : 'HTTP' }}
        </span>
      </header>
      <div v-loading="!s" class="sec-list">
        <div v-for="row in group.rows" :key="row.key" class="sec-row">
          <div class="sec-row__main">
            <div class="sec-row__label">{{ row.label }}</div>
            <div class="sec-row__desc">{{ row.desc }}</div>
          </div>
          <div class="sec-row__value lp-mono" :class="row.warn ? 'is-warn' : ''">{{ row.value }}</div>
          <div class="sec-row__action">
            <el-switch
              v-if="row.key === 'mfa'"
              :model-value="s?.mfa_enabled"
              @change="openDialog(s?.mfa_enabled ? 'mfa-off' : 'mfa-on')"
            />
            <el-button v-else size="small" @click="openDialog(row.key)">{{ t('ps.set') }}</el-button>
          </div>
        </div>
      </div>
    </section>

    <section class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Warning /></el-icon>{{ t('st.exposureTitle') }}</h2>
      </header>
      <div class="lp-card-body">
        <ul class="check-list">
          <li v-for="check in securityChecks" :key="check.key" class="check">
            <span class="check__dot" :class="`check__dot--${check.level}`"></span>
            <div class="check__body">
              <span class="check__label">{{ check.label }}</span>
              <span class="check__detail">{{ check.detail }}</span>
            </div>
          </li>
        </ul>
      </div>
    </section>

    <el-dialog v-model="dialog.visible" :title="dialogTitle" width="520px" append-to-body destroy-on-close>
      <el-form label-position="top" @submit.prevent="submit">
        <template v-if="dialog.key === 'port'">
          <el-form-item :label="t('ps.port')">
            <el-input-number v-model="edit.port" :min="1" :max="65535" controls-position="right" style="width: 100%" />
            <span class="hint">{{ t('ps.portHint') }}</span>
          </el-form-item>
        </template>

        <template v-else-if="dialog.key === 'entrance'">
          <el-form-item :label="t('ps.entrance')">
            <div class="inline-row">
              <el-input v-model="edit.entrance" placeholder="/a1b2c3d4" class="lp-mono" clearable />
              <el-button @click="randomizeEntrance">{{ t('st.entranceRandom') }}</el-button>
            </div>
            <span class="hint">{{ t('st.entranceHint') }}</span>
            <span v-if="edit.entrance" class="hint">
              {{ t('ps.entrancePreview') }}<code class="lp-mono strong">{{ panelUrl(s!.ssl_active, s!.active_port, normalizedEntrance) }}</code>
            </span>
          </el-form-item>
        </template>

        <template v-else-if="dialog.key === 'ssl'">
          <el-form-item :label="t('ps.ssl')">
            <el-switch v-model="edit.sslEnabled" />
          </el-form-item>
          <el-form-item v-if="edit.sslEnabled" :label="t('ps.sslCert')">
            <el-select v-model="edit.certId" style="width: 100%">
              <el-option :label="t('st.panelSslSelfSigned')" :value="0" />
              <el-option v-for="c in certs" :key="c.id" :label="`${c.name} (${c.domains})`" :value="c.id" />
            </el-select>
            <el-checkbox v-if="!edit.certId" v-model="edit.regenerate" class="mt">{{ t('ps.sslRegenerate') }}</el-checkbox>
            <span class="hint">{{ t('st.panelSslHint') }}</span>
          </el-form-item>
        </template>

        <template v-else-if="dialog.key === 'ips'">
          <el-form-item :label="t('ps.allowedIps')">
            <el-input v-model="edit.ips" type="textarea" :rows="5" :placeholder="t('ps.ipsPlaceholder')" class="lp-mono" />
            <div class="inline-row mt">
              <span class="hint hint--inline">{{ t('ps.myIp', { ip: s?.client_ip }) }}</span>
              <el-button size="small" link type="primary" @click="addMyIp">{{ t('ps.addMyIp') }}</el-button>
            </div>
            <span class="hint">{{ t('ps.ipsHint') }}</span>
          </el-form-item>
        </template>

        <template v-else-if="dialog.key === 'domain'">
          <el-form-item :label="t('ps.domain')">
            <el-input v-model="edit.domain" :placeholder="t('ps.domainPlaceholder')" class="lp-mono" clearable />
            <span class="hint">{{ t('ps.domainHint') }}</span>
          </el-form-item>
        </template>

        <template v-else-if="dialog.key === 'deny'">
          <el-radio-group v-model="edit.deny" class="deny-options">
            <el-radio v-for="mode in denyModes" :key="mode.value" :value="mode.value" border>
              <span class="deny-option__label">{{ mode.label }}</span>
              <span class="deny-option__desc">{{ mode.desc }}</span>
            </el-radio>
          </el-radio-group>
        </template>

        <template v-else-if="dialog.key === 'session'">
          <el-form-item :label="t('ps.session')">
            <el-select v-model="edit.session" style="width: 100%">
              <el-option :label="t('st.hours1')" :value="1" />
              <el-option :label="t('st.hours6')" :value="6" />
              <el-option :label="t('st.hours12')" :value="12" />
              <el-option :label="t('st.hours24')" :value="24" />
              <el-option :label="t('st.days7')" :value="168" />
            </el-select>
          </el-form-item>
        </template>

        <template v-else-if="dialog.key === 'username'">
          <el-form-item :label="t('ps.newUsername')">
            <el-input v-model="edit.username" autocomplete="username" />
            <span class="hint">{{ t('ps.usernameHint') }}</span>
          </el-form-item>
          <el-form-item :label="t('ps.currentPassword')">
            <el-input v-model="edit.password" type="password" show-password autocomplete="current-password" />
          </el-form-item>
        </template>

        <template v-else-if="dialog.key === 'password'">
          <el-form-item :label="t('st.oldPassword')">
            <el-input v-model="edit.oldPassword" type="password" show-password autocomplete="current-password" />
          </el-form-item>
          <el-form-item :label="t('st.newPassword')">
            <el-input v-model="edit.newPassword" type="password" show-password autocomplete="new-password" />
            <span class="hint">{{ t('st.passwordTooShort') }}</span>
          </el-form-item>
          <el-form-item :label="t('st.confirmPassword')">
            <el-input v-model="edit.confirmPassword" type="password" show-password autocomplete="new-password" />
          </el-form-item>
        </template>

        <template v-else-if="dialog.key === 'mfa-on'">
          <p class="section-note">{{ t('ps.mfaScan') }}</p>
          <div class="mfa-setup">
            <img v-if="mfa.qr" :src="mfa.qr" alt="QR" class="mfa-qr" />
            <code class="lp-mono mfa-secret">{{ mfa.secret }}</code>
          </div>
          <el-form-item :label="t('ps.mfaCode')">
            <el-input v-model="edit.code" maxlength="6" inputmode="numeric" autocomplete="one-time-code" class="lp-mono" />
            <span class="hint">{{ t('ps.mfaEnableHint') }}</span>
            <span class="hint">{{ t('ps.mfaRecovery') }}</span>
          </el-form-item>
        </template>

        <template v-else-if="dialog.key === 'mfa-off'">
          <el-form-item :label="t('ps.mfaCode')">
            <el-input v-model="edit.code" maxlength="6" inputmode="numeric" autocomplete="one-time-code" class="lp-mono" />
            <span class="hint">{{ t('ps.mfaDisableHint') }}</span>
          </el-form-item>
        </template>
      </el-form>

      <template #footer>
        <el-button @click="dialog.visible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="dialog.saving" @click="submit">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { usePage } from '@/utils/page'
import { storeToRefs } from 'pinia'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Key, User, Warning } from '@element-plus/icons-vue'
import QRCode from 'qrcode'
import {
  changePasswordApi, changeUsernameApi, getCerts, getPanelAccess, mfaDisableApi, mfaEnableApi,
  mfaSetupApi, restartPanelApi, setAllowedIpsApi, setBindDomainApi, setDenyResponseApi,
  setEntranceApi, setPanelPortApi, setPanelSslApi, updateSettings,
} from '@/api'
import { useI18n } from '@/i18n'
import { useRealtimeStore } from '@/stores/realtime'
import { useUserStore } from '@/stores/user'

interface PanelSecurityStatus {
  port: number
  active_port: number
  entrance: string | null
  ssl_enabled: boolean
  ssl_active: boolean
  ssl_cert_id: number | null
  self_signed: boolean
  cert: { not_after: string; days_left: number | null } | null
  allowed_ips: string
  bind_domain: string | null
  deny_response: string
  client_ip: string
  session_timeout: number
  username: string
  mfa_enabled: boolean
  restart_required: boolean
  restart_supported: boolean
}

type DialogKey =
  | 'port' | 'entrance' | 'ssl' | 'ips' | 'domain' | 'deny'
  | 'session' | 'username' | 'password' | 'mfa-on' | 'mfa-off'

const { t } = useI18n()
const userStore = useUserStore()
const { detail } = storeToRefs(useRealtimeStore())

const s = ref<PanelSecurityStatus | null>(null)
const certs = ref<any[]>([])
const restarting = ref(false)

async function load() {
  const [statusRes, certsRes]: any[] = await Promise.all([
    getPanelAccess(),
    getCerts().catch(() => ({ success: false })),
  ])
  if (statusRes.success) s.value = statusRes.data
  if (certsRes.success) certs.value = certsRes.data || []
}

/** The URL the panel answers at for a given scheme, port and entrance. */
function panelUrl(https: boolean, port: number, entrance: string | null) {
  return `${https ? 'https' : 'http'}://${window.location.hostname}:${port}${entrance || '/'}`
}

const denyModes = computed(() => [
  { value: 'hint', label: t('ps.denyHint'), desc: t('ps.denyHintDesc') },
  { value: '404', label: t('ps.deny404'), desc: t('ps.denyPlainDesc') },
  { value: '403', label: t('ps.deny403'), desc: t('ps.denyPlainDesc') },
  { value: '400', label: t('ps.deny400'), desc: t('ps.denyPlainDesc') },
])

const groups = computed(() => {
  const v = s.value
  const pending = (value: string, changed: boolean) =>
    changed ? t('ps.pendingRestart', { value }) : value
  const sslValue = !v?.ssl_enabled
    ? t('ps.sslOff')
    : `${t('ps.sslOn')}${v.self_signed ? ` · ${t('ps.sslSelfSigned')}` : ''}${
      v.cert ? ` · ${t('ps.sslExpires', { date: v.cert.not_after, days: v.cert.days_left ?? '—' })}` : ''}`
  return [
    {
      key: 'access',
      title: t('ps.accessTitle'),
      icon: Key,
      rows: [
        {
          key: 'port', label: t('ps.port'), desc: t('ps.portDesc'),
          value: v ? pending(String(v.port), v.port !== v.active_port) : '',
        },
        {
          key: 'entrance', label: t('ps.entrance'), desc: t('ps.entranceDesc'),
          value: v?.entrance || t('ps.notSet'), warn: !!v && !v.entrance,
        },
        {
          key: 'ssl', label: t('ps.ssl'), desc: t('ps.sslDesc'),
          value: v ? pending(sslValue, v.ssl_enabled !== v.ssl_active) : '', warn: !!v && !v.ssl_enabled,
        },
        {
          key: 'ips', label: t('ps.allowedIps'), desc: t('ps.allowedIpsDesc'),
          value: v?.allowed_ips || t('ps.unlimited'),
        },
        {
          key: 'domain', label: t('ps.domain'), desc: t('ps.domainDesc'),
          value: v?.bind_domain || t('ps.notBound'),
        },
        {
          key: 'deny', label: t('ps.deny'), desc: t('ps.denyDesc'),
          value: denyModes.value.find((m) => m.value === v?.deny_response)?.label || '',
        },
      ],
    },
    {
      key: 'account',
      title: t('ps.accountTitle'),
      icon: User,
      rows: [
        {
          key: 'username', label: t('ps.username'), desc: t('ps.usernameDesc'),
          value: v?.username || '', warn: v?.username === 'admin',
        },
        { key: 'password', label: t('ps.password'), desc: t('ps.passwordDesc'), value: '••••••••' },
        {
          key: 'mfa', label: t('ps.mfa'), desc: t('ps.mfaDesc'),
          value: v?.mfa_enabled ? t('ps.on') : t('ps.off'), warn: !!v && !v.mfa_enabled,
        },
        {
          key: 'session', label: t('ps.session'), desc: t('ps.sessionDesc'),
          value: v ? t('ps.hours', { n: v.session_timeout }) : '',
        },
      ],
    },
  ]
})

/** Read-only posture summary built from data the panel already has. */
const securityChecks = computed(() => {
  const v = s.value
  const checks: { key: string; label: string; detail: string; level: 'ok' | 'warn' }[] = []
  const add = (key: string, label: string, ok: boolean, okText: string, warnText: string) =>
    checks.push({ key, label, detail: ok ? okText : warnText, level: ok ? 'ok' : 'warn' })

  add('account', t('st.checkDefaultAccount'), v?.username !== 'admin',
    t('st.checkDefaultAccountOk'), t('st.checkDefaultAccountWarn'))
  add('https', t('st.checkHttps'), window.location.protocol === 'https:',
    t('st.checkHttpsOk'), t('st.checkHttpsWarn'))
  add('entrance', t('st.checkEntrance'), !!v?.entrance,
    t('st.checkEntranceOk'), t('st.checkEntranceWarn'))
  add('mfa', t('ps.mfa'), !!v?.mfa_enabled, t('ps.mfaEnabled'), t('ps.mfaDesc'))
  const hours = v?.session_timeout ?? 24
  add('session', t('st.checkSession'), hours <= 24,
    t('st.checkSessionDetail', { hours }), t('st.checkSessionDetail', { hours }))
  checks.push({
    key: 'interfaces',
    label: t('st.checkInterfaces'),
    detail: t('st.checkInterfacesDetail', { n: detail.value?.interfaces?.length ?? 0 }),
    level: 'ok',
  })
  return checks
})

// --- Dialog ---
const dialog = reactive({ visible: false, key: 'port' as DialogKey, saving: false })
const edit = reactive({
  port: 7200,
  entrance: '',
  sslEnabled: false,
  certId: 0,
  regenerate: false,
  ips: '',
  domain: '',
  deny: 'hint',
  session: 24,
  username: '',
  password: '',
  oldPassword: '',
  newPassword: '',
  confirmPassword: '',
  code: '',
})
const mfa = reactive({ secret: '', qr: '' })

const dialogTitle = computed(() => {
  const titles: Record<DialogKey, string> = {
    port: t('ps.port'), entrance: t('ps.entrance'), ssl: t('ps.ssl'), ips: t('ps.allowedIps'),
    domain: t('ps.domain'), deny: t('ps.deny'), session: t('ps.session'), username: t('ps.username'),
    password: t('ps.password'), 'mfa-on': t('ps.mfa'), 'mfa-off': t('ps.mfa'),
  }
  return titles[dialog.key]
})

const normalizedEntrance = computed(() => {
  const value = edit.entrance.trim().replace(/^\/+|\/+$/g, '')
  return value ? `/${value}` : null
})

async function openDialog(key: string) {
  const v = s.value
  if (!v) return
  Object.assign(edit, {
    port: v.port,
    entrance: v.entrance || '',
    sslEnabled: v.ssl_enabled,
    certId: v.ssl_cert_id || 0,
    regenerate: false,
    ips: v.allowed_ips.split(',').filter(Boolean).join('\n'),
    domain: v.bind_domain || '',
    deny: v.deny_response,
    session: v.session_timeout,
    username: v.username,
    password: '',
    oldPassword: '',
    newPassword: '',
    confirmPassword: '',
    code: '',
  })
  if (key === 'mfa-on') {
    const res: any = await mfaSetupApi().catch(() => null)
    if (!res?.success) return
    mfa.secret = res.data.secret
    mfa.qr = await QRCode.toDataURL(res.data.uri, { width: 200, margin: 1 })
  }
  dialog.key = key as DialogKey
  dialog.visible = true
}

function randomizeEntrance() {
  const alphabet = 'abcdefghijkmnpqrstuvwxyz23456789'
  const bytes = crypto.getRandomValues(new Uint8Array(10))
  edit.entrance = '/' + Array.from(bytes, (b) => alphabet[b % alphabet.length]).join('')
}

function addMyIp() {
  const ip = s.value?.client_ip
  if (!ip) return
  const lines = edit.ips.split('\n').map((l) => l.trim()).filter(Boolean)
  if (!lines.includes(ip)) lines.push(ip)
  edit.ips = lines.join('\n')
}

/** Runs the request for the open dialog; returns the API response. */
async function save(): Promise<any> {
  switch (dialog.key) {
    case 'port': return setPanelPortApi(edit.port)
    case 'entrance': return setEntranceApi(edit.entrance)
    case 'ssl': return setPanelSslApi({
      enabled: edit.sslEnabled, cert_id: edit.certId || null, regenerate: edit.regenerate,
    })
    case 'ips': return setAllowedIpsApi(edit.ips)
    case 'domain': return setBindDomainApi(edit.domain)
    case 'deny': return setDenyResponseApi(edit.deny)
    case 'session': return updateSettings({ session_timeout: String(edit.session) })
    case 'username': {
      const res: any = await changeUsernameApi({ username: edit.username, password: edit.password })
      if (res.success && res.data?.token) userStore.setLogin(res.data.token, res.data.username)
      return res
    }
    case 'password': {
      if (edit.newPassword !== edit.confirmPassword) {
        ElMessage.warning(t('st.passwordMismatch'))
        return null
      }
      const res: any = await changePasswordApi({
        username: userStore.username, old_password: edit.oldPassword, new_password: edit.newPassword,
      })
      // Other sessions were revoked; the response carries this session's new token.
      if (res.success && res.data?.token) userStore.setLogin(res.data.token, userStore.username)
      return res
    }
    case 'mfa-on': return mfaEnableApi(edit.code)
    case 'mfa-off': return mfaDisableApi(edit.code)
  }
}

const successText: Partial<Record<DialogKey, string>> = {
  username: 'ps.usernameChanged',
  password: 'st.sessionsRevoked',
  'mfa-on': 'ps.mfaEnabled',
  'mfa-off': 'ps.mfaDisabled',
}

async function submit() {
  dialog.saving = true
  try {
    const res = await save()
    if (!res?.success) return
    ElMessage.success(t(successText[dialog.key] || 'ps.saved'))
    dialog.visible = false
    // Most endpoints answer with the fresh status; the rest need a reload.
    if (res.data && 'restart_required' in res.data) s.value = res.data
    else await load()
  } catch {
    // Interceptor reports it.
  } finally {
    dialog.saving = false
  }
}

async function restartPanel() {
  const v = s.value
  if (!v) return
  if (!v.restart_supported) {
    ElMessage.warning(t('st.restartManual'))
    return
  }
  try {
    await ElMessageBox.confirm(t('st.restartConfirm'), t('st.restartPanel'), { type: 'warning' })
  } catch {
    return
  }
  restarting.value = true
  // Captured now: scheme and port may change, and a new origin needs a new login.
  const target = panelUrl(v.ssl_enabled, v.port, v.entrance)
  try {
    const res: any = await restartPanelApi()
    if (!res.success) {
      restarting.value = false
      return
    }
    ElMessage.info(t('st.restarting'))
    window.setTimeout(() => { window.location.href = target }, 6000)
  } catch {
    restarting.value = false
  }
}

onMounted(load)
usePage(load)
</script>

<style scoped lang="scss">
.setting-stack { display: flex; flex-direction: column; gap: var(--lp-4); }

.sec-list { min-height: 60px; }
.sec-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, auto) auto;
  align-items: center;
  gap: var(--lp-4);
  padding: var(--lp-4) var(--lp-5);
  border-top: 1px solid var(--lp-line);

  &:first-child { border-top: none; }
}
.sec-row__label { font-size: var(--lp-text-base); font-weight: 550; color: var(--lp-text); }
.sec-row__desc { margin-top: 2px; font-size: var(--lp-text-sm); color: var(--lp-text-dim); line-height: 1.5; }
.sec-row__value {
  max-width: 360px;
  font-size: var(--lp-text-sm);
  color: var(--lp-text);
  text-align: right;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;

  &.is-warn { color: var(--lp-warn); }
}
.sec-row__action { display: flex; justify-content: flex-end; min-width: 56px; }

@media (max-width: 720px) {
  .sec-row { grid-template-columns: minmax(0, 1fr) auto; }
  .sec-row__value { grid-column: 1; grid-row: 2; text-align: left; max-width: none; }
  .sec-row__action { grid-row: 1 / span 2; grid-column: 2; }
}

.hint {
  display: block;
  flex-basis: 100%;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  line-height: 1.5;

  &--inline { display: inline; flex-basis: auto; margin-top: 0; }
}
.strong { color: var(--lp-text); font-weight: 500; }
.mt { margin-top: var(--lp-2); }
.section-note { margin-bottom: var(--lp-3); font-size: var(--lp-text-sm); color: var(--lp-text-dim); }

.inline-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--lp-2);
  width: 100%;

  .el-input { flex: 1 1 220px; min-width: 0; }
}

.deny-options {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: var(--lp-2);
  width: 100%;

  .el-radio { height: auto; margin: 0; padding: var(--lp-2) var(--lp-3); }
  :deep(.el-radio__label) { display: flex; flex-direction: column; white-space: normal; }
}
.deny-option__label { font-weight: 550; }
.deny-option__desc { font-size: var(--lp-text-xs); color: var(--lp-text-dim); }

.mfa-setup {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--lp-2);
  margin-bottom: var(--lp-4);
}
.mfa-qr { width: 200px; height: 200px; border-radius: var(--lp-r-md); background: #fff; }
.mfa-secret { font-size: var(--lp-text-sm); letter-spacing: 0.08em; word-break: break-all; text-align: center; }

/* Security checklist */
.check-list { display: flex; flex-direction: column; gap: var(--lp-3); }
.check { display: flex; align-items: flex-start; gap: var(--lp-3); }
.check__dot {
  width: 8px;
  height: 8px;
  margin-top: 6px;
  border-radius: 50%;
  flex: none;
  &--ok { background: var(--lp-ok); }
  &--warn { background: var(--lp-warn); }
}
.check__body { display: flex; flex-direction: column; min-width: 0; }
.check__label { font-size: var(--lp-text-base); font-weight: 500; color: var(--lp-text); }
.check__detail { font-size: var(--lp-text-sm); color: var(--lp-text-dim); }
</style>
