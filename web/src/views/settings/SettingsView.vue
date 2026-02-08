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
      Object.assign(form, res.data)
    }
  } catch {}
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