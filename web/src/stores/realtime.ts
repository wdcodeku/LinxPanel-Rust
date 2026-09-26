import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'

export type MetricsSample = {
  ts: number
  cpu_percent: number
  cpu_cores: number[]
  load_avg: [number, number, number]
  mem_total: number
  mem_used: number
  mem_available: number
  mem_percent: number
  swap_total: number
  swap_used: number
  net_rx_rate: number
  net_tx_rate: number
  disk_read_rate: number
  disk_write_rate: number
  uptime: number
}

export type StaticInfo = {
  hostname: string
  os_name: string
  os_version: string
  kernel_version: string
  arch: string
  platform: string
  boot_time: number
  cpu_model: string
  cpu_vendor: string
  cpu_frequency_mhz: number
  physical_cores: number
  logical_cores: number
  mem_total: number
  swap_total: number
  panel_version: string
}

export type DiskInfo = {
  name: string
  mount_point: string
  fs_type: string
  total: number
  used: number
  free: number
  usage_percent: number
  is_removable: boolean
}

export type NetworkInterface = {
  name: string
  rx_bytes: number
  tx_bytes: number
  rx_rate: number
  tx_rate: number
  mac_address: string
  ip_addresses: string[]
  mtu: number
}

export type MetricsDetail = {
  ts: number
  disks: DiskInfo[]
  interfaces: NetworkInterface[]
  process_count: number
}

export type ConnectionState = 'connecting' | 'open' | 'closed'

/** Matches the server ring buffer so a reconnect never grows the client series. */
const MAX_POINTS = 600
const RECONNECT_BASE_MS = 1000
const RECONNECT_MAX_MS = 15000

export const useRealtimeStore = defineStore('realtime', () => {
  const state = ref<ConnectionState>('closed')
  const staticInfo = ref<StaticInfo | null>(null)
  const latest = ref<MetricsSample | null>(null)
  const detail = ref<MetricsDetail | null>(null)
  /** Chart series are replaced wholesale, so deep reactivity would only cost us. */
  const history = shallowRef<MetricsSample[]>([])
  const lastUpdate = ref(0)

  let socket: WebSocket | null = null
  let reconnectTimer: number | null = null
  let attempts = 0
  /** Views subscribe/unsubscribe on mount/unmount; the socket lives while any is open. */
  let subscribers = 0

  const connected = computed(() => state.value === 'open')

  function buildUrl(): string {
    const token = localStorage.getItem('token') || ''
    const url = new URL('/api/realtime/ws', window.location.origin)
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
    url.searchParams.set('token', token)
    return url.toString()
  }

  function pushSample(sample: MetricsSample) {
    const next = history.value.length >= MAX_POINTS
      ? history.value.slice(history.value.length - MAX_POINTS + 1)
      : history.value.slice()
    next.push(sample)
    history.value = next
    latest.value = sample
    lastUpdate.value = Date.now()
  }

  function handleMessage(event: MessageEvent) {
    let payload: any
    try {
      payload = JSON.parse(event.data)
    } catch {
      return
    }

    if (payload.type === 'snapshot') {
      const data = payload.data || {}
      staticInfo.value = data.static_info || null
      history.value = Array.isArray(data.history) ? data.history.slice(-MAX_POINTS) : []
      latest.value = data.latest || history.value[history.value.length - 1] || null
      detail.value = data.detail || null
      lastUpdate.value = Date.now()
      return
    }
    if (payload.type === 'tick') {
      // The frame is the sample itself: { type, ...MetricsSample }
      const { type, ...sample } = payload
      pushSample(sample as MetricsSample)
      return
    }
    if (payload.type === 'detail') {
      const { type, ...rest } = payload
      detail.value = rest as MetricsDetail
    }
  }

  function scheduleReconnect() {
    if (reconnectTimer !== null || subscribers === 0) return
    const delay = Math.min(RECONNECT_BASE_MS * 2 ** attempts, RECONNECT_MAX_MS)
    attempts += 1
    reconnectTimer = window.setTimeout(() => {
      reconnectTimer = null
      connect()
    }, delay)
  }

  function connect() {
    if (socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) {
      return
    }
    if (!localStorage.getItem('token')) return

    state.value = 'connecting'
    const ws = new WebSocket(buildUrl())
    socket = ws

    ws.onopen = () => {
      if (socket !== ws) return
      state.value = 'open'
      attempts = 0
    }
    ws.onmessage = (event) => {
      if (socket === ws) handleMessage(event)
    }
    ws.onclose = () => {
      if (socket !== ws) return
      socket = null
      state.value = 'closed'
      scheduleReconnect()
    }
    ws.onerror = () => {
      // onclose always follows, which is where reconnect is handled.
      ws.close()
    }
  }

  function subscribe() {
    subscribers += 1
    connect()
  }

  function unsubscribe() {
    subscribers = Math.max(0, subscribers - 1)
    if (subscribers === 0) disconnect()
  }

  function disconnect() {
    if (reconnectTimer !== null) {
      window.clearTimeout(reconnectTimer)
      reconnectTimer = null
    }
    attempts = 0
    const ws = socket
    socket = null
    state.value = 'closed'
    ws?.close()
  }

  /** Called on logout so a stale token never reconnects. */
  function reset() {
    subscribers = 0
    disconnect()
    staticInfo.value = null
    latest.value = null
    detail.value = null
    history.value = []
  }

  return {
    state,
    connected,
    staticInfo,
    latest,
    detail,
    history,
    lastUpdate,
    subscribe,
    unsubscribe,
    disconnect,
    reset,
  }
})
