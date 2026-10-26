<template>
  <div class="shell" :class="{ 'shell--collapsed': collapsed, 'shell--nav-open': mobileNavOpen }">
    <!-- Backdrop only exists on mobile, where the sidebar overlays content. -->
    <div class="shell__scrim" @click="mobileNavOpen = false"></div>

    <aside class="sidebar">
      <div class="sidebar__brand">
        <svg class="sidebar__mark" viewBox="0 0 32 32" aria-hidden="true">
          <rect width="32" height="32" rx="8" fill="var(--lp-accent)" />
          <path d="M9 8v14h8" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" fill="none" />
          <circle cx="22" cy="11" r="2.6" fill="#fff" />
        </svg>
        <span v-show="!collapsed" class="sidebar__name">LinxPanel</span>
      </div>

      <nav class="sidebar__nav">
        <template v-for="group in navGroups" :key="group.key">
          <div v-show="!collapsed" class="sidebar__group">{{ t(group.titleKey) }}</div>
          <button
            v-for="item in group.items"
            :key="item.path"
            type="button"
            class="nav-item"
            :class="{ 'nav-item--active': isActive(item) }"
            :title="collapsed ? t(item.titleKey) : undefined"
            @click="go(item.path)"
          >
            <el-icon :size="17"><component :is="item.icon" /></el-icon>
            <span v-show="!collapsed" class="nav-item__label">{{ t(item.titleKey) }}</span>
          </button>
        </template>
      </nav>

      <div class="sidebar__foot">
        <span v-show="!collapsed" class="sidebar__version">v{{ panelVersion }}</span>
        <button type="button" class="icon-btn" :title="t('nav.collapse')" @click="collapsed = !collapsed">
          <el-icon :size="16"><Fold v-if="!collapsed" /><Expand v-else /></el-icon>
        </button>
      </div>
    </aside>

    <div class="shell__main">
      <header class="topbar">
        <button type="button" class="icon-btn topbar__menu" @click="mobileNavOpen = !mobileNavOpen">
          <el-icon :size="18"><Expand /></el-icon>
        </button>

        <div class="topbar__host">
          <span class="host-chip host-chip--name">
            <el-icon :size="13"><Monitor /></el-icon>
            {{ host.hostname || '—' }}
          </span>
          <span class="host-chip">{{ host.os }}</span>
          <span class="host-chip host-chip--hide-sm">{{ t('nav.uptime') }} {{ host.uptime }}</span>
        </div>

        <div class="topbar__right">
          <router-link v-if="updateTag" to="/settings?tab=update" class="update-chip" :title="t('upd.available')">
            <el-icon :size="13"><Upload /></el-icon>
            <span class="update-chip__text">{{ t('upd.badge', { tag: updateTag }) }}</span>
          </router-link>
          <!-- Connection state is the one thing that must never be silently wrong. -->
          <span class="live" :class="`live--${realtime.state}`" :title="liveTitle">
            <span class="live__dot"></span>
            <span class="live__text">{{ liveLabel }}</span>
          </span>

          <button type="button" class="icon-btn" :title="t('language')" @click="toggleLang">
            <span class="icon-btn__text">{{ locale === 'zh' ? 'EN' : '中' }}</span>
          </button>
          <button type="button" class="icon-btn" :title="t('nav.theme')" @click="toggleTheme">
            <el-icon :size="16"><Sunny v-if="userStore.theme === 'dark'" /><Moon v-else /></el-icon>
          </button>

          <el-dropdown trigger="click">
            <button type="button" class="user-btn">
              <span class="user-btn__avatar">{{ initials }}</span>
              <span class="user-btn__name">{{ userStore.username }}</span>
              <el-icon :size="12"><ArrowDown /></el-icon>
            </button>
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
      </header>

      <main class="shell__content">
        <router-view v-slot="{ Component }">
          <transition name="fade" mode="out-in">
            <component :is="Component" />
          </transition>
        </router-view>
      </main>
    </div>

    <el-dialog
      v-model="showChangePassword"
      :title="t('st.changePassword')"
      width="440px"
      @closed="resetPasswordForm"
    >
      <el-form ref="pwRef" :model="pwForm" :rules="pwRules" label-position="top" @submit.prevent>
        <el-form-item :label="t('st.oldPassword')" prop="old_password">
          <el-input v-model="pwForm.old_password" type="password" show-password autocomplete="current-password" />
        </el-form-item>
        <el-form-item :label="t('st.newPassword')" prop="new_password">
          <el-input v-model="pwForm.new_password" type="password" show-password autocomplete="new-password" />
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
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import type { FormRules } from 'element-plus'
import {
  ArrowDown, Expand, Fold, Lock, Monitor, Moon, Sunny, SwitchButton,
} from '@element-plus/icons-vue'
import { useUserStore } from '@/stores/user'
import { useRealtimeStore } from '@/stores/realtime'
import { useI18n } from '@/i18n'
import type { Locale } from '@/i18n'
import { changePasswordApi, getUpdateStatus } from '@/api'
import { formatDuration } from '@/utils/format'

