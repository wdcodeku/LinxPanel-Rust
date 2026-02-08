<template>
  <div class="settings-page">
    <el-row :gutter="16">
      <el-col :xs="24" :lg="12">
        <el-card shadow="never" class="setting-card">
          <template #header>
            <div class="card-hd"><el-icon><Setting /></el-icon> {{ t('st.panelSettings') }}</div>
          </template>
          <el-form :model="form" label-width="100px" label-position="left">
            <el-form-item :label="t('st.panelName')">
              <el-input v-model="form.panel_name" placeholder="LinxPanel" />
            </el-form-item>
            <el-form-item :label="t('st.panelPort')">
              <el-input-number v-model.number="form.panel_port" :min="1024" :max="65535" />
            </el-form-item>
            <el-form-item :label="t('st.sessionTimeout')">
              <el-select v-model="form.session_timeout">
                <el-option :label="t('st.hours1')" value="1" />
                <el-option :label="t('st.hours6')" value="6" />
                <el-option :label="t('st.hours12')" value="12" />
                <el-option :label="t('st.hours24')" value="24" />
                <el-option :label="t('st.days7')" value="168" />
              </el-select>
            </el-form-item>
            <el-form-item :label="t('st.apiUrl')">
              <el-input v-model="form.api_url" placeholder="http://0.0.0.0:7200" />
            </el-form-item>
            <el-divider>{{ t('st.openrestySettings') }}</el-divider>
            <el-form-item :label="t('st.openrestyEnabled')">
              <el-switch v-model="form.openresty_enabled" />
            </el-form-item>
            <el-form-item :label="t('st.openrestyBin')">
              <el-input v-model="form.openresty_bin" :placeholder="t('st.openrestyBinPlaceholder')" />
            </el-form-item>
            <el-form-item :label="t('st.openrestySitesDir')">
              <el-input v-model="form.openresty_sites_dir" :placeholder="t('st.openrestySitesDirPlaceholder')" />
            </el-form-item>
            <el-form-item :label="t('st.openrestyConfPath')">
              <el-input v-model="form.openresty_conf_path" :placeholder="t('st.openrestyConfPathPlaceholder')" />
            </el-form-item>
            <el-form-item :label="t('st.phpFpmAddr')">
              <el-input v-model="form.php_fpm_addr" :placeholder="t('st.phpFpmAddrPlaceholder')" />
            </el-form-item>
            <el-form-item :label="t('st.openrestyIpv6')">
              <el-switch v-model="form.openresty_listen_ipv6" />
            </el-form-item>
            <el-form-item>
              <el-button type="primary" @click="saveSettings" :loading="saving">{{ t('st.saveSettings') }}</el-button>
            </el-form-item>
          </el-form>
        </el-card>
      </el-col>
      <el-col :xs="24" :lg="12">
        <el-card shadow="never" class="setting-card">
          <template #header>
            <div class="card-hd"><el-icon><Lock /></el-icon> {{ t('st.changePassword') }}</div>
          </template>
          <el-form :model="pwForm" :rules="pwRules" ref="pwRef" label-width="100px" label-position="left">
            <el-form-item :label="t('st.oldPassword')" prop="old_password">
              <el-input v-model="pwForm.old_password" type="password" show-password />
            </el-form-item>
            <el-form-item :label="t('st.newPassword')" prop="new_password">
              <el-input v-model="pwForm.new_password" type="password" show-password />
            </el-form-item>
            <el-form-item :label="t('st.confirmPassword')" prop="confirm_password">
              <el-input v-model="pwForm.confirm_password" type="password" show-password />
            </el-form-item>
            <el-form-item>
              <el-button type="primary" @click="changePassword" :loading="changingPw">{{ t('st.updatePassword') }}</el-button>
            </el-form-item>
          </el-form>
        </el-card>
      </el-col>
    </el-row>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted } from 'vue'
import { useI18n } from '@/i18n'
import { getSettings, saveSettingsApi, changePasswordApi } from '@/api'
import { ElMessage } from 'element-plus'

