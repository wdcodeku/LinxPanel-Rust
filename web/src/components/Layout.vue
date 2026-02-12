<template>
  <el-container class="layout">
    <el-aside :width="isCollapse ? '64px' : '220px'" class="sidebar">
      <div class="logo">
        <svg viewBox="0 0 64 64" width="32" height="32">
          <rect width="64" height="64" rx="12" fill="var(--lp-primary)"/>
          <text x="32" y="44" font-family="Arial" font-size="32" font-weight="bold" fill="white" text-anchor="middle">LP</text>
        </svg>
        <transition name="fade">
          <span v-show="!isCollapse" class="logo-text">LinxPanel</span>
        </transition>
      </div>
      <el-scrollbar>
        <nav class="sidebar-nav">
          <button
            v-for="item in menuItems"
            :key="item.path"
            type="button"
            class="nav-item"
            :class="{ active: isMenuActive(item.path) }"
            @click="goTo(item.path)"
          >
            <el-icon><component :is="item.icon" /></el-icon>
            <span v-show="!isCollapse" class="nav-label">{{ t(item.titleKey) }}</span>
          </button>
        </nav>
      </el-scrollbar>
      <div class="sidebar-footer" v-show="!isCollapse">
        <span class="version">v0.1.0</span>
      </div>
    </el-aside>
    <el-container>
      <el-header class="header">
        <div class="header-left">
          <el-icon class="collapse-btn" @click="isCollapse = !isCollapse" :size="20">
            <Fold v-if="!isCollapse" /><Expand v-else />
          </el-icon>
          <div class="platform-badges" v-if="platformInfo.hostname">
            <span class="badge">
              <el-icon :size="12"><Monitor /></el-icon>
              {{ platformInfo.hostname }}
            </span>
            <span class="badge">{{ platformInfo.os_name }} {{ platformInfo.arch }}</span>
            <span class="badge badge-success">
              <el-icon :size="12"><Timer /></el-icon>
              {{ formatUptime(platformInfo.uptime) }}
            </span>
          </div>
        </div>
        <div class="header-right">
          <el-tooltip :content="t('language')" :show-after="250">
            <el-button circle size="small" @click="toggleLang" class="lang-toggle">
              <span style="font-size:12px;font-weight:600">{{ locale === 'zh' ? 'EN' : '中' }}</span>
            </el-button>
          </el-tooltip>
          <el-button circle size="small" @click="toggleTheme" class="theme-toggle">
            <el-icon :size="16"><Sunny v-if="userStore.theme === 'dark'" /><Moon v-else /></el-icon>
          </el-button>
          <el-dropdown trigger="click" popper-class="user-dropdown-popper">
            <span class="user-info">
              <el-avatar :size="28" :icon="UserFilled" />
              <span class="user-name">{{ userStore.username }}</span>
            </span>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item @click="openChangePassword">
                  <el-icon><Lock /></el-icon> {{ t('st.changePassword') }}
                </el-dropdown-item>
                <el-dropdown-item divided @click="handleLogout">
                  <el-icon><SwitchButton /></el-icon> {{ t('logout') }}
                </el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
        </div>
      </el-header>
      <el-main class="main-content">
        <router-view v-slot="{ Component }">
          <transition name="slide" mode="out-in">
            <component :is="Component" />
          </transition>
        </router-view>
      </el-main>
    </el-container>
    <el-dialog
      v-model="showChangePassword"
      :title="t('st.changePassword')"
      width="460px"
      class="change-password-dialog"
      @closed="resetPasswordForm"
    >
      <el-form
        ref="pwRef"
        :model="pwForm"
        :rules="pwRules"
        label-position="top"
        @submit.prevent
      >
        <el-form-item :label="t('st.oldPassword')" prop="old_password">
          <el-input
            v-model="pwForm.old_password"
            type="password"
            show-password
            autocomplete="current-password"
          />
        </el-form-item>
        <el-form-item :label="t('st.newPassword')" prop="new_password">
          <el-input
            v-model="pwForm.new_password"
            type="password"
            show-password
            autocomplete="new-password"
          />
        </el-form-item>
        <el-form-item :label="t('st.confirmPassword')" prop="confirm_password">
          <el-input
            v-model="pwForm.confirm_password"
            type="password"
            show-password
            autocomplete="new-password"
            @keyup.enter="changePassword"
          />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="showChangePassword = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="changingPw" @click="changePassword">
          {{ t('st.updatePassword') }}
        </el-button>
      </template>
    </el-dialog>
  </el-container>
</template>
<script setup lang="ts">
import { ref, reactive, computed, nextTick, onMounted, onUnmounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useUserStore } from '@/stores/user'
import { useI18n } from '@/i18n'
import type { Locale } from '@/i18n'
import { getSystemInfo, changePasswordApi } from '@/api'
import { ElMessage } from 'element-plus'
import type { FormRules } from 'element-plus'
import { Sunny, Moon, UserFilled, SwitchButton } from '@element-plus/icons-vue'

const route = useRoute()
const router = useRouter()
const userStore = useUserStore()
const { t, locale, setLocale } = useI18n()
const isCollapse = ref(false)
const platformInfo = ref<any>({})
const showChangePassword = ref(false)
const changingPw = ref(false)
const pwRef = ref()

const pwForm = reactive({
  old_password: '',
  new_password: '',
  confirm_password: '',
})

const pwRules = computed<FormRules>(() => ({
  old_password: [{ required: true, message: t('st.passwordRequired'), trigger: 'blur' }],
  new_password: [{ required: true, message: t('st.passwordRequired'), trigger: 'blur' }],
  confirm_password: [{ required: true, message: t('st.passwordRequired'), trigger: 'blur' }],
}))

