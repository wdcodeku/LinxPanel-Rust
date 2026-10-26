<template>
  <div class="stack">
    <!-- Status -->
    <div class="lp-card">
      <div class="lp-card-body tiles" v-loading="!data">
        <div class="tile">
          <div class="tile__label">{{ t('def.autoBan') }}</div>
          <div class="tile__value">
            <el-switch
              :model-value="!!form.enabled"
              :loading="saving"
              @change="(v: string | number | boolean) => toggleEnabled(v === true)"
            />
            <span class="lp-pill" :class="form.enabled ? 'lp-pill--ok' : 'lp-pill--warn'">
              <span class="lp-pill__dot"></span>{{ form.enabled ? t('sec.active') : t('sec.inactive') }}
            </span>
          </div>
        </div>
        <div class="tile">
          <div class="tile__label">{{ t('def.kernel') }}</div>
          <div class="tile__value">
            <template v-if="status?.nft_available">
              <span class="lp-pill lp-pill--ok"><span class="lp-pill__dot"></span>nftables</span>
            </template>
            <el-button v-else size="small" type="warning" :loading="installing" @click="installNft">
              {{ t('def.installNft') }}
            </el-button>
          </div>
        </div>
        <div class="tile">
          <div class="tile__label">{{ t('def.bannedNow') }}</div>
          <div class="tile__value lp-num">{{ status?.kernel_bans ?? bans.length }}</div>
        </div>
        <div class="tile">
          <div class="tile__label">{{ t('def.connections') }}</div>
          <div class="tile__value lp-num">{{ status?.open_connections ?? '-' }}</div>
        </div>
        <div class="tile">
          <div class="tile__label">{{ t('def.tracked') }}</div>
          <div class="tile__value lp-num">{{ status?.tracked_addresses ?? '-' }}</div>
        </div>
        <div class="tile">
          <div class="tile__label">{{ t('def.hardening') }}</div>
          <div class="tile__value">
            <span class="lp-pill" :class="status?.sysctl.syncookies ? 'lp-pill--ok' : 'lp-pill--warn'">
              <span class="lp-pill__dot"></span>
              {{ status?.sysctl.applied ? t('def.applied') : t('def.notApplied') }}
            </span>
            <el-button size="small" link type="primary" :loading="hardening" @click="applyHardening">
              {{ status?.sysctl.applied ? t('def.reapply') : t('def.apply') }}
            </el-button>
          </div>
        </div>
      </div>
    </div>

    <el-alert
      v-if="data && !status?.nft_available"
      type="warning"
      :closable="false"
      show-icon
      :title="t('def.noNft')"
    />

    <!-- Rules -->
    <div class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Aim /></el-icon>{{ t('def.rules') }}</h2>
        <el-button type="primary" size="small" :loading="saving" @click="save">{{ t('save') }}</el-button>
      </header>
      <div class="lp-card-body">
        <div class="rules">
          <div v-for="rule in ruleGroups" :key="rule.key" class="rule">
            <div class="rule__head">
              <el-switch v-model="form[rule.enabledKey]" size="small" />
              <span class="rule__title">{{ rule.title }}</span>
            </div>
            <p class="rule__desc">{{ rule.desc }}</p>
            <div class="rule__fields">
              <label v-for="field in rule.fields" :key="field.key" class="field">
                <span class="field__label">{{ field.label }}</span>
                <el-input-number
                  v-model="form[field.key]"
                  :min="field.min"
                  :max="field.max"
                  :disabled="!form[rule.enabledKey]"
                  size="small"
                  controls-position="right"
                />
              </label>
            </div>
          </div>
        </div>

        <div class="general">
          <label class="field">
            <span class="field__label">{{ t('def.banMinutes') }}</span>
            <el-input-number v-model="form.ban_minutes" :min="0" :max="525600" size="small" controls-position="right" />
            <span class="lp-dim hint">{{ t('def.banMinutesHint') }}</span>
          </label>
          <label class="field field--wide">
            <span class="field__label">{{ t('def.whitelist') }}</span>
            <el-input
              v-model="form.whitelist"
              type="textarea"
              :rows="3"
              :placeholder="t('ps.ipsPlaceholder')"
            />
            <span class="lp-dim hint">
              {{ t('def.whitelistHint') }}
              <el-button v-if="status?.your_ip" link type="primary" size="small" @click="addMyIp">
                {{ t('ps.addMyIp') }} ({{ status.your_ip }})
              </el-button>
            </span>
          </label>
        </div>
      </div>
    </div>

    <!-- Bans -->
    <div class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><CircleClose /></el-icon>{{ t('def.bans') }}</h2>
        <el-button size="small" :icon="Plus" @click="banVisible = true">{{ t('def.banIp') }}</el-button>
      </header>
      <div class="lp-card-body lp-card-body--flush">
        <el-table :data="bans" size="small" :empty-text="t('def.noBans')">
          <el-table-column :label="t('def.ip')" min-width="160">
            <template #default="{ row }"><span class="lp-mono">{{ row.ip }}</span></template>
          </el-table-column>
          <el-table-column :label="t('def.source')" width="90">
            <template #default="{ row }">
              <span class="lp-pill" :class="row.source === 'manual' ? 'lp-pill--accent' : 'lp-pill--danger'">
                {{ sourceLabel(row.source) }}
              </span>
            </template>
          </el-table-column>
          <el-table-column :label="t('def.reason')" prop="reason" min-width="200" show-overflow-tooltip />
          <el-table-column :label="t('def.bannedAt')" width="160">
            <template #default="{ row }">{{ formatTime(row.created_at) }}</template>
          </el-table-column>
          <el-table-column :label="t('def.remaining')" width="110">
            <template #default="{ row }">{{ remaining(row.expires_at) }}</template>
          </el-table-column>
          <el-table-column :label="t('proc.actions')" width="90" align="right">
            <template #default="{ row }">
              <el-button link type="primary" size="small" @click="unban(row.ip)">{{ t('def.unban') }}</el-button>
            </template>
          </el-table-column>
        </el-table>
      </div>
    </div>

    <!-- Events -->
    <div class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Document /></el-icon>{{ t('def.events') }}</h2>
      </header>
      <div class="lp-card-body lp-card-body--flush">
        <el-table :data="events" size="small" max-height="360" :empty-text="t('def.noEvents')">
          <el-table-column :label="t('def.time')" width="160">
            <template #default="{ row }">{{ formatTime(row.at) }}</template>
          </el-table-column>
          <el-table-column :label="t('def.ip')" min-width="150">
            <template #default="{ row }"><span class="lp-mono">{{ row.ip }}</span></template>
          </el-table-column>
          <el-table-column :label="t('def.source')" width="90">
            <template #default="{ row }">{{ sourceLabel(row.source) }}</template>
          </el-table-column>
          <el-table-column :label="t('sec.action')" width="90">
            <template #default="{ row }">
              <span class="lp-pill" :class="actionClass(row.action)">{{ actionLabel(row.action) }}</span>
            </template>
          </el-table-column>
          <el-table-column :label="t('def.detail')" prop="detail" min-width="220" show-overflow-tooltip />
        </el-table>
      </div>
    </div>

    <el-dialog v-model="banVisible" :title="t('def.banIp')" width="440px">
      <el-form label-position="top">
        <el-form-item :label="t('def.ip')">
          <el-input v-model="banForm.ip" placeholder="203.0.113.7" />
        </el-form-item>
        <el-form-item :label="t('def.banMinutes')">
          <el-input-number v-model="banForm.minutes" :min="0" :max="525600" controls-position="right" />
          <span class="lp-dim hint">{{ t('def.banMinutesHint') }}</span>
        </el-form-item>
        <el-form-item :label="t('def.reason')">
          <el-input v-model="banForm.reason" maxlength="200" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="banVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="banning" @click="banIp">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Aim, CircleClose, Document, Plus } from '@element-plus/icons-vue'
