<template>
  <div class="cert-panel">
    <div class="cert-toolbar">
      <div class="cert-toolbar__info">
        <span class="lp-pill" :class="acme?.available ? 'lp-pill--ok' : ''">
          <span class="lp-pill__dot"></span>
          {{ acme?.available ? `ACME: ${acme.client}` : t('ct.noAcme') }}
        </span>
        <span v-if="acme?.version" class="lp-dim acme-version lp-truncate">{{ acme.version }}</span>
      </div>
      <div class="cert-toolbar__actions">
        <el-button size="small" :icon="Refresh" :loading="loading" @click="load">{{ t('refresh') }}</el-button>
        <el-button size="small" @click="openSelfSigned">{{ t('ct.selfSigned') }}</el-button>
        <el-button size="small" @click="openUpload">{{ t('ct.upload') }}</el-button>
        <el-tooltip :disabled="!!acme?.available" :content="acme?.message" placement="top">
          <span>
            <el-button type="primary" size="small" :icon="Plus" :disabled="!acme?.available" @click="openAcme">
              {{ t('ct.requestLetsEncrypt') }}
            </el-button>
          </span>
        </el-tooltip>
      </div>
    </div>

    <div class="lp-card">
      <el-table :data="certs" v-loading="loading" size="small" :empty-text="t('ct.empty')">
        <el-table-column :label="t('ct.name')" min-width="150">
          <template #default="{ row }">
            <div class="cell-stack">
              <span class="cert-name">{{ row.name }}</span>
              <span class="lp-dim">{{ sourceLabel(row.source) }}</span>
            </div>
          </template>
        </el-table-column>

        <el-table-column :label="t('ct.domains')" min-width="200">
          <template #default="{ row }">
            <div class="domain-list">
              <span v-for="domain in domainsOf(row)" :key="domain" class="domain-chip">{{ domain }}</span>
            </div>
          </template>
        </el-table-column>

        <el-table-column :label="t('ct.issuer')" min-width="160">
          <template #default="{ row }">
            <span class="lp-dim lp-truncate">{{ shortIssuer(row.issuer) }}</span>
          </template>
        </el-table-column>

        <el-table-column :label="t('ct.expiry')" width="190">
          <template #default="{ row }">
            <div class="cell-stack">
              <span class="lp-pill" :class="statusClass(row.status)">
                <span class="lp-pill__dot"></span>{{ expiryLabel(row) }}
              </span>
              <span class="lp-dim lp-num">{{ row.not_after }}</span>
            </div>
          </template>
        </el-table-column>

        <el-table-column :label="t('ct.usedBy')" min-width="130">
          <template #default="{ row }">
            <span v-if="!row.used_by.length" class="lp-dim">{{ t('ct.unused') }}</span>
            <span v-else class="lp-truncate">{{ row.used_by.join(', ') }}</span>
          </template>
        </el-table-column>

        <el-table-column :label="t('proc.actions')" width="150" align="right">
          <template #default="{ row }">
            <el-button
              v-if="row.source === 'acme'"
              link
              size="small"
              :loading="busy === row.id"
              @click="renew(row)"
            >{{ t('ct.renew') }}</el-button>
            <el-button link type="danger" size="small" @click="remove(row)">{{ t('delete') }}</el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <!-- Upload -->
    <el-dialog v-model="uploadVisible" :title="t('ct.upload')" width="640px" top="6vh">
      <el-form ref="uploadRef" :model="uploadForm" :rules="uploadRules" label-position="top">
        <el-form-item :label="t('ct.name')" prop="name">
          <el-input v-model="uploadForm.name" :placeholder="t('ct.namePlaceholder')" maxlength="64" />
        </el-form-item>
        <el-form-item :label="t('ct.certPem')" prop="cert_pem">
          <el-input
            v-model="uploadForm.cert_pem"
            type="textarea"
            :rows="6"
            class="lp-mono pem-input"
            placeholder="-----BEGIN CERTIFICATE-----"
          />
          <span class="hint">{{ t('ct.certPemHint') }}</span>
        </el-form-item>
        <el-form-item :label="t('ct.keyPem')" prop="key_pem">
          <el-input
            v-model="uploadForm.key_pem"
            type="textarea"
            :rows="6"
            class="lp-mono pem-input"
            placeholder="-----BEGIN PRIVATE KEY-----"
          />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="uploadVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="submitUpload">{{ t('ct.save') }}</el-button>
      </template>
    </el-dialog>

    <!-- Self-signed -->
    <el-dialog v-model="selfSignedVisible" :title="t('ct.selfSigned')" width="520px">
      <el-alert type="info" :closable="false" show-icon :title="t('ct.selfSignedNote')" class="dialog-note" />
      <el-form ref="selfSignedRef" :model="selfSignedForm" :rules="selfSignedRules" label-position="top">
        <el-form-item :label="t('ct.name')" prop="name">
          <el-input v-model="selfSignedForm.name" maxlength="64" />
        </el-form-item>
        <el-form-item :label="t('ct.domains')" prop="domainText">
          <el-input
            v-model="selfSignedForm.domainText"
            type="textarea"
            :rows="3"
            class="lp-mono"
            :placeholder="'example.com\nwww.example.com'"
          />
          <span class="hint">{{ t('ct.domainsHint') }}</span>
        </el-form-item>
        <el-form-item :label="t('ct.validDays')">
          <el-input-number v-model="selfSignedForm.days" :min="1" :max="3650" controls-position="right" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="selfSignedVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="submitSelfSigned">{{ t('create') }}</el-button>
      </template>
    </el-dialog>

    <!-- Let's Encrypt -->
    <el-dialog v-model="acmeVisible" :title="t('ct.requestLetsEncrypt')" width="560px" top="6vh">
      <el-alert type="warning" :closable="false" show-icon :title="t('ct.acmeNote')" class="dialog-note" />
      <el-form ref="acmeRef" :model="acmeForm" :rules="acmeRules" label-position="top">
        <el-form-item :label="t('ct.name')" prop="name">
          <el-input v-model="acmeForm.name" maxlength="64" />
        </el-form-item>
        <el-form-item :label="t('ct.domains')" prop="domainText">
          <el-input
            v-model="acmeForm.domainText"
            type="textarea"
            :rows="3"
            class="lp-mono"
            :placeholder="'example.com\nwww.example.com'"
          />
          <span class="hint">{{ t('ct.acmeDomainsHint') }}</span>
        </el-form-item>
        <el-form-item :label="t('ct.email')" prop="email">
          <el-input v-model="acmeForm.email" placeholder="admin@example.com" />
        </el-form-item>
        <el-form-item :label="t('ct.webroot')" prop="webroot">
          <el-select
            v-model="acmeForm.webroot"
            filterable
            allow-create
            default-first-option
            style="width: 100%"
            :placeholder="t('ct.webrootPlaceholder')"
          >
            <el-option v-for="site in sites" :key="site.id" :label="`${site.name} — ${site.doc_root}`" :value="site.doc_root" />
          </el-select>
          <span class="hint">{{ t('ct.webrootHint') }}</span>
        </el-form-item>
        <el-form-item>
          <el-switch v-model="acmeForm.staging" :active-text="t('ct.staging')" />
          <span class="hint hint--inline">{{ t('ct.stagingHint') }}</span>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="acmeVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="submitAcme">{{ t('ct.request') }}</el-button>
      </template>
    </el-dialog>

    <OperationLogDialog
      v-model="logVisible"
      :title="logTitle"
      :log="logText"
      :status="logStatus"
      :loading="saving"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { FormRules } from 'element-plus'
