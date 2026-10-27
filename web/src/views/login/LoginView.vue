<template>
  <div class="login-page">
    <div class="login-card">
      <div class="login-header">
        <svg class="login-mark" viewBox="0 0 32 32" width="52" height="52" aria-hidden="true">
          <rect width="32" height="32" rx="9" fill="var(--lp-accent)" />
          <path d="M9 8v14h8" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" fill="none" />
          <circle cx="22" cy="11" r="2.6" fill="#fff" />
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
        <el-form-item v-if="mfaRequired" prop="code">
          <el-input
            ref="codeRef"
            v-model="form.code"
            :placeholder="t('login.mfaCode')"
            :prefix-icon="Key"
            size="large"
            maxlength="6"
            inputmode="numeric"
            autocomplete="one-time-code"
            @keyup.enter="handleLogin"
          />
        </el-form-item>
        <el-button type="primary" size="large" :loading="loading" style="width:100%" @click="handleLogin">
          {{ t('login.loginBtn') }}
        </el-button>
      </el-form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useUserStore } from '@/stores/user'
import { useI18n } from '@/i18n'
import { login } from '@/api'
import { User, Lock, Key } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'

const router = useRouter()
const route = useRoute()
const userStore = useUserStore()
const { t } = useI18n()
const formRef = ref()
const loading = ref(false)

const form = reactive({ username: '', password: '', code: '' })
// Shown once the server says the account has two-factor login on.
const mfaRequired = ref(false)
const codeRef = ref()
const rules = computed(() => ({
  username: [{ required: true, message: t('login.usernameRequired'), trigger: 'blur' }],
  password: [{ required: true, message: t('login.passwordRequired'), trigger: 'blur' }],
}))

async function handleLogin() {
  await formRef.value?.validate()
  loading.value = true
  try {
    const res: any = await login({ ...form, code: mfaRequired.value ? form.code : undefined })
    if (!res.success && res.data?.mfa_required) {
      mfaRequired.value = true
      form.code = ''
      nextTick(() => codeRef.value?.focus())
      return
    }
    if (res.success) {
      userStore.setLogin(res.data.token, res.data.user.username)
      ElMessage.success(t('login.success'))
      // Back to the page the session expired on; only same-site paths.
      const back = route.query.redirect
      router.push(typeof back === 'string' && back.startsWith('/') && !back.startsWith('//') ? back : '/dashboard')
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
  padding: var(--lp-5);
  background: var(--lp-canvas);
  position: relative;
  overflow: hidden;
}

/* Two soft accent washes give the page depth without a heavy gradient slab. */
.login-page::before {
  content: "";
  position: absolute;
  inset: 0;
  background:
    radial-gradient(circle at 18% 22%, var(--lp-accent-soft) 0%, transparent 45%),
    radial-gradient(circle at 82% 78%, var(--lp-accent-soft) 0%, transparent 40%);
  pointer-events: none;
}

.login-card {
  width: 100%;
  max-width: 380px;
  padding: var(--lp-8);
  background: var(--lp-surface);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  box-shadow: var(--lp-shadow-lg);
  position: relative;
  z-index: 1;
}

.login-header {
  text-align: center;
  margin-bottom: var(--lp-6);

  h1 {
    margin: var(--lp-3) 0 var(--lp-1);
    font-size: var(--lp-text-xl);
    font-weight: 650;
    letter-spacing: -0.01em;
    color: var(--lp-text);
  }
  p {
    color: var(--lp-text-dim);
    font-size: var(--lp-text-base);
  }
}

.login-mark { display: block; margin: 0 auto; }

:deep(.el-form-item) { margin-bottom: var(--lp-4); }
:deep(.el-input__wrapper) { padding: 2px 11px; }
</style>