import {
  banDefenseIp, getDefense, getDefenseBans, getDefenseEvents, installNftables,
  saveDefense, setKernelHardening, unbanDefenseIp,
} from '@/api'
import type { DefenseConfig } from '@/api'
import { useI18n } from '@/i18n'

type Status = {
  nft_available: boolean
  kernel_bans: number | null
  open_connections: number
  tracked_addresses: number
  sysctl: { applied: boolean; syncookies: boolean; somaxconn: number }
  your_ip: string
}
type Ban = { ip: string; source: string; reason: string; created_at: number; expires_at: number }
type DefenseEvent = { at: number; ip: string; source: string; action: string; detail: string }
type NumberKey = { [K in keyof DefenseConfig]: DefenseConfig[K] extends number ? K : never }[keyof DefenseConfig]
type BoolKey = { [K in keyof DefenseConfig]: DefenseConfig[K] extends boolean ? K : never }[keyof DefenseConfig]

const { t } = useI18n()
const emit = defineEmits<{ (e: 'loaded', bans: number): void }>()

const data = ref<{ config: DefenseConfig; status: Status } | null>(null)
const status = computed(() => data.value?.status)
const form = reactive({} as DefenseConfig)
const bans = ref<Ban[]>([])
const events = ref<DefenseEvent[]>([])
const saving = ref(false)
const installing = ref(false)
const hardening = ref(false)

