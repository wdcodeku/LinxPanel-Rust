<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.security') }}</h1>
        <p class="lp-page-sub">{{ subtitle }}</p>
      </div>
      <div class="lp-page-actions">
        <el-button size="small" :icon="Refresh" :loading="loading" @click="loadAll">{{ t('refresh') }}</el-button>
      </div>
    </div>

    <el-tabs v-model="tab" class="tabs">
      <el-tab-pane :label="t('sec.firewall')" name="firewall">
        <div class="stack">
          <div class="lp-card">
            <div class="lp-card-body status-row">
              <div class="status-main">
                <span class="lp-pill" :class="status?.enabled ? 'lp-pill--ok' : 'lp-pill--danger'">
                  <span class="lp-pill__dot"></span>
                  {{ status?.enabled ? t('sec.active') : t('sec.inactive') }}
                </span>
                <div class="status-meta">
                  <span class="status-backend">{{ status?.backend === 'none' ? t('sec.noBackend') : status?.backend }}</span>
                  <span v-if="status?.default_incoming" class="lp-dim">
                    {{ t('sec.defaultIncoming') }}: {{ status.default_incoming }}
                  </span>
                </div>
              </div>
              <el-switch
                v-if="status?.available"
                :model-value="!!status?.enabled"
                :disabled="!status?.manageable || toggling"
                :loading="toggling"
                @change="(value: string | number | boolean) => toggleFirewall(value === true)"
              />
            </div>
          </div>

          <el-alert
            v-if="status && !status.manageable && status.message"
            :title="status.message"
            type="info"
            :closable="false"
            show-icon
          />

          <div class="lp-card">
            <header class="lp-card-head">
              <h2 class="lp-card-title"><el-icon><Lock /></el-icon>{{ t('sec.rules') }}</h2>
              <el-button
                type="primary"
                size="small"
                :icon="Plus"
                :disabled="!status?.manageable"
                @click="ruleVisible = true"
              >{{ t('sec.addRule') }}</el-button>
            </header>
            <div class="lp-card-body lp-card-body--flush">
              <el-table :data="rules" v-loading="rulesLoading" size="small" :empty-text="rulesEmpty">
                <el-table-column :label="t('sec.port')" width="120">
                  <template #default="{ row }"><span class="lp-mono">{{ row.port }}</span></template>
                </el-table-column>
                <el-table-column :label="t('sec.protocol')" prop="protocol" width="100" />
                <el-table-column :label="t('sec.action')" width="120">
                  <template #default="{ row }">
                    <span class="lp-pill" :class="row.action.includes('DENY') ? 'lp-pill--danger' : 'lp-pill--ok'">
                      {{ row.action }}
                    </span>
                  </template>
                </el-table-column>
                <el-table-column :label="t('sec.source')" prop="source" min-width="160" />
                <el-table-column :label="t('proc.actions')" width="90" align="right">
                  <template #default="{ row }">
                    <el-button link type="danger" size="small" :disabled="!status?.manageable" @click="removeRule(row)">
                      {{ t('delete') }}
                    </el-button>
                  </template>
                </el-table-column>
              </el-table>
            </div>
          </div>
        </div>
      </el-tab-pane>

      <el-tab-pane :label="t('sec.ports')" name="ports">
        <div class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Connection /></el-icon>{{ t('sec.listening') }}</h2>
            <el-input
              v-model="portKeyword"
              :placeholder="t('sec.searchPorts')"
              :prefix-icon="Search"
              clearable
              size="small"
              style="width: 220px"
            />
          </header>
          <div class="lp-card-body lp-card-body--flush">
            <el-table :data="filteredPorts" v-loading="portsLoading" size="small" :empty-text="portsEmpty">
              <el-table-column :label="t('sec.port')" width="100" sortable :sort-method="(a: any, b: any) => a.port - b.port">
                <template #default="{ row }"><span class="lp-mono port-num">{{ row.port }}</span></template>
              </el-table-column>
              <el-table-column :label="t('sec.protocol')" prop="protocol" width="90" />
              <el-table-column :label="t('sec.address')" width="180">
                <template #default="{ row }">
                  <span class="lp-mono">{{ row.address }}</span>
                  <span v-if="isPublic(row.address)" class="lp-pill lp-pill--warn exposure">{{ t('sec.public') }}</span>
                </template>
              </el-table-column>
              <el-table-column :label="t('sec.process')" min-width="180">
                <template #default="{ row }">
                  <span>{{ row.process || '-' }}</span>
                  <span v-if="row.pid" class="lp-dim"> (pid {{ row.pid }})</span>
                </template>
              </el-table-column>
            </el-table>
          </div>
        </div>
      </el-tab-pane>
    </el-tabs>

    <el-dialog v-model="ruleVisible" :title="t('sec.addRule')" width="460px">
      <el-form ref="ruleFormRef" :model="ruleForm" :rules="ruleRules" label-position="top">
        <el-form-item :label="t('sec.port')" prop="port">
          <el-input v-model="ruleForm.port" placeholder="8080 / 8000:8100" />
        </el-form-item>
        <el-form-item :label="t('sec.protocol')">
          <el-radio-group v-model="ruleForm.protocol">
            <el-radio-button value="tcp">TCP</el-radio-button>
            <el-radio-button value="udp">UDP</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item :label="t('sec.action')">
          <el-radio-group v-model="ruleForm.action">
            <el-radio-button value="allow">{{ t('sec.allow') }}</el-radio-button>
            <el-radio-button value="deny">{{ t('sec.deny') }}</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item :label="t('sec.source')">
          <el-input v-model="ruleForm.source" :placeholder="t('sec.sourcePlaceholder')" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="ruleVisible = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="savingRule" @click="addRule">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { FormRules } from 'element-plus'
