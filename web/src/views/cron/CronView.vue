<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.cron') }}</h1>
        <p class="lp-page-sub">{{ t('cron.subtitle', { total: rows.length }) }}</p>
      </div>
      <div class="lp-page-actions">
        <el-button size="small" :icon="Refresh" :loading="loading" @click="load">{{ t('refresh') }}</el-button>
        <el-button type="primary" size="small" :icon="Plus" :disabled="!available" @click="openCreate">
          {{ t('cron.newJob') }}
        </el-button>
      </div>
    </div>

    <el-alert
      v-if="backendMessage"
      :title="backendMessage"
      type="warning"
      :closable="false"
      show-icon
      class="notice"
    />

    <div class="lp-card">
      <el-table :data="rows" v-loading="loading" size="small" :empty-text="t('cron.empty')">
        <el-table-column width="56">
          <template #default="{ row }">
            <el-switch
              :model-value="row.enabled"
              size="small"
              :loading="busy === row.id"
              @change="(value: string | number | boolean) => toggle(row, value === true)"
            />
          </template>
        </el-table-column>
        <el-table-column :label="t('cron.name')" min-width="150">
          <template #default="{ row }">
            <span v-if="row.name" class="job-name">{{ row.name }}</span>
            <span v-else class="lp-dim">{{ t('cron.unnamed') }}</span>
          </template>
        </el-table-column>
        <el-table-column :label="t('cron.schedule')" width="160">
          <template #default="{ row }">
            <div class="cell-stack">
              <code class="schedule">{{ row.schedule }}</code>
              <span class="lp-dim">{{ describe(row.schedule) }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column :label="t('cron.command')" min-width="260">
          <template #default="{ row }">
            <code class="lp-mono lp-truncate command">{{ row.command }}</code>
          </template>
        </el-table-column>
        <el-table-column :label="t('proc.actions')" width="140" align="right">
          <template #default="{ row }">
            <el-button link size="small" :disabled="!available" @click="openEdit(row)">{{ t('cron.edit') }}</el-button>
            <el-button link type="danger" size="small" :disabled="!available" @click="remove(row)">
              {{ t('delete') }}
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <el-dialog v-model="formVisible" :title="editing ? t('cron.editJob') : t('cron.newJob')" width="560px">
      <el-form ref="formRef" :model="form" :rules="rules" label-position="top">
        <el-form-item v-if="!editing" :label="t('cron.type')">
          <el-radio-group v-model="jobType" @change="applyTemplate">
            <el-radio-button value="custom">{{ t('cron.typeCustom') }}</el-radio-button>
            <el-radio-button value="site">{{ t('cron.typeSite') }}</el-radio-button>
            <el-radio-button value="db">{{ t('cron.typeDb') }}</el-radio-button>
          </el-radio-group>
        </el-form-item>

        <template v-if="!editing && jobType === 'site'">
          <el-form-item :label="t('cron.backupSite')">
            <el-select v-model="template.siteId" filterable style="width: 100%" @change="applyTemplate">
              <el-option v-for="s in sites" :key="s.id" :label="s.name" :value="s.id" />
            </el-select>
          </el-form-item>
        </template>
        <template v-if="!editing && jobType === 'db'">
          <div class="form-row">
            <el-form-item :label="t('cron.backupEngine')" class="form-row__narrow">
              <el-select v-model="template.engine" @change="loadTemplateDbs">
                <el-option label="MySQL" value="mysql" />
                <el-option label="PostgreSQL" value="postgresql" />
              </el-select>
            </el-form-item>
            <el-form-item :label="t('cron.backupDb')" class="form-row__wide">
              <el-select v-model="template.dbName" filterable style="width: 100%" @change="applyTemplate">
                <el-option v-for="d in templateDbs" :key="d" :label="d" :value="d" />
              </el-select>
            </el-form-item>
          </div>
        </template>
        <el-form-item v-if="!editing && jobType !== 'custom'" :label="t('cron.keep')">
          <el-input-number v-model="template.keep" :min="1" :max="365" @change="applyTemplate" />
          <span class="hint__text keep-hint">{{ t('cron.keepHint') }}</span>
        </el-form-item>

        <el-form-item :label="t('cron.name')" prop="name">
          <el-input v-model="form.name" :placeholder="t('cron.namePlaceholder')" maxlength="64" />
        </el-form-item>

        <el-form-item :label="t('cron.schedule')" prop="schedule">
          <el-input v-model="form.schedule" placeholder="*/5 * * * *" class="lp-mono" />
          <div class="hint">
            <span class="hint__text">{{ describe(form.schedule) }}</span>
            <div class="preset-row">
              <button
                v-for="preset in presets"
                :key="preset.value"
                type="button"
                class="preset"
                @click="form.schedule = preset.value"
              >{{ preset.label }}</button>
            </div>
          </div>
        </el-form-item>

        <el-form-item :label="t('cron.command')" prop="command">
          <el-input
            v-model="form.command"
            type="textarea"
            :rows="3"
            placeholder="/usr/bin/backup.sh"
            class="lp-mono"
            :readonly="!editing && jobType !== 'custom'"
          />
        </el-form-item>

        <el-form-item>
          <el-switch v-model="form.enabled" :active-text="t('cron.enabled')" />
        </el-form-item>
      </el-form>

      <template #footer>
        <el-button @click="formVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="submit">{{ t('save') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { usePage } from '@/utils/page'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { FormRules } from 'element-plus'
import { Plus, Refresh } from '@element-plus/icons-vue'
import { useRoute } from 'vue-router'
import {
  createCronJob, deleteCronJob, getBackupCronPrefix, getCronBackend, getCronJobs, getSites,
  listDatabases, toggleCronJob, updateCronJob,
} from '@/api'
import { useI18n } from '@/i18n'

type CronJob = {
  id: number
  name: string
  schedule: string
  command: string
  enabled: boolean
  raw: string
}

const { t } = useI18n()

const rows = ref<CronJob[]>([])
const loading = ref(false)
const available = ref(false)
const backendMessage = ref('')
const busy = ref<number | null>(null)

const presets = computed(() => [
  { label: t('cron.everyMinute'), value: '* * * * *' },
  { label: t('cron.every5Minutes'), value: '*/5 * * * *' },
  { label: t('cron.hourly'), value: '0 * * * *' },
  { label: t('cron.daily'), value: '0 3 * * *' },
  { label: t('cron.weekly'), value: '0 3 * * 0' },
  { label: t('cron.onReboot'), value: '@reboot' },
])

async function load() {
  loading.value = true
  try {
    const backendRes: any = await getCronBackend()
    if (backendRes.success) {
      available.value = !!backendRes.data?.available
      backendMessage.value = backendRes.data?.available ? '' : backendRes.data?.message || ''
    }
    if (!available.value) {
      rows.value = []
      return
    }
    const res: any = await getCronJobs()
    rows.value = res.success ? res.data || [] : []
  } catch {
    rows.value = []
  } finally {
    loading.value = false
  }
}

// --- Form ---
const formVisible = ref(false)
const saving = ref(false)
const editing = ref<CronJob | null>(null)
const formRef = ref()
const form = reactive({ name: '', schedule: '*/5 * * * *', command: '', enabled: true })

const rules = computed<FormRules>(() => ({
  schedule: [
    { required: true, message: t('cron.scheduleRequired'), trigger: 'blur' },
    {
      validator: (_rule: any, value: string, callback: (error?: Error) => void) => {
        callback(isValidSchedule(value) ? undefined : new Error(t('cron.scheduleInvalid')))
      },
      trigger: 'blur',
    },
  ],
  command: [{ required: true, message: t('cron.commandRequired'), trigger: 'blur' }],
}))

// --- Backup job templates: the command runs `linxpanel backup ...` ---
const jobType = ref<'custom' | 'site' | 'db'>('custom')
const template = reactive({ siteId: 0, engine: 'mysql', dbName: '', keep: 7 })
const sites = ref<{ id: number; name: string }[]>([])
const templateDbs = ref<string[]>([])
const cronPrefix = ref('')

async function loadTemplateDbs() {
  template.dbName = ''
  const res: any = await listDatabases(template.engine).catch(() => null)
  templateDbs.value = res?.success ? res.data.map((d: any) => d.name) : []
  applyTemplate()
}

async function applyTemplate() {
  if (jobType.value === 'custom') {
    form.command = ''
    return
  }
  if (!cronPrefix.value) {
    const res: any = await getBackupCronPrefix().catch(() => null)
    cronPrefix.value = res?.data?.prefix || 'linxpanel'
  }
  if (!sites.value.length) {
    const res: any = await getSites().catch(() => null)
    sites.value = res?.success ? res.data : []
  }
  if (jobType.value === 'db' && !templateDbs.value.length && !template.dbName) {
    const res: any = await listDatabases(template.engine).catch(() => null)
    templateDbs.value = res?.success ? res.data.map((d: any) => d.name) : []
  }
  form.schedule = form.schedule === '*/5 * * * *' ? '30 2 * * *' : form.schedule
  if (jobType.value === 'site') {
    const site = sites.value.find((s) => s.id === template.siteId)
    form.command = site ? `${cronPrefix.value} backup site ${site.id} --keep ${template.keep}` : ''
    form.name = site ? t('cron.backupSiteName', { name: site.name }) : ''
  } else {
    form.command = template.dbName
      ? `${cronPrefix.value} backup db ${template.engine} ${template.dbName} --keep ${template.keep}`
      : ''
    form.name = template.dbName ? t('cron.backupDbName', { name: template.dbName }) : ''
  }
}

function openCreate() {
  editing.value = null
  jobType.value = 'custom'
  Object.assign(template, { siteId: 0, engine: 'mysql', dbName: '', keep: 7 })
  form.name = ''
  form.schedule = '*/5 * * * *'
  form.command = ''
  form.enabled = true
  formVisible.value = true
}

function openEdit(row: CronJob) {
  editing.value = row
  form.name = row.name
  form.schedule = row.schedule
  form.command = row.command
  form.enabled = row.enabled
  formVisible.value = true
}

async function submit() {
  try {
    await formRef.value?.validate()
  } catch {
    return
  }
  saving.value = true
  try {
    const payload = { ...form }
    const res: any = editing.value
      ? await updateCronJob(editing.value.id, payload)
      : await createCronJob(payload)
    if (res.success) {
      ElMessage.success(t('cron.saved'))
      formVisible.value = false
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

async function toggle(row: CronJob, enabled: boolean) {
  busy.value = row.id
  try {
    const res: any = await toggleCronJob(row.id, enabled)
    if (res.success) await load()
  } catch {
    await load()
  } finally {
    busy.value = null
  }
}

async function remove(row: CronJob) {
  try {
    await ElMessageBox.confirm(
      t('cron.confirmDelete', { command: row.command }),
      t('cron.confirmDeleteTitle'),
      { type: 'warning', confirmButtonText: t('delete'), cancelButtonText: t('cancel') },
    )
  } catch {
    return
  }
  try {
    const res: any = await deleteCronJob(row.id)
    if (res.success) {
      ElMessage.success(t('cron.deleted'))
      await load()
    }
  } catch {
    // Interceptor reports it.
  }
}

const CRON_FIELD = /^[\d*/,-]+$/
const SPECIALS = ['@reboot', '@yearly', '@annually', '@monthly', '@weekly', '@daily', '@midnight', '@hourly']

function isValidSchedule(value: string): boolean {
  const text = (value || '').trim()
  if (!text) return false
  if (text.startsWith('@')) return SPECIALS.includes(text)
  const fields = text.split(/\s+/)
  return fields.length === 5 && fields.every((field) => CRON_FIELD.test(field))
}

/** Plain-language gloss for the common shapes; falls back to the raw expression. */
function describe(schedule: string): string {
  const text = (schedule || '').trim()
  if (!text) return ''
  if (text.startsWith('@')) {
    const key = text.slice(1)
    return t(`cron.special.${key}`) !== `cron.special.${key}` ? t(`cron.special.${key}`) : text
  }
  const fields = text.split(/\s+/)
  if (fields.length !== 5) return t('cron.scheduleInvalid')
  const [minute, hour, dayOfMonth, month, dayOfWeek] = fields
  const everyField = (value: string) => value === '*'

  if (minute === '*' && everyField(hour)) return t('cron.descEveryMinute')
  if (minute.startsWith('*/') && everyField(hour)) {
    return t('cron.descEveryNMinutes', { n: minute.slice(2) })
  }
  if (/^\d+$/.test(minute) && everyField(hour)) return t('cron.descHourlyAt', { m: minute })
  if (/^\d+$/.test(minute) && /^\d+$/.test(hour) && everyField(dayOfMonth) && everyField(month)) {
    const time = `${hour.padStart(2, '0')}:${minute.padStart(2, '0')}`
    return everyField(dayOfWeek)
      ? t('cron.descDailyAt', { time })
      : t('cron.descWeeklyAt', { time, day: dayOfWeek })
  }
  return text
}

const route = useRoute()
async function openFromRoute() {
  if (route.query.template === 'site' && route.query.site) {
    openCreate()
    jobType.value = 'site'
    template.siteId = Number(route.query.site)
    await applyTemplate()
  }
}
onMounted(async () => {
  await load()
  await openFromRoute()
})
usePage(async () => {
  await load()
  await openFromRoute()
})
</script>

<style scoped lang="scss">
.notice { margin-bottom: var(--lp-4); border-radius: var(--lp-r); }

.cell-stack { display: flex; flex-direction: column; line-height: 1.35; min-width: 0; }
.cell-stack .lp-dim { font-size: 11px; }

.job-name { font-weight: 500; }
.schedule {
  font-family: var(--lp-font-mono);
  font-size: var(--lp-text-sm);
  color: var(--lp-accent);
}
.command {
  display: block;
  font-size: var(--lp-text-sm);
  color: var(--lp-text-soft);
}

.hint {
  width: 100%;
  margin-top: 6px;
}
.hint__text {
  display: block;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  margin-bottom: 6px;
}
.preset-row { display: flex; flex-wrap: wrap; gap: 6px; }
.preset {
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

.form-row { display: flex; gap: var(--lp-3); }
.form-row__narrow { width: 160px; flex: none; }
.form-row__wide { flex: 1; min-width: 0; }
.keep-hint { margin-left: var(--lp-2); }
</style>