const ruleGroups = computed(() => {
  const field = (key: NumberKey, label: string, min: number, max: number) => ({ key, label, min, max })
  return [
    {
      key: 'web', enabledKey: 'web_enabled' as BoolKey, title: t('def.webTitle'), desc: t('def.webDesc'),
      fields: [
        field('web_window', t('def.window'), 10, 3600),
        field('web_max_requests', t('def.maxRequests'), 10, 1000000),
        field('web_max_errors', t('def.maxErrors'), 5, 1000000),
      ],
    },
    {
      key: 'cc', enabledKey: 'cc_enabled' as BoolKey, title: t('def.ccTitle'), desc: t('def.ccDesc'),
      fields: [
        field('cc_rate', t('def.ccRate'), 1, 10000),
        field('cc_burst', t('def.ccBurst'), 0, 100000),
        field('cc_conn', t('def.ccConn'), 1, 100000),
      ],
    },
    {
      key: 'ssh', enabledKey: 'ssh_enabled' as BoolKey, title: t('def.sshTitle'), desc: t('def.sshDesc'),
      fields: [
        field('ssh_window', t('def.window'), 30, 86400),
        field('ssh_max_failures', t('def.maxFailures'), 2, 1000),
      ],
    },
    {
      key: 'panel', enabledKey: 'panel_enabled' as BoolKey, title: t('def.panelTitle'), desc: t('def.panelDesc'),
      fields: [
        field('panel_window', t('def.window'), 30, 86400),
        field('panel_max_denied', t('def.maxDenied'), 3, 10000),
      ],
    },
  ]
})

async function load() {
  try {
    const [overview, banList, eventList]: any[] = await Promise.all([
      getDefense(), getDefenseBans(), getDefenseEvents(200),
    ])
    if (overview.success) {
      data.value = overview.data
      Object.assign(form, overview.data.config)
    }
    if (banList.success) bans.value = banList.data
    if (eventList.success) events.value = eventList.data
    emit('loaded', bans.value.length)
  } catch {
    // Interceptor reports it.
  }
}