import { Connection, Lock, Plus, Refresh, Search } from '@element-plus/icons-vue'
import {
  addFirewallRule, deleteFirewallRule, getFirewallRules, getFirewallStatus,
  getListeningPorts, toggleFirewall as toggleFirewallApi,
} from '@/api'
import { useI18n } from '@/i18n'

type FirewallStatus = {
  backend: string
  available: boolean
  manageable: boolean
  enabled: boolean
  default_incoming: string
  message: string
}
type FirewallRule = { id: string; port: string; protocol: string; source: string; action: string }
type ListeningPort = { protocol: string; address: string; port: number; pid: number | null; process: string }

const { t } = useI18n()

const tab = ref('firewall')
const loading = ref(false)
const status = ref<FirewallStatus | null>(null)
const rules = ref<FirewallRule[]>([])
const rulesLoading = ref(false)
const rulesError = ref('')
const ports = ref<ListeningPort[]>([])
const portsLoading = ref(false)
const portsError = ref('')
const portKeyword = ref('')
const toggling = ref(false)

const subtitle = computed(() =>
  t('sec.subtitle', { rules: rules.value.length, ports: ports.value.length }),
)
const rulesEmpty = computed(() => rulesError.value || t('sec.noRules'))
const portsEmpty = computed(() => portsError.value || t('sec.noPorts'))

const filteredPorts = computed(() => {
  const search = portKeyword.value.trim().toLowerCase()
  if (!search) return ports.value
  return ports.value.filter(
    (row) =>
      String(row.port).includes(search) ||
      row.process.toLowerCase().includes(search) ||
      row.address.toLowerCase().includes(search),
  )
})

/** A socket bound to a wildcard address is reachable from outside the host. */
function isPublic(address: string) {
  return address === '0.0.0.0' || address === '::' || address === '*'
}

async function loadAll() {
  loading.value = true
  await Promise.all([loadFirewall(), loadPorts()])
  loading.value = false
}

