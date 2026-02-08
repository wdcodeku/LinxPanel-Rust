<template>
  <div class="login-page">
    <div class="login-card">
      <div class="login-header">
        <svg viewBox="0 0 64 64" width="48" height="48">
          <rect width="64" height="64" rx="12" fill="var(--lp-primary)"/>
          <text x="32" y="44" font-family="Arial" font-size="32" font-weight="bold" fill="white" text-anchor="middle">LP</text>
        </svg>
        <h1>{{ t('login.title') }}</h1>
        <p>{{ t('login.subtitle') }}</p>
      </div>
      <el-form ref="formRef" :model="form" :rules="rules" @submit.prevent="handleLogin">
        <el-form-item prop="username">
          <el-input v-model="form.username" :placeholder="t('login.username')" :prefix-icon="User" size="large" />
        </el-form-item>
        <el-form-item prop="password">
          <el-input v-model="form.password" type="password" :placeholder="t('login.password')" :prefix-icon="Lock" size="large" show-password @keyup.enter="handleLogin" />
        </el-form-item>
        <el-button type="primary" size="large" :loading="loading" style="width:100%" @click="handleLogin">
          {{ t('login.loginBtn') }}
        </el-button>
      </el-form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed } from 'vue'
import { useRouter } from 'vue-router'
import { useUserStore } from '@/stores/user'
import { useI18n } from '@/i18n'
import { login } from '@/api'
import { User, Lock } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'

const router = useRouter()
const userStore = useUserStore()
const { t } = useI18n()
const formRef = ref()
const loading = ref(false)

const form = reactive({ username: '', password: '' })
const rules = computed(() => ({
  username: [{ required: true, message: t('login.usernameRequired'), trigger: 'blur' }],
  password: [{ required: true, message: t('login.passwordRequired'), trigger: 'blur' }],
}))

async function handleLogin() {
  await formRef.value?.validate()
  loading.value = true
  try {
    const res: any = await login(form)
    if (res.success) {
      userStore.setLogin(res.data.token, res.data.user.username)
      ElMessage.success(t('login.success'))
      router.push('/dashboard')
    }
  } finally { loading.value = false }
}
</script>
<style scoped lang="scss">
.login-page {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, var(--lp-primary) 0%, var(--lp-primary-dark) 100%);
  position: relative;
  overflow: hidden;
}
.login-page::before {
  content: "";
  position: absolute;
  inset: 0;
  background: radial-gradient(circle at 30% 50%, rgba(255,255,255,0.08) 0%, transparent 60%),
              radial-gradient(circle at 70% 80%, rgba(255,255,255,0.05) 0%, transparent 50%);
  pointer-events: none;
}
.login-card {
  width: 400px;
  padding: 40px;
  background: var(--lp-card-bg);
  border-radius: var(--lp-radius-xl);
  box-shadow: var(--lp-shadow-lg);
  position: relative;
  z-index: 1;
  backdrop-filter: blur(20px);
}
.login-header {
  text-align: center;
  margin-bottom: 32px;
  h1 { margin: 12px 0 4px; font-size: 24px; color: var(--lp-text-primary); }
  p { color: var(--lp-text-secondary); font-size: 14px; }
}
</style>