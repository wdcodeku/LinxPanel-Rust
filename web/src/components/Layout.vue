<template>
  <div class="shell" :class="{ 'shell--collapsed': collapsed, 'shell--nav-open': mobileNavOpen }">
    <!-- Backdrop only exists on mobile, where the sidebar overlays content. -->
    <div class="shell__scrim" @click="mobileNavOpen = false"></div>

    <aside class="sidebar">
      <div class="sidebar__brand">
        <svg class="sidebar__mark" viewBox="0 0 32 32" aria-hidden="true">
          <defs>
            <linearGradient id="lp-mark" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stop-color="var(--lp-accent-400)" />
              <stop offset="1" stop-color="var(--lp-accent-600)" />
            </linearGradient>
          </defs>
          <rect width="32" height="32" rx="9" fill="url(#lp-mark)" />
          <path d="M9 8v14h8" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" fill="none" />
          <circle cx="22" cy="11" r="2.6" fill="#fff" />
        </svg>
        <span class="sidebar__name">LinxPanel</span>
      </div>

      <nav class="sidebar__nav">
        <template v-for="group in navGroups" :key="group.key">
          <div class="sidebar__group">{{ t(group.titleKey) }}</div>
          <el-tooltip
            v-for="item in group.items"
            :key="item.path"
            :content="t(item.titleKey)"
            placement="right"
            :disabled="!collapsed"
            :show-after="200"
          >
            <a
              :href="item.path"
              class="nav-item"
              :class="{ 'nav-item--active': isActive(item) }"
              @click.prevent="go(item.path)"
            >
              <el-icon :size="18" class="nav-item__icon"><component :is="item.icon" /></el-icon>
              <span class="nav-item__label">{{ t(item.titleKey) }}</span>
            </a>
          </el-tooltip>
        </template>
      </nav>

      <div class="sidebar__foot">
        <div class="host-card" :title="`${host.hostname} · ${host.os}`">
          <span class="host-card__icon"><el-icon :size="16"><Monitor /></el-icon></span>
          <span class="host-card__text">
            <span class="host-card__name lp-truncate">{{ host.hostname || '—' }}</span>
            <span class="host-card__meta lp-truncate">{{ host.os }} · v{{ panelVersion }}</span>
          </span>
        </div>
        <button type="button" class="icon-btn sidebar__collapse" :title="t('nav.collapse')" @click="collapsed = !collapsed">
          <el-icon :size="16"><Fold v-if="!collapsed" /><Expand v-else /></el-icon>
        </button>
      </div>
    </aside>

    <div class="shell__main">
      <header class="topbar">
        <button type="button" class="icon-btn topbar__menu" @click="mobileNavOpen = !mobileNavOpen">
          <el-icon :size="18"><Expand /></el-icon>
        </button>

        <nav class="crumbs" aria-label="breadcrumb">
          <span v-if="crumbs.group" class="crumbs__group">{{ crumbs.group }}</span>
          <el-icon v-if="crumbs.group" :size="12" class="crumbs__sep"><ArrowRight /></el-icon>
          <router-link v-if="crumbs.child" :to="crumbs.parentPath" class="crumbs__link">{{ crumbs.title }}</router-link>
          <span v-else class="crumbs__current">{{ crumbs.title }}</span>
          <template v-if="crumbs.child">
            <el-icon :size="12" class="crumbs__sep"><ArrowRight /></el-icon>
            <span class="crumbs__current">{{ crumbs.child }}</span>
          </template>
        </nav>

        <button type="button" class="search-btn" @click="paletteOpen = true">
          <el-icon :size="15"><Search /></el-icon>
          <span class="search-btn__text">{{ t('nav.search') }}</span>
          <kbd class="search-btn__kbd">{{ isMac ? '⌘' : 'Ctrl' }} K</kbd>
        </button>

        <div class="topbar__right">
          <button
            v-if="taskStore.running.length"
            type="button"
            class="chip chip--accent"
            @click="taskStore.show(taskStore.running[0].id)"
          >
            <el-icon :size="13" class="spin"><Loading /></el-icon>
            <span class="chip__text">{{ t('task.runningCount', { n: taskStore.running.length }) }}</span>
          </button>
          <router-link v-if="updateTag" to="/settings?tab=update" class="chip chip--accent" :title="t('upd.available')">
            <el-icon :size="13"><Upload /></el-icon>
            <span class="chip__text">{{ t('upd.badge', { tag: updateTag }) }}</span>
          </router-link>
          <!-- Connection state is the one thing that must never be silently wrong. -->
          <span class="live" :class="`live--${realtime.state}`" :title="liveTitle">
            <span class="live__dot"></span>
            <span class="live__text">{{ liveLabel }}</span>
          </span>

          <span class="topbar__divider"></span>

          <button type="button" class="icon-btn" :title="t('language')" @click="toggleLang">
            <span class="icon-btn__text">{{ locale === 'zh' ? 'EN' : '中' }}</span>
          </button>
          <button type="button" class="icon-btn" :title="t('nav.theme')" @click="toggleTheme">
            <el-icon :size="16"><Sunny v-if="userStore.theme === 'dark'" /><Moon v-else /></el-icon>
          </button>

          <el-dropdown trigger="click" placement="bottom-end">
            <button type="button" class="user-btn">
              <span class="user-btn__avatar">{{ initials }}</span>
              <span class="user-btn__name">{{ userStore.username }}</span>
              <el-icon :size="12" class="user-btn__caret"><ArrowDown /></el-icon>
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

      <TaskCenter />
      <main ref="contentRef" class="shell__content">
        <!-- Visited pages stay alive: switching back shows them at once, as
             they were, and they refresh quietly behind that (see usePage).
             The swap itself is immediate - no fade from a faded frame. -->
        <router-view v-slot="{ Component, route: viewRoute }">
          <keep-alive :exclude="['SiteDetail']" :max="12">
            <component :is="Component" :key="viewRoute.path" />
          </keep-alive>
        </router-view>
      </main>
    </div>

    <CommandPalette v-model="paletteOpen" />

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
  ArrowDown, ArrowRight, Expand, Fold, Lock, Monitor, Moon, Search, Sunny, SwitchButton,
} from '@element-plus/icons-vue'
import { useUserStore } from '@/stores/user'
import { useRealtimeStore } from '@/stores/realtime'
import { useI18n } from '@/i18n'
import type { Locale } from '@/i18n'
import { changePasswordApi, getUpdateStatus } from '@/api'
import { prefetchViews } from '@/router'
import TaskCenter from '@/components/TaskCenter.vue'
import CommandPalette from '@/components/CommandPalette.vue'
import { findNav, navGroups } from '@/components/nav'
import type { NavItem } from '@/components/nav'
import { useTaskStore } from '@/stores/tasks'
import { formatDuration } from '@/utils/format'

