<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('settings') }}</h1>
        <p class="lp-page-sub">{{ t('st.subtitle') }}</p>
      </div>
      <div class="lp-page-actions">
        <el-button size="small" :icon="Refresh" :loading="loading" @click="load">{{ t('refresh') }}</el-button>
        <!-- Security and update tabs save their own settings. -->
        <el-button v-if="!['security', 'update'].includes(tab)" type="primary" size="small" :loading="saving" @click="save">{{ t('st.saveSettings') }}</el-button>
      </div>
    </div>

    <el-tabs v-model="tab">
      <!-- ============ General ============ -->
      <el-tab-pane :label="t('st.tabGeneral')" name="general">
        <div class="setting-stack">
          <section class="lp-card">
            <header class="lp-card-head">
              <h2 class="lp-card-title"><el-icon><Setting /></el-icon>{{ t('st.panelSettings') }}</h2>
            </header>
            <div class="lp-card-body">
              <el-form label-position="top">
                <el-form-item :label="t('st.panelName')">
                  <el-input v-model="form.panel_name" placeholder="LinxPanel" />
                </el-form-item>
                <el-form-item :label="t('st.apiUrl')">
                  <el-input v-model="form.api_url" placeholder="http://0.0.0.0:7200" class="lp-mono" />
                  <span class="hint">{{ t('st.apiUrlHint') }}</span>
                </el-form-item>
              </el-form>
            </div>
          </section>

          <section class="lp-card">
            <header class="lp-card-head">
              <h2 class="lp-card-title"><el-icon><Connection /></el-icon>{{ t('st.openrestyTitle') }}</h2>
              <span class="lp-pill" :class="openresty?.installed ? 'lp-pill--ok' : ''">
                <span class="lp-pill__dot"></span>
                {{ openresty?.installed ? openresty?.version || t('st.installed') : t('st.notInstalled') }}
              </span>
            </header>
            <div class="lp-card-body">
              <el-form label-position="top">
                <div class="grid-2">
                  <el-form-item :label="t('st.openrestyBin')">
                    <el-input v-model="form.openresty_bin" placeholder="/usr/local/openresty/nginx/sbin/nginx" class="lp-mono" />
                  </el-form-item>
                  <el-form-item :label="t('st.openrestyConfPath')">
                    <el-input v-model="form.openresty_conf_path" placeholder="/usr/local/openresty/nginx/conf/nginx.conf" class="lp-mono" />
                  </el-form-item>
                  <el-form-item :label="t('st.openrestySitesDir')">
                    <el-input v-model="form.openresty_sites_dir" placeholder="/usr/local/openresty/nginx/conf/sites-enabled" class="lp-mono" />
                  </el-form-item>
                  <el-form-item :label="t('st.phpFpmAddr')">
                    <el-input v-model="form.php_fpm_addr" placeholder="127.0.0.1:9000" class="lp-mono" />
                    <span class="hint">{{ t('st.phpFpmHint') }}</span>
                  </el-form-item>
                </div>
                <el-form-item>
                  <el-switch v-model="openrestyEnabled" :active-text="t('st.openrestyEnabled')" />
                  <span class="hint hint--inline">{{ t('st.openrestyEnabledHint') }}</span>
                </el-form-item>
              </el-form>
            </div>
          </section>
        </div>
      </el-tab-pane>

      <!-- ============ Security ============ -->
      <el-tab-pane :label="t('st.tabSecurity')" name="security" lazy>
        <PanelSecurity />
      </el-tab-pane>

      <!-- ============ Update ============ -->
      <el-tab-pane :label="t('upd.tab')" name="update" lazy>
        <PanelUpdate />
      </el-tab-pane>

      <!-- ============ Alerts ============ -->
      <el-tab-pane :label="t('st.tabAlerts')" name="alerts">
        <section class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Bell /></el-icon>{{ t('st.alertTitle') }}</h2>
          </header>
          <div class="lp-card-body">
            <p class="section-note">{{ t('st.alertHint') }}</p>
            <el-form label-position="top">
              <div class="grid-3">
                <el-form-item v-for="item in alertFields" :key="item.key" :label="item.label">
                  <el-slider
                    v-model="alerts[item.key]"
                    :min="50"
                    :max="100"
                    :marks="{ 70: '70', 90: '90' }"
                    show-input
                    :show-input-controls="false"
                  />
                </el-form-item>
              </div>
              <el-form-item>
                <el-switch v-model="alertsEnabled" :active-text="t('st.alertEnabled')" />
              </el-form-item>
            </el-form>
          </div>
        </section>
      </el-tab-pane>

      <!-- ============ About ============ -->
      <el-tab-pane :label="t('st.tabAbout')" name="about">
        <section class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><InfoFilled /></el-icon>{{ t('st.aboutTitle') }}</h2>
          </header>
          <div class="lp-card-body">
            <dl class="about-grid">
              <div><dt>{{ t('st.panelVersion') }}</dt><dd>v{{ info?.panel_version || '—' }}</dd></div>
              <div><dt>{{ t('db.hostname') }}</dt><dd>{{ info?.hostname || '—' }}</dd></div>
              <div><dt>{{ t('db.system') }}</dt><dd>{{ info?.os_version || info?.os_name || '—' }}</dd></div>
              <div><dt>{{ t('db.kernel') }}</dt><dd class="lp-truncate">{{ info?.kernel_version || '—' }}</dd></div>
              <div><dt>{{ t('db.platform') }}</dt><dd>{{ info?.platform || '—' }}</dd></div>
              <div><dt>{{ t('db.processor') }}</dt><dd class="lp-truncate">{{ info?.cpu_model || '—' }}</dd></div>
              <div><dt>{{ t('db.physCores') }}</dt><dd>{{ info?.physical_cores }} / {{ info?.logical_cores }}</dd></div>
              <div><dt>{{ t('db.memory') }}</dt><dd>{{ formatBytes(info?.mem_total) }}</dd></div>
            </dl>
          </div>
        </section>
      </el-tab-pane>
    </el-tabs>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { ElMessage } from 'element-plus'