const route = useRoute()
const router = useRouter()
const userStore = useUserStore()
const realtime = useRealtimeStore()
const { t, locale, setLocale } = useI18n()

const COLLAPSE_KEY = 'lp.sidebar.collapsed'
const collapsed = ref(localStorage.getItem(COLLAPSE_KEY) === '1')
const mobileNavOpen = ref(false)

watch(collapsed, (value) => {
  try {
    localStorage.setItem(COLLAPSE_KEY, value ? '1' : '0')
  } catch {
    // Private mode / blocked storage: collapsing still works for this session.
  }
})

const navGroups = [
  {
    key: 'monitor',
    titleKey: 'nav.groupMonitor',
    items: [
      { path: '/dashboard', titleKey: 'dashboard', icon: 'Odometer' },
      { path: '/processes', titleKey: 'nav.processes', icon: 'Histogram' },
      { path: '/services', titleKey: 'nav.services', icon: 'SetUp' },
    ],
  },
  {
    key: 'server',
    titleKey: 'nav.groupServer',
    items: [
      { path: '/sites', titleKey: 'sites', icon: 'Connection', match: '/sites' },
      { path: '/appstore', titleKey: 'nav.appstore', icon: 'Shop' },
      { path: '/databases', titleKey: 'nav.databases', icon: 'Coin' },
      { path: '/runtime', titleKey: 'nav.runtime', icon: 'Box' },
      { path: '/files', titleKey: 'fileManager', icon: 'FolderOpened' },
      { path: '/terminal', titleKey: 'terminal', icon: 'Monitor' },
      { path: '/cron', titleKey: 'nav.cron', icon: 'AlarmClock' },
      { path: '/security', titleKey: 'nav.security', icon: 'Lock' },
    ],
  },
  {
    key: 'system',
    titleKey: 'nav.groupSystem',
    items: [{ path: '/settings', titleKey: 'settings', icon: 'Setting' }],
  },
]

const panelVersion = computed(() => realtime.staticInfo?.panel_version || '0.1.0')

// New-release notice, refreshed now and then (the server checks GitHub itself).
const updateTag = ref('')
async function refreshUpdateNotice() {
  try {
    const res: any = await getUpdateStatus()
    const check = res?.success ? res.data?.check : null
    updateTag.value = check?.has_update && res.data?.can_update ? check.latest?.tag || '' : ''
  } catch {
    updateTag.value = ''
  }
}
let updateTimer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  refreshUpdateNotice()
  updateTimer = setInterval(refreshUpdateNotice, 30 * 60 * 1000)
})
onUnmounted(() => clearInterval(updateTimer))

const host = computed(() => {
  const info = realtime.staticInfo
  return {
    hostname: info?.hostname || '',
    os: info ? `${info.os_name} ${info.arch}` : '',
    uptime: realtime.latest ? formatDuration(realtime.latest.uptime) : '—',
  }
})

const liveLabel = computed(() => {
  if (realtime.state === 'open') return t('nav.live')
  if (realtime.state === 'connecting') return t('nav.connecting')
  return t('nav.offline')
})