const menuItems = [
  { path: '/dashboard', titleKey: 'dashboard', icon: 'Odometer' },
  { path: '/sites', titleKey: 'sites', icon: 'Monitor' },
  { path: '/files', titleKey: 'fileManager', icon: 'FolderOpened' },
  { path: '/settings', titleKey: 'settings', icon: 'Setting' },
]

function toggleTheme() {
  userStore.setTheme(userStore.theme === 'dark' ? 'default' : 'dark')
}
function toggleLang() {
  setLocale(locale.value === 'zh' ? 'en' : 'zh' as Locale)
}
function handleLogout() { userStore.logout(); router.push('/login') }
function resetPasswordForm() {
  pwForm.old_password = ''
  pwForm.new_password = ''
  pwForm.confirm_password = ''
  pwRef.value?.clearValidate()
}
function openChangePassword() {
  showChangePassword.value = true
  nextTick(() => pwRef.value?.clearValidate())
}
async function changePassword() {
  if (pwForm.new_password !== pwForm.confirm_password) {
    ElMessage.warning(t('st.passwordMismatch'))
    return
  }
  try {
    await pwRef.value?.validate()
  } catch {
    return
  }
  changingPw.value = true
  try {
    const res: any = await changePasswordApi({
      username: userStore.username,
      old_password: pwForm.old_password,
      new_password: pwForm.new_password,
    })
    if (res.success) {
      ElMessage.success(t('st.passwordChanged'))
      showChangePassword.value = false
      resetPasswordForm()
    } else {
      ElMessage.error(res.message || t('api.requestFailed'))
    }
  } catch (err: any) {
    ElMessage.error(err?.message || t('api.requestFailed'))
  } finally {
    changingPw.value = false
  }
}
function goTo(path: string) {
  if (route.path !== path) router.push(path)
}
function isMenuActive(path: string) {
  if (path === '/sites') return route.path === '/sites' || route.path.startsWith('/sites/')
  return route.path === path
}

function formatUptime(s: number) {
  if (!s) return '-'
  const d = Math.floor(s / 86400)
  const h = Math.floor((s % 86400) / 3600)
  const m = Math.floor((s % 3600) / 60)
  if (d > 0) return `${d}d ${h}h`
  return `${h}h ${m}m`
}

let timer: any
async function fetchPlatform() {
  try {
    const res: any = await getSystemInfo()
    if (res.success) platformInfo.value = res.data.platform
  } catch {}
}
onMounted(() => { fetchPlatform(); timer = setInterval(fetchPlatform, 30000) })
onUnmounted(() => clearInterval(timer))
</script>
<style scoped lang="scss">
.layout { height: 100vh; overflow: hidden; }
.sidebar {
  background: var(--lp-sidebar-bg);
  transition: width var(--lp-transition);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  border-right: 1px solid rgba(255,255,255,0.04);
  .logo {
    height: 56px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    flex-shrink: 0;
  }
  .logo-text {
    color: #fff;
    font-size: 17px;
    font-weight: 700;
    letter-spacing: 0.5px;
    white-space: nowrap;
  }
  :deep(.el-scrollbar) { flex: 1; }
  .sidebar-nav {
    display: flex;
    flex-direction: column;
    padding: 4px 0;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 42px;
    width: calc(100% - 16px);
    padding: 0 20px;
    margin: 3px 8px;
    border: 0;
    border-radius: var(--lp-radius);
    background: transparent;
    color: var(--lp-sidebar-text);
    text-decoration: none;
    text-align: left;
    font-size: 14px;
    transition: all var(--lp-transition);
    cursor: pointer;
    &:hover:not(.active) { background: rgba(255,255,255,0.06); }
    &.active {
      background: var(--lp-sidebar-active-bg);
      color: var(--lp-sidebar-active-text);
      font-weight: 500;
    }
    .el-icon { font-size: 18px; }
  }
  .nav-label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sidebar-footer {
    padding: 12px 16px;
    text-align: center;
    flex-shrink: 0;
    .version { color: rgba(255,255,255,0.25); font-size: 11px; }
  }
}
.header {
  background: var(--lp-header-bg);
  border-bottom: 1px solid var(--lp-border-light);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  height: 48px;
  box-shadow: var(--lp-shadow-sm);
  z-index: 10;
  flex-shrink: 0;
  overflow: hidden;
}
.header-left {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  flex: 1;
  overflow: hidden;
}
.collapse-btn {
  cursor: pointer;
  color: var(--lp-text-secondary);
  flex-shrink: 0;
  transition: color var(--lp-transition);
  &:hover { color: var(--lp-primary); }
}
.platform-badges {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  overflow: hidden;
  flex-shrink: 1;
}
.badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 10px;
  font-size: 12px;
  line-height: 20px;
  border-radius: 10px;
  border: 1px solid var(--lp-border);
  color: var(--lp-text-secondary);
  background: var(--lp-card-bg);
  white-space: nowrap;
  flex-shrink: 0;
  .el-icon { color: var(--lp-primary); }
}
.badge-success {
  color: var(--lp-success);
  border-color: var(--lp-success);
  .el-icon { color: var(--lp-success); }
}
.header-right {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
  margin-left: 12px;
}
.user-info {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  color: var(--lp-text-primary);
  font-size: 13px;
  white-space: nowrap;
}
.user-name {
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.theme-toggle, .lang-toggle {
  border: 1px solid var(--lp-border) !important;
  background: var(--lp-card-bg) !important;
  color: var(--lp-text-secondary) !important;
  &:hover { color: var(--lp-primary) !important; border-color: var(--lp-primary) !important; }
}
.main-content {
  background: var(--lp-bg-page);
  min-height: 0;
  overflow-y: auto;
  padding: 20px;
}
.change-password-dialog :deep(.el-form-item:last-child) {
  margin-bottom: 0;
}
</style>