async function save() {
  saving.value = true
  try {
    const res: any = await saveDefense({ ...form })
    if (res.success) {
      ElMessage.success(t('def.saved'))
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

async function toggleEnabled(enabled: boolean) {
  form.enabled = enabled
  await save()
}

function addMyIp() {
  const ip = status.value?.your_ip
  if (!ip) return
  const lines = form.whitelist.split(/[\s,]+/).filter(Boolean)
  if (!lines.includes(ip)) lines.push(ip)
  form.whitelist = lines.join('\n')
}

async function installNft() {
  installing.value = true
  try {
    const res: any = await installNftables()
    if (res.success) {
      ElMessage.success(t('def.nftReady'))
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    installing.value = false
  }
}

async function applyHardening() {
  hardening.value = true
  try {
    const res: any = await setKernelHardening(true)
    if (res.success) {
      ElMessage.success(t('def.hardeningApplied'))
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    hardening.value = false
  }
}

const banVisible = ref(false)
const banning = ref(false)
const banForm = reactive({ ip: '', minutes: 60, reason: '' })

async function banIp() {
  if (!banForm.ip.trim()) return
  banning.value = true
  try {
    const res: any = await banDefenseIp({ ...banForm, ip: banForm.ip.trim() })
    if (res.success) {
      ElMessage.success(t('def.bannedOk'))
      banVisible.value = false
      banForm.ip = ''
      banForm.reason = ''
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    banning.value = false
  }
}

async function unban(ip: string) {
  try {
    await ElMessageBox.confirm(t('def.confirmUnban', { ip }), t('sec.confirmTitle'), { type: 'warning' })
  } catch {
    return
  }
  try {
    const res: any = await unbanDefenseIp(ip)
    if (res.success) {
      ElMessage.success(t('def.unbannedOk'))
      await load()
    }
  } catch {
    // Interceptor reports it.
  }
}

function formatTime(unix: number) {
  return unix ? new Date(unix * 1000).toLocaleString() : '-'
}

const now = ref(Date.now())
function remaining(expiresAt: number) {
  if (!expiresAt) return t('def.permanent')
  const seconds = Math.max(0, expiresAt - Math.floor(now.value / 1000))
  if (seconds >= 3600) return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`
  return `${Math.ceil(seconds / 60)}m`
}

function sourceLabel(source: string) {
  const labels: Record<string, string> = {
    web: t('def.srcWeb'), ssh: 'SSH', panel: t('def.srcPanel'), manual: t('def.srcManual'),
  }
  return labels[source] || source
}

function actionLabel(action: string) {
  const labels: Record<string, string> = {
    ban: t('def.actBan'), unban: t('def.actUnban'), skip: t('def.actSkip'), error: t('def.actError'),
  }
  return labels[action] || action
}

function actionClass(action: string) {
  return { ban: 'lp-pill--danger', unban: 'lp-pill--ok', skip: 'lp-pill--warn', error: 'lp-pill--warn' }[action] || ''
}

// Bans arrive on their own; keep the view current while it is open.
let timer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  load()
  timer = setInterval(() => {
    now.value = Date.now()
    if (!saving.value && !banVisible.value) load()
  }, 15000)
})
onBeforeUnmount(() => clearInterval(timer))

defineExpose({ load })
</script>

<style scoped lang="scss">
.stack { display: flex; flex-direction: column; gap: var(--lp-4); }

.tiles {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: var(--lp-4);
}
.tile__label { font-size: var(--lp-text-xs); color: var(--lp-text-dim); margin-bottom: var(--lp-1); }
.tile__value {
  display: flex;
  align-items: center;
  gap: var(--lp-2);
  min-height: 28px;
  font-size: var(--lp-text-lg);
  font-weight: 600;
}

.rules {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--lp-4);
}
@media (max-width: 900px) {
  .rules { grid-template-columns: minmax(0, 1fr); }
}
.rule {
  border: 1px solid var(--lp-line-soft);
  border-radius: var(--lp-r-lg);
  padding: var(--lp-4);
}
.rule__head { display: flex; align-items: center; gap: var(--lp-2); }
.rule__title { font-weight: 600; }
.rule__desc { margin: var(--lp-2) 0 var(--lp-3); font-size: var(--lp-text-xs); color: var(--lp-text-dim); line-height: 1.5; }
.rule__fields { display: flex; flex-wrap: wrap; gap: var(--lp-3); }

.field { display: flex; flex-direction: column; gap: var(--lp-1); }
.field__label { font-size: var(--lp-text-xs); color: var(--lp-text-soft); }
.field--wide { flex: 1 1 320px; }

.general {
  display: flex;
  flex-wrap: wrap;
  gap: var(--lp-5);
  margin-top: var(--lp-5);
  padding-top: var(--lp-4);
  border-top: 1px solid var(--lp-line-soft);
}
.hint { font-size: var(--lp-text-xs); line-height: 1.5; }
</style>