const route = useRoute()
const router = useRouter()
const userStore = useUserStore()
const taskStore = useTaskStore()
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
  prefetchViews()
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

// The highlight moves on click, before the next page has rendered: the
// sidebar answers at once even when that page is still loading.
const pendingPath = ref('')
watch(() => route.path, () => {
  pendingPath.value = ''
})

function isActive(item: NavItem) {
  const path = pendingPath.value || route.path
  if (item.match) return path === item.match || path.startsWith(`${item.match}/`)
  return path === item.path
}

function go(path: string) {
  mobileNavOpen.value = false
  if (route.path === path) return
  pendingPath.value = path
  router.push(path).finally(() => {
    pendingPath.value = ''
  })
}

const crumbs = computed(() => {
  const found = findNav(route.path)
  const title = found ? t(found.item.titleKey) : ''
  const child = found && found.item.path !== route.path ? t('nav.siteDetail') : ''
  return {
    group: found ? t(found.group.titleKey) : '',
    title,
    child,
    parentPath: found?.item.path || '/',
  }
})

// --- Quick search (Ctrl/Cmd + K, or "/" outside text fields) ---
const paletteOpen = ref(false)
const isMac = /Mac|iPhone|iPad/.test(navigator.platform)
function onShortcut(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null
  const typing = !!target && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))
  if ((event.key === 'k' || event.key === 'K') && (event.ctrlKey || event.metaKey)) {
    event.preventDefault()
    paletteOpen.value = !paletteOpen.value
  } else if (event.key === '/' && !typing && !paletteOpen.value) {
    event.preventDefault()
    paletteOpen.value = true
  }
}
onMounted(() => window.addEventListener('keydown', onShortcut))
onUnmounted(() => window.removeEventListener('keydown', onShortcut))

// Each page keeps its own scroll position; a page seen for the first time
// starts at the top.
const contentRef = ref<HTMLElement>()
const scrollByPath = new Map<string, number>()
const removeScrollGuard = router.beforeEach((_to, from) => {
  if (contentRef.value) scrollByPath.set(from.path, contentRef.value.scrollTop)
})
onUnmounted(removeScrollGuard)
watch(() => route.path, async (path) => {
  await nextTick()
  if (contentRef.value) contentRef.value.scrollTop = scrollByPath.get(path) || 0
})

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
  height: 100dvh;
  overflow: hidden;
  background: var(--lp-canvas);
}