import { Plus, Refresh } from '@element-plus/icons-vue'
import OperationLogDialog from '@/components/OperationLogDialog.vue'
import {
  createSelfSignedCert, deleteCert, getAcmeStatus, getCerts, issueAcmeCert, renewCert, uploadCert,
} from '@/api'
import { useI18n } from '@/i18n'

type CertRow = {
  id: number
  name: string
  domains: string
  issuer: string
  subject: string
  not_before: string
  not_after: string
  source: string
  auto_renew: boolean
  days_remaining: number
  status: string
  used_by: string[]
}
type AcmeInfo = { client: string; available: boolean; version: string; message: string }

const props = defineProps<{ sites: { id: number; name: string; doc_root: string }[] }>()

const { t } = useI18n()

const certs = ref<CertRow[]>([])
const acme = ref<AcmeInfo | null>(null)
const loading = ref(false)
const saving = ref(false)
const busy = ref<number | null>(null)
const sites = computed(() => props.sites || [])

async function load() {
  loading.value = true
  try {
    const [certRes, acmeRes]: any[] = await Promise.all([getCerts(), getAcmeStatus()])
    if (certRes.success) certs.value = certRes.data || []
    if (acmeRes.success) acme.value = acmeRes.data
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}

function domainsOf(row: CertRow) {
  return row.domains.split(',').map((d) => d.trim()).filter(Boolean)
}

/** `CN=R3,O=Let's Encrypt,C=US` -> `R3 (Let's Encrypt)` */
function shortIssuer(issuer: string) {
  const parts = Object.fromEntries(
    issuer
      .split(/[,/]/)
      .map((part) => part.trim().split('='))
      .filter((pair) => pair.length === 2) as [string, string][],
  )
  const cn = parts.CN || ''
  const org = parts.O || ''
  if (cn && org && cn !== org) return `${cn} (${org})`
  return cn || org || issuer
}

function sourceLabel(source: string) {
  return (
    { acme: "Let's Encrypt", uploaded: t('ct.sourceUploaded'), 'self-signed': t('ct.sourceSelfSigned') }[
      source
    ] || source
  )
}

function statusClass(status: string) {
  return {
    valid: 'lp-pill--ok',
    expiring: 'lp-pill--warn',
    expired: 'lp-pill--danger',
  }[status] || ''
}

function expiryLabel(row: CertRow) {
  if (row.status === 'expired') return t('ct.expired')
  if (row.status === 'unknown') return t('ct.unknown')
  return t('ct.daysLeft', { n: row.days_remaining })
}

function parseDomains(text: string) {
  return text
    .split(/[\s,;]+/)
    .map((d) => d.trim())
    .filter(Boolean)
}

// --- Operation log ---
const logVisible = ref(false)
const logText = ref('')
const logTitle = ref('')
const logStatus = ref<'idle' | 'running' | 'success' | 'error'>('idle')

// --- Upload ---
const uploadVisible = ref(false)
const uploadRef = ref()
const uploadForm = reactive({ name: '', cert_pem: '', key_pem: '' })
const uploadRules = computed<FormRules>(() => ({
  name: [{ required: true, message: t('ct.nameRequired'), trigger: 'blur' }],
  cert_pem: [{ required: true, message: t('ct.certRequired'), trigger: 'blur' }],
  key_pem: [{ required: true, message: t('ct.keyRequired'), trigger: 'blur' }],
}))

function openUpload() {
  uploadForm.name = ''
  uploadForm.cert_pem = ''
  uploadForm.key_pem = ''
  uploadVisible.value = true
}

async function submitUpload() {
  try {
    await uploadRef.value?.validate()
  } catch {
    return
  }
  saving.value = true
  try {
    const res: any = await uploadCert({ ...uploadForm })
    if (res.success) {
      ElMessage.success(t('ct.saved'))
      uploadVisible.value = false
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

// --- Self-signed ---
const selfSignedVisible = ref(false)
const selfSignedRef = ref()
const selfSignedForm = reactive({ name: '', domainText: '', days: 825 })
const selfSignedRules = computed<FormRules>(() => ({
  name: [{ required: true, message: t('ct.nameRequired'), trigger: 'blur' }],
  domainText: [{ required: true, message: t('ct.domainsRequired'), trigger: 'blur' }],
}))

function openSelfSigned() {
  selfSignedForm.name = ''
  selfSignedForm.domainText = ''
  selfSignedForm.days = 825
  selfSignedVisible.value = true
}

async function submitSelfSigned() {
  try {
    await selfSignedRef.value?.validate()
  } catch {
    return
  }
  saving.value = true
  try {
    const res: any = await createSelfSignedCert({
      name: selfSignedForm.name,
      domains: parseDomains(selfSignedForm.domainText),
      days: selfSignedForm.days,
    })
    if (res.success) {
      ElMessage.success(t('ct.saved'))
      selfSignedVisible.value = false
      await load()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

// --- ACME ---
const acmeVisible = ref(false)
const acmeRef = ref()
const acmeForm = reactive({ name: '', domainText: '', email: '', webroot: '', staging: false })
const acmeRules = computed<FormRules>(() => ({
  name: [{ required: true, message: t('ct.nameRequired'), trigger: 'blur' }],
  domainText: [{ required: true, message: t('ct.domainsRequired'), trigger: 'blur' }],
  email: [{ required: true, message: t('ct.emailRequired'), trigger: 'blur' }],
  webroot: [{ required: true, message: t('ct.webrootRequired'), trigger: 'blur' }],
}))

function openAcme() {
  acmeForm.name = ''
  acmeForm.domainText = ''
  acmeForm.email = ''
  acmeForm.webroot = sites.value[0]?.doc_root || ''
  acmeForm.staging = false
  acmeVisible.value = true
}

async function submitAcme() {
  try {
    await acmeRef.value?.validate()
  } catch {
    return
  }
  saving.value = true
  logTitle.value = t('ct.requestLetsEncrypt')
  logText.value = t('ct.requestRunning')
  logStatus.value = 'running'
  logVisible.value = true
  acmeVisible.value = false
  try {
    const res: any = await issueAcmeCert({
      name: acmeForm.name,
      domains: parseDomains(acmeForm.domainText),
      email: acmeForm.email,
      webroot: acmeForm.webroot,
      staging: acmeForm.staging,
    })
    if (res.success) {
      logText.value = res.data?.log || ''
      logStatus.value = res.data?.success ? 'success' : 'error'
      if (res.data?.success) ElMessage.success(t('ct.issued'))
      await load()
    } else {
      logText.value = res.message || ''
      logStatus.value = 'error'
    }
  } catch (err: any) {
    logText.value = err?.message || ''
    logStatus.value = 'error'
  } finally {
    saving.value = false
  }
}

async function renew(row: CertRow) {
  busy.value = row.id
  logTitle.value = `${row.name} — ${t('ct.renew')}`
  logText.value = t('ct.renewRunning')
  logStatus.value = 'running'
  logVisible.value = true
  try {
    const res: any = await renewCert(row.id)
    if (res.success) {
      logText.value = res.data?.log || ''
      logStatus.value = res.data?.success ? 'success' : 'error'
      if (res.data?.success) ElMessage.success(t('ct.renewed'))
      await load()
    } else {
      logText.value = res.message || ''
      logStatus.value = 'error'
    }
  } catch (err: any) {
    logText.value = err?.message || ''
    logStatus.value = 'error'
  } finally {
    busy.value = null
  }
}

async function remove(row: CertRow) {
  try {
    await ElMessageBox.confirm(t('ct.confirmDelete', { name: row.name }), t('delete'), {
      type: 'warning',
      confirmButtonText: t('delete'),
      cancelButtonText: t('cancel'),
    })
  } catch {
    return
  }
  try {
    const res: any = await deleteCert(row.id)
    if (res.success) {
      ElMessage.success(t('ct.deleted'))
      await load()
    }
  } catch {
    // Interceptor reports it.
  }
}

defineExpose({ load })
onMounted(load)
</script>

<style scoped lang="scss">
.cert-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--lp-3);
  margin-bottom: var(--lp-4);
  flex-wrap: wrap;
}
.cert-toolbar__info { display: flex; align-items: center; gap: var(--lp-2); min-width: 0; }
.cert-toolbar__actions { display: flex; align-items: center; gap: var(--lp-2); flex-wrap: wrap; }
.acme-version { font-size: var(--lp-text-xs); max-width: 260px; }

.cell-stack { display: flex; flex-direction: column; gap: 2px; line-height: 1.35; min-width: 0; }
.cell-stack .lp-dim { font-size: 11px; }
.cert-name { font-weight: 600; }

.domain-list { display: flex; flex-wrap: wrap; gap: 4px; }
.domain-chip {
  padding: 1px 7px;
  border-radius: var(--lp-r-full);
  background: var(--lp-surface-sunken);
  color: var(--lp-text-soft);
  font-family: var(--lp-font-mono);
  font-size: var(--lp-text-xs);
}

.hint {
  display: block;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  line-height: 1.5;

  &--inline { display: inline; margin-left: var(--lp-3); }
}

.dialog-note { margin-bottom: var(--lp-4); border-radius: var(--lp-r-sm); }
.pem-input :deep(textarea) { font-size: 11px; line-height: 1.45; }
</style>
