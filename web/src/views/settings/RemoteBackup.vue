<template>
  <div class="rb-stack">
    <section class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><UploadFilled /></el-icon>{{ t('rs.title') }}</h2>
        <el-switch v-model="form.enabled" :active-text="t('rs.enabled')" />
      </header>
      <div class="lp-card-body">
        <p class="section-note">{{ t('rs.hint') }}</p>
        <el-form label-position="top" @submit.prevent>
          <el-form-item :label="t('rs.type')">
            <el-radio-group v-model="form.kind">
              <el-radio-button value="s3">{{ t('rs.s3') }}</el-radio-button>
              <el-radio-button value="webdav">WebDAV</el-radio-button>
              <el-radio-button value="ftp">FTP / FTPS</el-radio-button>
            </el-radio-group>
          </el-form-item>

          <el-form-item :label="t('rs.endpoint')">
            <el-input v-model="form.endpoint" class="lp-mono" :placeholder="ENDPOINTS[form.kind]" />
            <span class="hint">{{ t(`rs.endpointHint.${form.kind}`) }}</span>
          </el-form-item>

          <template v-if="form.kind === 's3'">
            <div class="rs-row">
              <el-form-item :label="t('rs.bucket')" class="rs-grow">
                <el-input v-model="form.bucket" class="lp-mono" placeholder="my-backups" />
              </el-form-item>
              <el-form-item :label="t('rs.region')" class="rs-grow">
                <el-input v-model="form.region" class="lp-mono" placeholder="us-east-1 / oss-cn-hangzhou / ap-guangzhou" />
              </el-form-item>
            </div>
            <div class="rs-row">
              <el-form-item label="Access Key" class="rs-grow">
                <el-input v-model="form.access_key" class="lp-mono" autocomplete="off" />
              </el-form-item>
              <el-form-item label="Secret Key" class="rs-grow">
                <el-input v-model="form.secret_key" type="password" show-password class="lp-mono" autocomplete="new-password" />
              </el-form-item>
            </div>
            <el-form-item>
              <el-checkbox v-model="form.path_style">{{ t('rs.pathStyle') }}</el-checkbox>
              <span class="hint">{{ t('rs.pathStyleHint') }}</span>
            </el-form-item>
          </template>
          <div v-else class="rs-row">
            <el-form-item :label="t('rs.username')" class="rs-grow">
              <el-input v-model="form.username" class="lp-mono" autocomplete="off" />
            </el-form-item>
            <el-form-item :label="t('rs.password')" class="rs-grow">
              <el-input v-model="form.password" type="password" show-password autocomplete="new-password" />
            </el-form-item>
          </div>

          <div class="rs-row">
            <el-form-item :label="t('rs.prefix')" class="rs-grow">
              <el-input v-model="form.prefix" class="lp-mono" placeholder="linxpanel" />
              <span class="hint">{{ t('rs.prefixHint') }}</span>
            </el-form-item>
            <el-form-item :label="t('rs.keep')">
              <el-input-number v-model="form.keep" :min="0" :max="1000" controls-position="right" class="rs-keep" />
              <span class="hint">{{ t('rs.keepHint') }}</span>
            </el-form-item>
          </div>
        </el-form>
        <div class="rs-actions">
          <el-button :loading="testing" @click="test">{{ t('rs.test') }}</el-button>
          <el-button type="primary" :loading="saving" @click="save">{{ t('save') }}</el-button>
        </div>
      </div>
    </section>

    <section class="lp-card">
      <header class="lp-card-head">
        <h2 class="lp-card-title"><el-icon><Clock /></el-icon>{{ t('rs.history') }}</h2>
        <el-button size="small" :icon="Refresh" @click="loadHistory">{{ t('refresh') }}</el-button>
      </header>
      <el-table :data="history" size="small" max-height="360" :empty-text="t('rs.noHistory')">
        <el-table-column :label="t('rs.time')" width="150">
          <template #default="{ row }"><span :title="formatFullTime(epoch(row.uploaded_at))">{{ formatShortTime(epoch(row.uploaded_at)) }}</span></template>
        </el-table-column>
        <el-table-column :label="t('rs.object')" min-width="320">
          <template #default="{ row }"><span class="lp-mono rs-key">{{ row.object_key }}</span></template>
        </el-table-column>
        <el-table-column :label="t('fm.size')" width="100" align="right">
          <template #default="{ row }">{{ formatBytes(row.size) }}</template>
        </el-table-column>
        <el-table-column :label="t('rs.type')" width="90">
          <template #default="{ row }">{{ row.kind.toUpperCase() }}</template>
        </el-table-column>
      </el-table>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { Clock, Refresh, UploadFilled } from '@element-plus/icons-vue'
import { getRemoteBackup, getRemoteBackupHistory, saveRemoteBackup, testRemoteBackup } from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes, formatFullTime, formatShortTime } from '@/utils/format'

const { t } = useI18n()
const ENDPOINTS: Record<string, string> = {
  s3: 'https://oss-cn-hangzhou.aliyuncs.com',
  webdav: 'https://dav.jianguoyun.com/dav',
  ftp: 'ftp://backup.example.com',
}
const form = reactive({
  enabled: false, kind: 's3', endpoint: '', region: '', bucket: '', path_style: false,
  access_key: '', secret_key: '', username: '', password: '', prefix: 'linxpanel', keep: 7,
})
const history = ref<any[]>([])
const saving = ref(false)
const testing = ref(false)

function epoch(text: string) {
  const ms = Date.parse(`${text.replace(' ', 'T')}Z`)
  return Number.isNaN(ms) ? 0 : Math.floor(ms / 1000)
}

async function load() {
  const res: any = await getRemoteBackup().catch(() => null)
  if (res?.success) {
    Object.assign(form, res.data)
    if (!form.kind) form.kind = 's3'
  }
  await loadHistory()
}
async function loadHistory() {
  const res: any = await getRemoteBackupHistory().catch(() => null)
  if (res?.success) history.value = res.data
}
async function save() {
  saving.value = true
  try {
    const res: any = await saveRemoteBackup({ ...form })
    if (res.success) ElMessage.success(t('rs.saved'))
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}
async function test() {
  testing.value = true
  try {
    const res: any = await testRemoteBackup({ ...form, enabled: true })
    if (res.success) ElMessage.success(t('rs.testOk'))
  } catch {
    // Interceptor reports it.
  } finally {
    testing.value = false
  }
}
onMounted(load)
</script>

<style scoped lang="scss">
.rb-stack { display: flex; flex-direction: column; gap: var(--lp-4); }
.rs-row { display: flex; gap: 12px; flex-wrap: wrap; }
.rs-grow { flex: 1; min-width: 200px; }
.rs-keep { width: 140px; }
.rs-actions { display: flex; justify-content: flex-end; gap: 8px; }
.rs-key { font-size: 12px; word-break: break-all; }
.hint { display: block; flex-basis: 100%; margin-top: 4px; font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
.section-note { margin: 0 0 12px; font-size: 13px; color: var(--lp-text-dim); }
</style>
