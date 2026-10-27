<template>
  <div class="term-page">
    <div class="term-tabs">
      <div class="term-tabs__list">
        <button
          v-for="(session, index) in sessions"
          :key="session.id"
          type="button"
          class="term-tab"
          :class="{ 'term-tab--active': session.id === activeId }"
          @click="activate(session.id)"
        >
          <span class="term-tab__dot" :class="{ 'is-on': session.connected }"></span>
          <span class="term-tab__label">{{ session.title || `${t('terminal')} ${index + 1}` }}</span>
          <span
            v-if="sessions.length > 1"
            class="term-tab__close"
            role="button"
            :aria-label="t('close')"
            @click.stop="close(session.id)"
          >×</span>
        </button>
      </div>

      <div class="term-tabs__actions">
        <el-input
          v-model="cwdInput"
          size="small"
          class="cwd-input lp-mono"
          :placeholder="t('tm.cwdPlaceholder')"
          @keyup.enter="applyCwd"
        >
          <template #append>
            <el-button size="small" @click="applyCwd">{{ t('tm.go') }}</el-button>
          </template>
        </el-input>
        <el-button type="primary" size="small" :icon="Plus" @click="add">{{ t('tm.newTab') }}</el-button>
      </div>
    </div>

    <!--
      Every session stays mounted so its socket and scrollback survive tab switches;
      only the active one is visible.
    -->
    <div class="term-stage">
      <div
        v-for="session in sessions"
        v-show="session.id === activeId"
        :key="session.id"
        class="term-slot"
      >
        <TerminalPane
          :ref="(el: any) => setPaneRef(session.id, el)"
          :cwd="session.cwd"
          flat
          @connected="session.connected = true"
          @disconnected="session.connected = false"
          @update:cwd="(path: string) => onCwd(session, path)"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { nextTick, reactive, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { usePage } from '@/utils/page'
import { Plus } from '@element-plus/icons-vue'
import TerminalPane from '@/components/TerminalPane.vue'
import { useI18n } from '@/i18n'

type Session = {
  id: number
  title: string
  cwd: string
  connected: boolean
}

const { t } = useI18n()

// Other pages open a terminal in a directory with /terminal?cwd=/path.
const route = useRoute()
const requestedCwd = () => (typeof route.query.cwd === 'string' ? route.query.cwd.trim() : '')
let handledRequest = route.fullPath

let nextId = 1
const sessions = reactive<Session[]>([{ id: nextId++, title: '', cwd: requestedCwd(), connected: false }])
const activeId = ref(sessions[0].id)
const cwdInput = ref('')

const panes = new Map<number, any>()

function setPaneRef(id: number, el: any) {
  if (el) panes.set(id, el)
  else panes.delete(id)
}

function add() {
  const session: Session = { id: nextId++, title: '', cwd: '', connected: false }
  sessions.push(session)
  activate(session.id)
}

usePage(() => activate(activeId.value))

// Back on the (kept-alive) page with a new directory: open it in a new tab.
watch(
  () => route.fullPath,
  (path) => {
    if (!route.path.startsWith('/terminal')) return
    const cwd = requestedCwd()
    if (!cwd || path === handledRequest) return
    handledRequest = path
    const session: Session = { id: nextId++, title: '', cwd, connected: false }
    sessions.push(session)
    activate(session.id)
  },
)

function activate(id: number) {
  activeId.value = id
  const session = sessions.find((item) => item.id === id)
  cwdInput.value = session?.cwd || ''
  // The pane was hidden, so its size is stale until it is shown again.
  nextTick(() => {
    const pane = panes.get(id)
    pane?.refit?.()
    pane?.focus?.()
  })
}

function close(id: number) {
  const index = sessions.findIndex((item) => item.id === id)
  if (index < 0 || sessions.length === 1) return
  panes.get(id)?.disconnect?.()
  panes.delete(id)
  sessions.splice(index, 1)
  if (activeId.value === id) {
    activate(sessions[Math.max(0, index - 1)].id)
  }
}

function onCwd(session: Session, path: string) {
  session.cwd = path
  // Show the last path segment as the tab label, like a shell prompt would.
  const segments = path.split('/').filter(Boolean)
  session.title = segments.length ? segments[segments.length - 1] : '/'
  if (session.id === activeId.value) cwdInput.value = path
}

function applyCwd() {
  const path = cwdInput.value.trim()
  if (!path) return
  panes.get(activeId.value)?.chdir?.(path)
}

watch(activeId, () => {
  const session = sessions.find((item) => item.id === activeId.value)
  cwdInput.value = session?.cwd || ''
})
</script>

<style scoped lang="scss">
.term-page {
  display: flex;
  flex-direction: column;
  /* Fill the shell's content area exactly; the terminal scrolls internally. */
  height: 100%;
  min-height: 0;
  padding: var(--lp-4);
  gap: var(--lp-3);
}

.term-tabs {
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  flex: none;
  flex-wrap: wrap;
}

.term-tabs__list {
  display: flex;
  align-items: center;
  gap: var(--lp-1);
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  scrollbar-width: none;
}
.term-tabs__list::-webkit-scrollbar { display: none; }

.term-tab {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 10px;
  flex: none;
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-sm);
  background: var(--lp-surface);
  color: var(--lp-text-soft);
  font-family: inherit;
  font-size: var(--lp-text-base);
  cursor: pointer;
  transition: all var(--lp-fast);

  &:hover { border-color: var(--lp-line-strong); color: var(--lp-text); }
  &--active {
    border-color: var(--lp-accent);
    background: var(--lp-accent-soft);
    color: var(--lp-accent);
    font-weight: 600;
  }
}

.term-tab__dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--lp-line-strong);
  flex: none;
  &.is-on { background: var(--lp-ok); }
}

.term-tab__label { max-width: 140px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.term-tab__close {
  margin-left: 2px;
  width: 16px;
  height: 16px;
  line-height: 14px;
  text-align: center;
  border-radius: 4px;
  font-size: 15px;
  color: var(--lp-text-dim);

  &:hover { background: var(--lp-danger-soft); color: var(--lp-danger); }
}

.term-tabs__actions {
  display: flex;
  align-items: center;
  gap: var(--lp-2);
  flex: none;
}
.cwd-input { width: 300px; }

.term-stage {
  flex: 1;
  min-height: 0;
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  overflow: hidden;
  box-shadow: var(--lp-shadow-sm);
}
.term-slot { height: 100%; }

@media (max-width: 768px) {
  .cwd-input { width: 100%; }
  .term-tabs__actions { width: 100%; }
}
</style>