const { t } = useI18n()
const saving = ref(false)
const changingPw = ref(false)
const pwRef = ref()

const form = reactive({
  panel_name: 'LinxPanel',
  panel_port: 7200,
  session_timeout: '24',
  api_url: '',
  openresty_enabled: false,
  openresty_bin: '/usr/local/openresty/nginx/sbin/nginx',
  openresty_sites_dir: '/usr/local/openresty/nginx/conf/sites-enabled',
  openresty_conf_path: '/usr/local/openresty/nginx/conf/nginx.conf',
  php_fpm_addr: '127.0.0.1:9000',
  openresty_listen_ipv6: true,
})

const pwForm = reactive({
  old_password: '',
  new_password: '',
  confirm_password: '',
})

const pwRules = computed(() => ({
  old_password: [{ required: true, message: t('st.passwordRequired'), trigger: 'blur' }],
  new_password: [{ required: true, message: t('st.passwordRequired'), trigger: 'blur' }],
  confirm_password: [{ required: true, message: t('st.passwordRequired'), trigger: 'blur' }],
}))

async function loadSettings() {
  try {
    const res: any = await getSettings()
    if (res.success && res.data) {
      const data = res.data as Record<string, any>
      form.panel_name = String(data.panel_name ?? form.panel_name)
      form.panel_port = Number(data.panel_port ?? form.panel_port)
      form.session_timeout = String(data.session_timeout ?? form.session_timeout)
      form.api_url = String(data.api_url ?? form.api_url)
      form.openresty_enabled = parseBoolLike(data.openresty_enabled, form.openresty_enabled)
      form.openresty_bin = String(data.openresty_bin ?? form.openresty_bin)
      form.openresty_sites_dir = String(data.openresty_sites_dir ?? form.openresty_sites_dir)
      form.openresty_conf_path = String(data.openresty_conf_path ?? form.openresty_conf_path)
      form.php_fpm_addr = String(data.php_fpm_addr ?? form.php_fpm_addr)
      form.openresty_listen_ipv6 = parseBoolLike(data.openresty_listen_ipv6, form.openresty_listen_ipv6)
    }
  } catch {}
}

function parseBoolLike(value: unknown, fallback: boolean) {
  if (typeof value === 'boolean') return value
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase()
    if (['1', 'true', 'yes', 'on'].includes(text)) return true
    if (['0', 'false', 'no', 'off'].includes(text)) return false
  }
  if (typeof value === 'number') return value !== 0
  return fallback
}

async function saveSettings() {
  saving.value = true
  try {
    const res: any = await saveSettingsApi(form)
    if (res.success) ElMessage.success(t('st.saved'))
    else ElMessage.error(res.message)
  } finally { saving.value = false }
}

async function changePassword() {
  if (pwForm.new_password !== pwForm.confirm_password) {
    ElMessage.warning(t('st.passwordMismatch'))
    return
  }
  await pwRef.value?.validate()
  changingPw.value = true
  try {
    const res: any = await changePasswordApi(pwForm as any)
    if (res.success) {
      ElMessage.success(t('st.passwordChanged'))
      pwForm.old_password = ''
      pwForm.new_password = ''
      pwForm.confirm_password = ''
    } else ElMessage.error(res.message)
  } finally { changingPw.value = false }
}

onMounted(() => loadSettings())
</script>
<style scoped lang="scss">
.settings-page { max-width: 1200px; }
.setting-card {
  background: var(--lp-card-bg);
  border: 1px solid var(--lp-border-light);
  border-radius: var(--lp-radius-lg);
  :deep(.el-card__header) { padding: 14px 20px; border-bottom: 1px solid var(--lp-border-light); }
  :deep(.el-card__body) { padding: 20px; }
}
.card-hd {
  display: flex; align-items: center; gap: 8px;
  font-size: 15px; font-weight: 600; color: var(--lp-text-primary);
  .el-icon { color: var(--lp-primary); }
}
</style>