const liveTitle = computed(() =>
  realtime.lastUpdate ? `${t('nav.lastUpdate')}: ${new Date(realtime.lastUpdate).toLocaleTimeString()}` : liveLabel.value,
)

const initials = computed(() => (userStore.username || '?').slice(0, 1).toUpperCase())

function isActive(item: { path: string; match?: string }) {
  if (item.match) return route.path === item.match || route.path.startsWith(`${item.match}/`)
  return route.path === item.path
}

function go(path: string) {
  mobileNavOpen.value = false
  if (route.path !== path) router.push(path)
}

function toggleTheme() {
  userStore.setTheme(userStore.theme === 'dark' ? 'default' : 'dark')
}

function toggleLang() {
  setLocale(locale.value === 'zh' ? 'en' : ('zh' as Locale))
}

function handleLogout() {
  realtime.reset()
  userStore.logout()
  router.push('/login')
}

// --- Change password ---
const showChangePassword = ref(false)
const changingPw = ref(false)
const pwRef = ref()
const pwForm = reactive({ old_password: '', new_password: '', confirm_password: '' })
const pwRules = computed<FormRules>(() => ({
  old_password: [{ required: true, message: t('st.passwordRequired'), trigger: 'blur' }],
  new_password: [
    { required: true, message: t('st.passwordRequired'), trigger: 'blur' },
    { min: 8, message: t('st.passwordTooShort'), trigger: 'blur' },
  ],
  confirm_password: [{ required: true, message: t('st.passwordRequired'), trigger: 'blur' }],
}))

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
      // Other sessions were revoked; the response carries this session's new token.
      if (res.data?.token) userStore.setLogin(res.data.token, userStore.username)
      ElMessage.success(t('st.sessionsRevoked'))
      showChangePassword.value = false
      resetPasswordForm()
    }
  } catch (err: any) {
    ElMessage.error(err?.message || t('api.requestFailed'))
  } finally {
    changingPw.value = false
  }
}

// The shell keeps one subscription open so the header stays live on every page.
onMounted(() => realtime.subscribe())
onUnmounted(() => realtime.unsubscribe())
</script>

<style scoped lang="scss">
.shell {
  display: flex;
  height: 100vh;
  overflow: hidden;
  background: var(--lp-canvas);
}

.shell__scrim {
  display: none;
  position: fixed;
  inset: 0;
  z-index: 19;
  background: rgba(16, 24, 40, 0.45);
}

/* ---------- Sidebar ---------- */
.sidebar {
  display: flex;
  flex-direction: column;
  width: var(--lp-sidebar-w);
  flex: none;
  background: var(--lp-nav);
  border-right: 1px solid var(--lp-line);
  transition: width var(--lp-base);
  z-index: 20;
}
.shell--collapsed .sidebar { width: var(--lp-sidebar-w-collapsed); }

.sidebar__brand {
  display: flex;
  align-items: center;
  gap: var(--lp-2);
  height: var(--lp-header-h);
  padding: 0 var(--lp-4);
  flex: none;
  border-bottom: 1px solid var(--lp-line-soft);
}
.sidebar__mark { width: 26px; height: 26px; flex: none; }
.sidebar__name {
  font-size: 15px;
  font-weight: 650;
  letter-spacing: -0.01em;
  color: var(--lp-text);
  white-space: nowrap;
}

.sidebar__nav {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--lp-3) var(--lp-2);
}

.sidebar__group {
  padding: var(--lp-3) var(--lp-2) var(--lp-1);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--lp-text-faint);
}

.nav-item {
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  width: 100%;
  padding: 8px 10px;
  margin-bottom: 2px;
  border: 0;
  border-radius: var(--lp-r-sm);
  background: transparent;
  color: var(--lp-nav-text);
  font-family: inherit;
  font-size: var(--lp-text-base);
  font-weight: 500;
  text-align: left;
  cursor: pointer;
  white-space: nowrap;
  transition: background var(--lp-fast), color var(--lp-fast);

  &:hover { background: var(--lp-surface-hover); color: var(--lp-text); }
  &--active {
    background: var(--lp-nav-active-bg);
    color: var(--lp-nav-text-active);
    font-weight: 600;
  }
}
.shell--collapsed .nav-item { justify-content: center; padding: 9px 0; }

