<template>
  <div class="an">
    <!-- Channels -->
    <section class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Promotion /></el-icon>{{ t('an.channels') }}</h2>
        <el-button type="primary" size="small" :icon="Plus" @click="openChannel()">{{ t('an.addChannel') }}</el-button>
      </header>
      <div class="lp-card-body">
        <p class="section-note">{{ t('an.channelsHint') }}</p>
        <div v-if="config.channels.length" class="an-channels">
          <div v-for="(c, i) in config.channels" :key="c.id || i" class="an-channel">
            <span class="an-badge" :data-kind="c.kind">{{ KIND_SHORT[c.kind] || c.kind }}</span>
            <div class="an-channel__main">
              <span class="strong">{{ c.name || t(`an.kind.${c.kind}`) }}</span>
              <span class="lp-dim an-channel__sub lp-mono lp-truncate">{{ channelSummary(c) }}</span>
            </div>
            <el-switch v-model="c.enabled" size="small" @change="persist()" />
            <el-button link type="primary" size="small" :loading="testing === (c.id || String(i))" @click="sendTest(c, c.id || String(i))">{{ t('an.test') }}</el-button>
            <el-button link type="primary" size="small" @click="openChannel(i)">{{ t('cron.edit') }}</el-button>
            <el-button link type="danger" size="small" @click="removeChannel(i)">{{ t('delete') }}</el-button>
          </div>
        </div>
        <el-empty v-else :image-size="56" :description="t('an.noChannels')" />
      </div>
    </section>

    <!-- Events -->
    <section class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Bell /></el-icon>{{ t('an.events') }}</h2>
      </header>
      <div class="lp-card-body">
        <div class="an-events">
          <el-checkbox v-model="config.events.resource" @change="persist()">{{ t('an.ev.resource') }}</el-checkbox>
          <el-checkbox v-model="config.events.app_crash" @change="persist()">{{ t('an.ev.appCrash') }}</el-checkbox>
          <span class="an-inline">
            <el-checkbox v-model="config.events.ssl_expiry" @change="persist()">{{ t('an.ev.ssl') }}</el-checkbox>
            <el-input-number v-model="config.events.ssl_days" :min="1" :max="90" size="small" controls-position="right" class="an-num" @change="persist()" />
            {{ t('an.days') }}
          </span>
          <el-checkbox v-model="config.events.backup_failed" @change="persist()">{{ t('an.ev.backup') }}</el-checkbox>
          <el-checkbox v-model="config.events.login_failed" @change="persist()">{{ t('an.ev.loginFailed') }}</el-checkbox>
          <el-checkbox v-model="config.events.login" @change="persist()">{{ t('an.ev.login') }}</el-checkbox>
        </div>
        <div class="an-inline an-cooldown">
          {{ t('an.cooldown') }}
          <el-input-number v-model="config.cooldown_minutes" :min="1" :max="1440" size="small" controls-position="right" class="an-num" @change="persist()" />
          {{ t('an.minutes') }}
        </div>
        <p class="section-note">{{ t('an.resourceNote') }}</p>
      </div>
    </section>

    <!-- History -->
    <section class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Clock /></el-icon>{{ t('an.history') }}</h2>
        <el-button size="small" :icon="Refresh" @click="loadHistory">{{ t('refresh') }}</el-button>
      </header>
      <el-table :data="history" size="small" max-height="360" :empty-text="t('an.noHistory')">
        <el-table-column :label="t('an.time')" width="150">
          <template #default="{ row }"><span :title="formatFullTime(epoch(row.created_at))">{{ formatShortTime(epoch(row.created_at)) }}</span></template>
        </el-table-column>
        <el-table-column :label="t('an.alert')" min-width="260">
          <template #default="{ row }">
            <div class="strong">{{ row.title }}</div>
            <div class="lp-dim an-body">{{ row.body }}</div>
          </template>
        </el-table-column>
        <el-table-column :label="t('an.result')" min-width="200">
          <template #default="{ row }"><span class="an-result" :class="{ 'is-bad': /:\s(?!ok)/.test(row.result) }">{{ row.result }}</span></template>
        </el-table-column>
      </el-table>
    </section>

    <!-- Channel dialog -->
    <el-dialog v-model="dialog" :title="editIndex === null ? t('an.addChannel') : t('an.editChannel')" width="560px" append-to-body>
      <el-form label-position="top" @submit.prevent>
        <el-form-item :label="t('an.type')">
          <el-radio-group v-model="draft.kind" class="an-kinds">
            <el-radio-button v-for="k in KINDS" :key="k" :value="k">{{ t(`an.kind.${k}`) }}</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item :label="t('an.name')">
          <el-input v-model="draft.name" :placeholder="t(`an.kind.${draft.kind}`)" maxlength="40" />
        </el-form-item>
        <template v-if="['dingtalk', 'wecom', 'feishu', 'webhook'].includes(draft.kind)">
          <el-form-item :label="t('an.url')">
            <el-input v-model="draft.url" class="lp-mono" :placeholder="URL_HINT[draft.kind]" />
            <span class="hint">{{ t(`an.urlHint.${draft.kind}`) }}</span>
          </el-form-item>
          <el-form-item v-if="draft.kind === 'dingtalk' || draft.kind === 'feishu'" :label="t('an.signSecret')">
            <el-input v-model="draft.secret" class="lp-mono" :placeholder="t('an.optional')" show-password />
            <span class="hint">{{ t('an.signHint') }}</span>
          </el-form-item>
        </template>
        <template v-if="draft.kind === 'telegram'">
          <el-form-item label="Bot Token">
            <el-input v-model="draft.secret" class="lp-mono" placeholder="123456:ABC-DEF..." show-password />
          </el-form-item>
          <el-form-item label="Chat ID">
            <el-input v-model="draft.chat_id" class="lp-mono" placeholder="123456789" />
          </el-form-item>
          <el-form-item :label="t('an.tgApi')">
            <el-input v-model="draft.url" class="lp-mono" placeholder="https://api.telegram.org" />
            <span class="hint">{{ t('an.tgApiHint') }}</span>
          </el-form-item>
        </template>
        <template v-if="draft.kind === 'email'">
          <div class="an-row">
            <el-form-item :label="t('an.smtpHost')" class="an-grow">
              <el-input v-model="draft.smtp_host" class="lp-mono" placeholder="smtp.qq.com" />
            </el-form-item>
            <el-form-item :label="t('an.port')">
              <el-input-number v-model="draft.smtp_port" :min="1" :max="65535" controls-position="right" class="an-port" />
            </el-form-item>
          </div>
          <el-form-item :label="t('an.security')">
            <el-radio-group v-model="draft.security" @change="suggestPort">
              <el-radio value="ssl">SSL/TLS</el-radio>
              <el-radio value="starttls">STARTTLS</el-radio>
              <el-radio value="none">{{ t('an.noEncryption') }}</el-radio>
            </el-radio-group>
          </el-form-item>
          <div class="an-row">
            <el-form-item :label="t('an.username')" class="an-grow">
              <el-input v-model="draft.username" class="lp-mono" autocomplete="off" />
            </el-form-item>
            <el-form-item :label="t('an.password')" class="an-grow">
              <el-input v-model="draft.password" type="password" show-password autocomplete="new-password" :placeholder="t('an.authCode')" />
            </el-form-item>
          </div>
          <el-form-item :label="t('an.from')">
            <el-input v-model="draft.from" class="lp-mono" placeholder="alerts@example.com" />
          </el-form-item>
          <el-form-item :label="t('an.to')">
            <el-input v-model="draft.to" class="lp-mono" placeholder="ops@example.com, me@example.com" />
          </el-form-item>
        </template>
      </el-form>
      <template #footer>
        <el-button :loading="testing === 'draft'" @click="sendTest(draft, 'draft')">{{ t('an.test') }}</el-button>
        <el-button @click="dialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="saveChannel">{{ t('save') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Bell, Clock, Plus, Promotion, Refresh } from '@element-plus/icons-vue'