async function loadFirewall() {
  rulesLoading.value = true
  rulesError.value = ''
  try {
    const statusRes: any = await getFirewallStatus()
    if (statusRes.success) status.value = statusRes.data
    if (!status.value?.available) {
      rules.value = []
      rulesError.value = status.value?.message || ''
      return
    }
    const rulesRes: any = await getFirewallRules()
    if (rulesRes.success) {
      rules.value = rulesRes.data || []
    } else {
      rules.value = []
      rulesError.value = rulesRes.message || ''
    }
  } catch {
    rules.value = []
  } finally {
    rulesLoading.value = false
  }
}

async function loadPorts() {
  portsLoading.value = true
  portsError.value = ''
  try {
    const res: any = await getListeningPorts()
    if (res.success) {
      ports.value = res.data || []
    } else {
      ports.value = []
      portsError.value = res.message || ''
    }
  } catch {
    ports.value = []
  } finally {
    portsLoading.value = false
  }
}

async function toggleFirewall(enabled: boolean) {
  try {
    await ElMessageBox.confirm(
      enabled ? t('sec.confirmEnable') : t('sec.confirmDisable'),
      t('sec.confirmTitle'),
      { type: 'warning', confirmButtonText: t('confirm'), cancelButtonText: t('cancel') },
    )
  } catch {
    return
  }
  toggling.value = true
  try {
    const res: any = await toggleFirewallApi(enabled)
    if (res.success) {
      ElMessage.success(t('sec.updated'))
      await loadFirewall()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    toggling.value = false
  }
}

// --- Add rule ---
const ruleVisible = ref(false)
const savingRule = ref(false)
const ruleFormRef = ref()
const ruleForm = reactive({ port: '', protocol: 'tcp', action: 'allow', source: '' })

const ruleRules = computed<FormRules>(() => ({
  port: [
    { required: true, message: t('sec.portRequired'), trigger: 'blur' },
    {
      validator: (_rule: any, value: string, callback: (error?: Error) => void) => {
        const parts = String(value || '').split(':')
        const valid =
          parts.length <= 2 &&
          parts.every((part) => /^\d+$/.test(part) && Number(part) > 0 && Number(part) <= 65535)
        callback(valid ? undefined : new Error(t('sec.portInvalid')))
      },
      trigger: 'blur',
    },
  ],
}))

async function addRule() {
  try {
    await ruleFormRef.value?.validate()
  } catch {
    return
  }
  savingRule.value = true
  try {
    const res: any = await addFirewallRule({ ...ruleForm })
    if (res.success) {
      ElMessage.success(t('sec.ruleAdded'))
      ruleVisible.value = false
      ruleForm.port = ''
      ruleForm.source = ''
      await loadFirewall()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    savingRule.value = false
  }
}

async function removeRule(row: FirewallRule) {
  try {
    await ElMessageBox.confirm(
      t('sec.confirmDeleteRule', { port: row.port }),
      t('sec.confirmTitle'),
      { type: 'warning', confirmButtonText: t('delete'), cancelButtonText: t('cancel') },
    )
  } catch {
    return
  }
  try {
    const res: any = await deleteFirewallRule(row.id)
    if (res.success) {
      ElMessage.success(t('sec.ruleDeleted'))
      await loadFirewall()
    }
  } catch {
    // Interceptor reports it.
  }
}

onMounted(loadAll)
</script>

<style scoped lang="scss">
.tabs { margin-top: calc(var(--lp-2) * -1); }
.stack { display: flex; flex-direction: column; gap: var(--lp-4); }

.status-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--lp-4);
  flex-wrap: wrap;
}
.status-main { display: flex; align-items: center; gap: var(--lp-3); }
.status-meta { display: flex; flex-direction: column; line-height: 1.4; }
.status-backend { font-weight: 600; font-size: var(--lp-text-md); }
.status-meta .lp-dim { font-size: var(--lp-text-xs); }

.port-num { font-weight: 600; }
.exposure { margin-left: var(--lp-2); }
</style>
