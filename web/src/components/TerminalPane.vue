<template>
  <div class="term-pane" :class="{ 'term-pane--flat': flat }">
    <div v-if="showBar" class="term-bar">
      <span class="lp-pill" :class="connected ? 'lp-pill--ok' : 'lp-pill--danger'">
        <span class="lp-pill__dot"></span>{{ connected ? t('tm.connected') : t('tm.disconnected') }}
      </span>
      <span class="term-cwd lp-mono lp-truncate" :title="cwd">{{ cwd || '~' }}</span>
      <div class="term-bar__actions">
        <slot name="actions" />
        <el-tooltip :content="t('tm.clear')" :show-after="400">
          <button type="button" class="term-btn" @click="clear"><el-icon :size="14"><Delete /></el-icon></button>
        </el-tooltip>
        <el-tooltip :content="connected ? t('tm.disconnect') : t('tm.reconnect')" :show-after="400">
          <button type="button" class="term-btn" @click="connected ? disconnect() : connect()">
            <el-icon :size="14"><component :is="connected ? 'SwitchButton' : 'RefreshRight'" /></el-icon>
          </button>
        </el-tooltip>
      </div>
    </div>
    <div ref="host" class="term-host"></div>
  </div>
</template>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Terminal } from 'xterm'
import { FitAddon } from '@xterm/addon-fit'
import { Delete } from '@element-plus/icons-vue'
import { useUserStore } from '@/stores/user'
import { useI18n } from '@/i18n'
import 'xterm/css/xterm.css'

const props = withDefaults(
  defineProps<{
    /** Starting working directory. */
    cwd?: string
    /** Hide the status bar when the host provides its own chrome. */
    showBar?: boolean
    /** Drop the border/radius when embedded in an existing frame. */
    flat?: boolean
    /** Connect as soon as the component mounts. */
    autoConnect?: boolean
  }>(),
  { cwd: '', showBar: true, flat: false, autoConnect: true },
)

const emit = defineEmits<{
  (event: 'update:cwd', path: string): void
  (event: 'connected'): void
  (event: 'disconnected'): void
}>()

const { t } = useI18n()
const userStore = useUserStore()

const host = ref<HTMLElement | null>(null)
const connected = ref(false)
const cwd = ref(props.cwd)

let term: Terminal | null = null
let fit: FitAddon | null = null
let socket: WebSocket | null = null
let observer: ResizeObserver | null = null
let reconnectTimer: number | null = null
/** Set when the user disconnects on purpose, so we do not fight them by reconnecting. */
let manualClose = false
let attempts = 0

const LIGHT_THEME = {
  background: '#ffffff',
  foreground: '#24292f',
  cursor: '#24292f',
  selectionBackground: 'rgba(79, 99, 232, 0.25)',
}
const DARK_THEME = {
  background: '#0f1218',
  foreground: '#d5dae3',
  cursor: '#7b8bf0',
  selectionBackground: 'rgba(123, 139, 240, 0.3)',
}

function themeForMode() {
  return userStore.theme === 'dark' ? DARK_THEME : LIGHT_THEME
}

function buildUrl(path: string) {
  const url = new URL('/api/files/terminal/ws', window.location.origin)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  if (path) url.searchParams.set('cwd', path)
  const token = localStorage.getItem('token')
  if (token) url.searchParams.set('token', token)
  return url.toString()
}

function ensureTerminal() {
  if (term || !host.value) return
  term = new Terminal({
    fontSize: 13,
    fontFamily: 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace',
    lineHeight: 1.25,
    cursorBlink: true,
    convertEol: true,
    scrollback: 5000,
    theme: themeForMode(),
  })
  fit = new FitAddon()
  term.loadAddon(fit)
  term.open(host.value)
  // Keystrokes go over the wire as raw text; only resize/chdir are JSON control frames.
  term.onData((data) => {
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(data)
    }
  })

  observer = new ResizeObserver(() => refit())
  observer.observe(host.value)
  refit()
}

function refit() {
  if (!fit || !term) return
  try {
    fit.fit()
  } catch {
    // The host can be zero-sized while hidden; the next resize retries.
    return
  }
  if (socket?.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }))
  }
}