.shell__scrim {
  display: none;
  position: fixed;
  inset: 0;
  z-index: 19;
  background: var(--lp-scrim);
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
  overflow: hidden;
}
.shell--collapsed .sidebar { width: var(--lp-sidebar-w-collapsed); }

.sidebar__brand {
  display: flex;
  align-items: center;
  gap: 10px;
  height: var(--lp-header-h);
  padding: 0 18px;
  flex: none;
}
.sidebar__mark { width: 28px; height: 28px; flex: none; }
.sidebar__name {
  font-size: 16px;
  font-weight: 700;
  letter-spacing: -0.02em;
  color: var(--lp-text);
  white-space: nowrap;
}

.sidebar__nav {
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  padding: var(--lp-1) var(--lp-3) var(--lp-3);
}

.sidebar__group {
  height: 30px;
  padding: 12px 10px 0;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  color: var(--lp-text-faint);
  white-space: nowrap;
}

.nav-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  height: 38px;
  padding: 0 11px;
  margin-bottom: 2px;
  border-radius: var(--lp-r);
  color: var(--lp-nav-text);
  font-size: var(--lp-text-md);
  font-weight: 500;
  text-decoration: none;
  white-space: nowrap;
  cursor: pointer;
  transition: background var(--lp-fast), color var(--lp-fast);

  &:hover { background: var(--lp-surface-hover); color: var(--lp-text); }
  &:focus-visible { outline-offset: -2px; }

  &--active,
  &--active:hover {
    background: var(--lp-nav-active-bg);
    color: var(--lp-nav-text-active);
    font-weight: 600;
  }
  &--active::before {
    content: '';
    position: absolute;
    left: -12px;
    top: 9px;
    bottom: 9px;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: var(--lp-accent);
  }
}
.nav-item__icon { flex: none; }
.nav-item__label { overflow: hidden; text-overflow: ellipsis; }

.sidebar__foot {
  display: flex;
  align-items: center;
  gap: var(--lp-2);
  padding: var(--lp-3);
  flex: none;
  border-top: 1px solid var(--lp-line-soft);
}

.host-card {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 1;
  min-width: 0;
}
.host-card__icon {
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  flex: none;
  border-radius: var(--lp-r);
  background: var(--lp-surface-sunken);
  color: var(--lp-text-soft);
}
.host-card__text { display: flex; flex-direction: column; min-width: 0; line-height: 1.3; }
.host-card__name { font-size: var(--lp-text-base); font-weight: 600; color: var(--lp-text); }
.host-card__meta { font-size: var(--lp-text-xs); color: var(--lp-text-dim); }

/* Collapsed: icons only, centred. */
.shell--collapsed {
  .sidebar__brand { padding: 0; justify-content: center; }
  .sidebar__name,
  .nav-item__label,
  .host-card { display: none; }
  .sidebar__group {
    height: 17px;
    padding: 0;
    margin: 8px 10px;
    font-size: 0;
    border-top: 1px solid var(--lp-line-soft);
  }
  .sidebar__group:first-child { height: 0; margin-top: 0; border: 0; }
  .nav-item { justify-content: center; padding: 0; }
  .nav-item--active::before { left: -12px; }
  .sidebar__foot { justify-content: center; }
}

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
  padding: 0 var(--lp-5);
  background: var(--lp-surface);
  border-bottom: 1px solid var(--lp-line);
}



.crumbs {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-size: var(--lp-text-md);
  white-space: nowrap;
}
.crumbs__group { color: var(--lp-text-dim); }
.crumbs__sep { color: var(--lp-text-faint); flex: none; }
.crumbs__link { color: var(--lp-text-dim); text-decoration: none; }
.crumbs__link:hover { color: var(--lp-accent); }
.crumbs__current { font-weight: 600; color: var(--lp-text); overflow: hidden; text-overflow: ellipsis; }