import { Bell, Connection, InfoFilled, Refresh, Setting } from '@element-plus/icons-vue'
import { getOpenRestyStatus, getSettings, saveSettingsApi } from '@/api'
import { useI18n } from '@/i18n'
import { useRealtimeStore } from '@/stores/realtime'
import { formatBytes } from '@/utils/format'
import PanelSecurity from './PanelSecurity.vue'
import PanelUpdate from './PanelUpdate.vue'
import { useRoute } from 'vue-router'

const { t } = useI18n()
const realtime = useRealtimeStore()
const { staticInfo: info } = storeToRefs(realtime)

const route = useRoute()
// ?tab=update opens a tab directly (the header's update badge links there).
const tab = ref(typeof route.query.tab === 'string' ? route.query.tab : 'general')
const loading = ref(false)
const saving = ref(false)
const openresty = ref<any>(null)

const form = reactive({
  panel_name: 'LinxPanel',
  api_url: '',
  openresty_bin: '',
  openresty_conf_path: '',
  openresty_sites_dir: '',
  php_fpm_addr: '',
})
const openrestyEnabled = ref(true)
const alertsEnabled = ref(false)
const alerts = reactive<Record<string, number>>({ cpu: 90, memory: 90, disk: 85 })

const alertFields = computed(() => [
  { key: 'cpu', label: `${t('db.cpu')} (%)` },
  { key: 'memory', label: `${t('db.memory')} (%)` },
  { key: 'disk', label: `${t('db.disk')} (%)` },
])

async function load() {
  loading.value = true
  try {
    const [settingsRes, openrestyRes]: any[] = await Promise.all([
      getSettings(),
      getOpenRestyStatus().catch(() => ({ success: false })),
    ])

    if (settingsRes.success) {
      const data = settingsRes.data || {}
      form.panel_name = data.panel_name || 'LinxPanel'
      form.api_url = data.api_url || ''
      form.openresty_bin = data.openresty_bin || ''
      form.openresty_conf_path = data.openresty_conf_path || ''
      form.openresty_sites_dir = data.openresty_sites_dir || ''
      form.php_fpm_addr = data.php_fpm_addr || ''
      openrestyEnabled.value = data.openresty_enabled !== 'false'
      alertsEnabled.value = data.alerts_enabled === 'true'
      alerts.cpu = Number(data.alert_cpu || 90)
      alerts.memory = Number(data.alert_memory || 90)
      alerts.disk = Number(data.alert_disk || 85)
    }
    if (openrestyRes.success) openresty.value = openrestyRes.data
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}

async function save() {
  saving.value = true
  try {
    const payload: Record<string, string> = {
      panel_name: form.panel_name,
      api_url: form.api_url,
      openresty_bin: form.openresty_bin,
      openresty_conf_path: form.openresty_conf_path,
      openresty_sites_dir: form.openresty_sites_dir,
      php_fpm_addr: form.php_fpm_addr,
      openresty_enabled: String(openrestyEnabled.value),
      alerts_enabled: String(alertsEnabled.value),
      alert_cpu: String(alerts.cpu),
      alert_memory: String(alerts.memory),
      alert_disk: String(alerts.disk),
    }
    const res: any = await saveSettingsApi(payload)
    if (res.success) ElMessage.success(t('st.saved'))
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

onMounted(load)
</script>

<style scoped lang="scss">
.setting-stack { display: flex; flex-direction: column; gap: var(--lp-4); }

.grid-2,
.grid-3 { display: grid; gap: 0 var(--lp-4); }
.grid-2 { grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); }
.grid-3 { grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); }

.hint {
  display: block;
  /* Form-item content is a flex row; keep each hint on its own line. */
  flex-basis: 100%;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  line-height: 1.5;

  &--inline { display: inline; margin-left: var(--lp-3); }
}

.section-note {
  margin-bottom: var(--lp-4);
  font-size: var(--lp-text-base);
  color: var(--lp-text-dim);
}

/* About */
.about-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
  gap: var(--lp-4);
  margin: 0;

  div { min-width: 0; }
  dt { font-size: var(--lp-text-xs); color: var(--lp-text-dim); margin-bottom: 3px; }
  dd { margin: 0; font-size: var(--lp-text-base); font-weight: 500; color: var(--lp-text); }
}
</style>