import { getAlertHistory, getNotify, saveNotify, testNotify } from '@/api'
import { useI18n } from '@/i18n'
import { formatFullTime, formatShortTime } from '@/utils/format'

type Channel = {
  id: string; name: string; kind: string; enabled: boolean; url: string; secret: string; chat_id: string
  smtp_host: string; smtp_port: number; security: string; username: string; password: string; from: string; to: string
}
const KINDS = ['dingtalk', 'wecom', 'feishu', 'telegram', 'email', 'webhook']
const KIND_SHORT: Record<string, string> = { dingtalk: '钉', wecom: '企', feishu: '飞', telegram: 'TG', email: '@', webhook: '{ }' }
const URL_HINT: Record<string, string> = {
  dingtalk: 'https://oapi.dingtalk.com/robot/send?access_token=...',
  wecom: 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=...',
  feishu: 'https://open.feishu.cn/open-apis/bot/v2/hook/...',
  webhook: 'https://example.com/hooks/linxpanel',
}

const { t } = useI18n()
const config = reactive({
  channels: [] as Channel[],
  events: { resource: true, app_crash: true, ssl_expiry: true, ssl_days: 7, backup_failed: true, login: false, login_failed: true },
  cooldown_minutes: 30,
})
const history = ref<any[]>([])
const dialog = ref(false)
const editIndex = ref<number | null>(null)
const saving = ref(false)
const testing = ref('')
const blank = (): Channel => ({
  id: '', name: '', kind: 'dingtalk', enabled: true, url: '', secret: '', chat_id: '',
  smtp_host: '', smtp_port: 465, security: 'ssl', username: '', password: '', from: '', to: '',
})
const draft = reactive<Channel>(blank())