.sidebar__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--lp-2);
  padding: var(--lp-2) var(--lp-3);
  flex: none;
  border-top: 1px solid var(--lp-line-soft);
}
.shell--collapsed .sidebar__foot { justify-content: center; }
.update-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 26px;
  padding: 0 10px;
  border-radius: var(--lp-r-full);
  background: var(--lp-accent-soft);
  color: var(--lp-accent);
  font-size: var(--lp-text-xs);
  font-weight: 600;
  text-decoration: none;
  white-space: nowrap;
}
.update-chip:hover { filter: brightness(0.96); }
@media (max-width: 640px) {
  .update-chip__text { display: none; }
}
.sidebar__version { font-size: var(--lp-text-xs); color: var(--lp-text-faint); }

/* ---------- Top bar ---------- */
.shell__main {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
}

.topbar {
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  height: var(--lp-header-h);
  flex: none;
  padding: 0 var(--lp-4);
  background: var(--lp-surface);
  border-bottom: 1px solid var(--lp-line);
}

.topbar__menu { display: none; }

.topbar__host {
  display: flex;
  align-items: center;
  gap: var(--lp-2);
  min-width: 0;
  overflow: hidden;
}

.host-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 3px 9px;
  border-radius: var(--lp-r-full);
  background: var(--lp-surface-sunken);
  font-size: var(--lp-text-xs);
  color: var(--lp-text-soft);
  white-space: nowrap;

  &--name { font-weight: 600; color: var(--lp-text); }
}

.topbar__right {
  display: flex;
  align-items: center;
  gap: var(--lp-2);
  margin-left: auto;
}

.live {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 3px 10px;
  border-radius: var(--lp-r-full);
  font-size: var(--lp-text-xs);
  font-weight: 600;
  background: var(--lp-muted-soft);
  color: var(--lp-muted);
}
.live__dot { width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
.live--open { background: var(--lp-ok-soft); color: var(--lp-ok); }
.live--open .live__dot { animation: live-pulse 2s ease-in-out infinite; }
.live--connecting { background: var(--lp-warn-soft); color: var(--lp-warn); }
.live--closed { background: var(--lp-danger-soft); color: var(--lp-danger); }

@keyframes live-pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.45; transform: scale(0.82); }
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border: 0;
  border-radius: var(--lp-r-sm);
  background: transparent;
  color: var(--lp-text-soft);
  cursor: pointer;
  transition: background var(--lp-fast), color var(--lp-fast);

  &:hover { background: var(--lp-surface-hover); color: var(--lp-text); }
}
.icon-btn__text { font-size: 11px; font-weight: 700; }

.user-btn {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 8px 0 4px;
  border: 0;
  border-radius: var(--lp-r-full);
  background: transparent;
  color: var(--lp-text);
  font-family: inherit;
  font-size: var(--lp-text-base);
  cursor: pointer;
  transition: background var(--lp-fast);

  &:hover { background: var(--lp-surface-hover); }
}
.user-btn__avatar {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: var(--lp-accent);
  color: #fff;
  font-size: 11px;
  font-weight: 700;
}
[data-theme='dark'] .user-btn__avatar { color: var(--lp-text-invert); }
.user-btn__name { font-weight: 500; }

.shell__content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

/* ---------- Responsive ---------- */
@media (max-width: 900px) {
  .host-chip--hide-sm { display: none; }
}

@media (max-width: 768px) {
  .sidebar {
    position: fixed;
    inset: 0 auto 0 0;
    transform: translateX(-100%);
    box-shadow: var(--lp-shadow-lg);
  }
  .shell--nav-open .sidebar { transform: translateX(0); }
  .shell--nav-open .shell__scrim { display: block; }
  .topbar__menu { display: inline-flex; }
  .user-btn__name { display: none; }
  .live__text { display: none; }
  .live { padding: 3px 7px; }
}
</style>