.search-btn {
  display: flex;
  align-items: center;
  gap: var(--lp-2);
  width: 260px;
  height: 34px;
  margin-left: var(--lp-6);
  padding: 0 6px 0 12px;
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r);
  background: var(--lp-surface-sunken);
  color: var(--lp-text-dim);
  font: inherit;
  font-size: var(--lp-text-base);
  cursor: pointer;
  transition: border-color var(--lp-fast), background var(--lp-fast);

  &:hover { border-color: var(--lp-line-strong); background: var(--lp-surface); }
}
.search-btn__text { flex: 1; text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.search-btn__kbd {
  padding: 1px 6px;
  border: 1px solid var(--lp-line);
  border-radius: 5px;
  background: var(--lp-surface);
  font-family: var(--lp-font);
  font-size: 11px;
  color: var(--lp-text-dim);
  white-space: nowrap;
}

.topbar__right {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
}
.topbar__divider { width: 1px; height: 20px; margin: 0 4px; background: var(--lp-line); }

.chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 28px;
  padding: 0 11px;
  border: 0;
  border-radius: var(--lp-r-full);
  font: inherit;
  font-size: var(--lp-text-sm);
  font-weight: 600;
  text-decoration: none;
  white-space: nowrap;
  cursor: pointer;

  &--accent { background: var(--lp-accent-soft); color: var(--lp-accent); }
  &:hover { filter: brightness(0.97); }
}
.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }

.live {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 11px;
  border-radius: var(--lp-r-full);
  font-size: var(--lp-text-sm);
  font-weight: 600;
  background: var(--lp-muted-soft);
  color: var(--lp-muted);
}
.live__dot { width: 7px; height: 7px; border-radius: 50%; background: currentColor; }
.live--open { background: var(--lp-ok-soft); color: var(--lp-ok); }
.live--open .live__dot { animation: live-pulse 2s ease-in-out infinite; }
.live--connecting { background: var(--lp-warn-soft); color: var(--lp-warn); }
.live--closed { background: var(--lp-danger-soft); color: var(--lp-danger); }

@keyframes live-pulse {
  0%, 100% { box-shadow: 0 0 0 0 currentColor; }
  60% { box-shadow: 0 0 0 4px transparent; }
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  flex: none;
  border: 0;
  border-radius: var(--lp-r);
  background: transparent;
  color: var(--lp-text-soft);
  cursor: pointer;
  transition: background var(--lp-fast), color var(--lp-fast);

  &:hover { background: var(--lp-surface-hover); color: var(--lp-text); }
}
.icon-btn__text { font-size: 12px; font-weight: 700; }

.user-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 36px;
  padding: 0 8px 0 4px;
  margin-left: 2px;
  border: 0;
  border-radius: var(--lp-r-full);
  background: transparent;
  color: var(--lp-text);
  font: inherit;
  font-size: var(--lp-text-base);
  cursor: pointer;
  transition: background var(--lp-fast);

  &:hover { background: var(--lp-surface-hover); }
}
.user-btn__avatar {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background: linear-gradient(135deg, var(--lp-accent-400), var(--lp-accent-600));
  color: #fff;
  font-size: 12px;
  font-weight: 700;
}
.user-btn__name { font-weight: 600; }
.user-btn__caret { color: var(--lp-text-dim); }

.shell__content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  /* The scrollbar's space is always reserved: pages of different heights
     must not shift sideways when switching between them. */
  scrollbar-gutter: stable;
}

/* After .icon-btn, which would otherwise show it on desktop. */
.topbar__menu { display: none; }

/* ---------- Responsive ---------- */
@media (max-width: 1100px) {
  .search-btn { width: 200px; margin-left: var(--lp-3); }
}

@media (max-width: 900px) {
  .search-btn { width: 34px; padding: 0; justify-content: center; margin-left: auto; }
  .search-btn__text,
  .search-btn__kbd { display: none; }
  .topbar__right { margin-left: 0; }
  .crumbs__group,
  .crumbs__group + .crumbs__sep { display: none; }
}

@media (max-width: 768px) {
  .topbar { padding: 0 var(--lp-3); gap: var(--lp-2); }
  .sidebar {
    position: fixed;
    inset: 0 auto 0 0;
    width: 248px;
    transform: translateX(-100%);
    transition: transform var(--lp-base);
    box-shadow: var(--lp-shadow-lg);
  }
  .shell--collapsed .sidebar { width: 248px; }
  .shell--collapsed {
    .sidebar__brand { padding: 0 18px; justify-content: flex-start; }
    .sidebar__name,
    .nav-item__label { display: inline; }
    .host-card { display: flex; }
    .sidebar__group { height: 30px; padding: 12px 10px 0; margin: 0; font-size: 11px; border: 0; }
    .nav-item { justify-content: flex-start; padding: 0 11px; }
  }
  .sidebar__collapse { display: none; }
  .shell--nav-open .sidebar { transform: translateX(0); }
  .shell--nav-open .shell__scrim { display: block; }
  .topbar__menu { display: inline-flex; }
  .topbar__divider,
  .user-btn__name,
  .user-btn__caret,
  .chip__text,
  .live__text { display: none; }
  .chip,
  .live { padding: 0 9px; }
}
</style>