function epoch(text: string) {
  const ms = Date.parse(`${text.replace(' ', 'T')}Z`)
  return Number.isNaN(ms) ? 0 : Math.floor(ms / 1000)
}
function channelSummary(c: Channel) {
  if (c.kind === 'email') return `${c.smtp_host}:${c.smtp_port} → ${c.to}`
  if (c.kind === 'telegram') return `chat ${c.chat_id}`
  return c.url
}

async function load() {
  const res: any = await getNotify().catch(() => null)
  if (res?.success) Object.assign(config, res.data)
  await loadHistory()
}
async function loadHistory() {
  const res: any = await getAlertHistory().catch(() => null)
  if (res?.success) history.value = res.data
}

async function persist(): Promise<boolean> {
  saving.value = true
  try {
    const res: any = await saveNotify(JSON.parse(JSON.stringify(config)))
    if (res.success) {
      // Fresh ids and masks for what was just saved.
      const fresh: any = await getNotify().catch(() => null)
      if (fresh?.success) Object.assign(config, fresh.data)
      ElMessage.success(t('an.saved'))
      return true
    }
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
  return false
}

function openChannel(index: number | null = null) {
  editIndex.value = index
  Object.assign(draft, blank(), index === null ? {} : JSON.parse(JSON.stringify(config.channels[index])))
  dialog.value = true
}
function suggestPort(security: string | number | boolean | undefined) {
  if (draft.smtp_port === 465 || draft.smtp_port === 587 || draft.smtp_port === 25) {
    draft.smtp_port = security === 'ssl' ? 465 : security === 'starttls' ? 587 : 25
  }
}
async function saveChannel() {
  const copy = JSON.parse(JSON.stringify(draft)) as Channel
  const before = JSON.parse(JSON.stringify(config.channels))
  if (editIndex.value === null) config.channels.push(copy)
  else config.channels.splice(editIndex.value, 1, copy)
  if (await persist()) dialog.value = false
  else config.channels.splice(0, config.channels.length, ...before)
}
async function removeChannel(index: number) {
  try {
    await ElMessageBox.confirm(t('an.removeConfirm', { name: config.channels[index].name || config.channels[index].kind }), t('delete'), { type: 'warning' })
  } catch {
    return
  }
  config.channels.splice(index, 1)
  await persist()
}
async function sendTest(channel: Channel, key: string) {
  testing.value = key
  try {
    const res: any = await testNotify(JSON.parse(JSON.stringify(channel)))
    if (res.success) ElMessage.success(t('an.testSent'))
  } catch {
    // Interceptor reports it.
  } finally {
    testing.value = ''
  }
}

onMounted(load)
</script>

<style scoped lang="scss">
.an { display: flex; flex-direction: column; gap: var(--lp-4); }
.an-channels { display: flex; flex-direction: column; gap: 8px; }
.an-channel {
  display: flex; align-items: center; gap: 10px; padding: 10px 12px;
  border: 1px solid var(--lp-line); border-radius: var(--lp-r-md);
}
.an-channel__main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.an-channel__sub { font-size: 12px; }
.an-badge {
  display: grid; place-items: center; width: 34px; height: 34px; border-radius: var(--lp-r-sm);
  font-weight: 700; font-size: 12px; color: #fff; background: #64748b; flex-shrink: 0;
  &[data-kind='dingtalk'] { background: #1677ff; }
  &[data-kind='wecom'] { background: #07c160; }
  &[data-kind='feishu'] { background: #3370ff; }
  &[data-kind='telegram'] { background: #229ed9; }
  &[data-kind='email'] { background: #f59e0b; }
}
.an-events { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px 16px; }
.an-inline { display: inline-flex; align-items: center; gap: 8px; font-size: 14px; }
.an-num { width: 100px; }
.an-cooldown { margin-top: 14px; color: var(--lp-text-dim); }
.an-body { font-size: 12px; white-space: pre-wrap; }
.an-result { font-size: 12px; color: var(--lp-ok); word-break: break-all; &.is-bad { color: var(--lp-warn); } }
.an-kinds { flex-wrap: wrap; }
.an-row { display: flex; gap: 12px; flex-wrap: wrap; }
.an-grow { flex: 1; min-width: 180px; }
.an-port { width: 130px; }
.strong { font-weight: 600; }
.hint { display: block; flex-basis: 100%; margin-top: 4px; font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
.section-note { margin: 0 0 12px; font-size: 13px; color: var(--lp-text-dim); }
</style>