function connect(path?: string) {
  if (socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) {
    return
  }
  ensureTerminal()
  manualClose = false
  const target = path ?? cwd.value

  const ws = new WebSocket(buildUrl(target))
  ws.binaryType = 'arraybuffer'
  socket = ws

  ws.onopen = () => {
    if (socket !== ws) return
    connected.value = true
    attempts = 0
    emit('connected')
    refit()
    if (target) ws.send(JSON.stringify({ type: 'chdir', path: target }))
    term?.focus()
  }

  ws.onmessage = async (event) => {
    if (!term) return
    if (typeof event.data === 'string') {
      // Control frames arrive as JSON; anything else is terminal output.
      try {
        const payload = JSON.parse(event.data) as { type?: string; path?: string }
        if (payload?.type === 'cwd' && typeof payload.path === 'string') {
          cwd.value = payload.path
          emit('update:cwd', payload.path)
          return
        }
      } catch {
        // Not JSON: fall through and print it.
      }
      term.write(event.data)
      return
    }
    if (event.data instanceof ArrayBuffer) {
      term.write(new Uint8Array(event.data))
      return
    }
    if (event.data instanceof Blob) {
      term.write(new Uint8Array(await event.data.arrayBuffer()))
    }
  }

  ws.onerror = () => {
    if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) ws.close()
  }

  ws.onclose = () => {
    if (socket !== ws) return
    socket = null
    connected.value = false
    emit('disconnected')
    if (!manualClose) scheduleReconnect()
  }
}

function scheduleReconnect() {
  if (reconnectTimer !== null) return
  const delay = Math.min(1000 * 2 ** attempts, 15000)
  attempts += 1
  reconnectTimer = window.setTimeout(() => {
    reconnectTimer = null
    connect()
  }, delay)
}

function disconnect() {
  manualClose = true
  if (reconnectTimer !== null) {
    window.clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
  const ws = socket
  socket = null
  connected.value = false
  ws?.close()
}

function clear() {
  term?.clear()
}

function chdir(path: string) {
  cwd.value = path
  if (socket?.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify({ type: 'chdir', path }))
  }
}

function focus() {
  term?.focus()
}

watch(
  () => userStore.theme,
  () => {
    if (term) term.options.theme = themeForMode()
  },
)

watch(
  () => props.cwd,
  (value) => {
    if (value && value !== cwd.value) chdir(value)
  },
)

onMounted(async () => {
  await nextTick()
  ensureTerminal()
  if (props.autoConnect) connect()
})

onBeforeUnmount(() => {
  disconnect()
  observer?.disconnect()
  observer = null
  fit = null
  term?.dispose()
  term = null
})

defineExpose({ connect, disconnect, clear, chdir, focus, refit, connected })
</script>

<style scoped lang="scss">
.term-pane {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--lp-surface);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  overflow: hidden;

  &--flat { border: 0; border-radius: 0; }
}

[data-theme='dark'] .term-pane { background: #0f1218; }

.term-bar {
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  flex: none;
  padding: var(--lp-2) var(--lp-3);
  border-bottom: 1px solid var(--lp-line);
  background: var(--lp-surface-sunken);
}

.term-cwd {
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  min-width: 0;
}

.term-bar__actions {
  display: flex;
  align-items: center;
  gap: var(--lp-1);
  margin-left: auto;
}

.term-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: 0;
  border-radius: var(--lp-r-sm);
  background: transparent;
  color: var(--lp-text-dim);
  cursor: pointer;
  transition: background var(--lp-fast), color var(--lp-fast);

  &:hover { background: var(--lp-surface-hover); color: var(--lp-text); }
}

.term-host {
  flex: 1;
  min-height: 0;
  padding: var(--lp-2);
}

/* xterm renders its own scrollbar; keep it subtle. */
.term-host :deep(.xterm-viewport)::-webkit-scrollbar { width: 8px; }
.term-host :deep(.xterm-viewport)::-webkit-scrollbar-thumb {
  background: var(--lp-line-strong);
  border-radius: var(--lp-r-full);
}
</style>
