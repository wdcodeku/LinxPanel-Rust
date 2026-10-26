import request from '@/utils/request'

// Auth
export const login = (data: { username: string; password: string; code?: string }) =>
  request.post('/auth/login', data)

// System
export const getSystemInfo = () => request.get('/system/info')
export const getSystemOverview = () => request.get('/system/overview')
export const getRealtimeSnapshot = () => request.get('/realtime/snapshot')

// Processes
export type ProcessQuery = {
  keyword?: string
  sort_by?: string
  order?: string
  offset?: number
  limit?: number
}
export const getProcesses = (params: ProcessQuery = {}) =>
  request.get('/processes', { params })
export const signalProcess = (pid: number, signal: string) =>
  request.post(`/processes/${pid}/signal`, { signal })

// Services
export const getServiceBackend = () => request.get('/services/backend')
export const getServices = () => request.get('/services')
export const serviceAction = (data: { name: string; action: string }) =>
  request.post('/services/action', data)
export const getServiceStatus = (name: string) =>
  request.get(`/services/${encodeURIComponent(name)}/status`)
export const getServiceLogs = (name: string, lines = 200) =>
  request.get(`/services/${encodeURIComponent(name)}/logs`, { params: { lines } })

// Cron
export type CronJobPayload = {
  name: string
  schedule: string
  command: string
  enabled: boolean
}
export const getCronBackend = () => request.get('/cron/backend')
export const getCronJobs = () => request.get('/cron')
export const createCronJob = (data: CronJobPayload) => request.post('/cron', data)
export const updateCronJob = (id: number, data: CronJobPayload) => request.post(`/cron/${id}`, data)
export const deleteCronJob = (id: number) => request.delete(`/cron/${id}`)
export const toggleCronJob = (id: number, enabled: boolean) =>
  request.post(`/cron/${id}/toggle`, { enabled })

// Firewall & ports
export const getFirewallStatus = () => request.get('/firewall/status')
export const getFirewallRules = () => request.get('/firewall/rules')
export const addFirewallRule = (data: {
  port: string
  protocol: string
  action: string
  source: string
}) => request.post('/firewall/rules', data)
export const deleteFirewallRule = (id: string) =>
  request.delete(`/firewall/rules/${encodeURIComponent(id)}`)
export const toggleFirewall = (enabled: boolean) => request.post('/firewall/toggle', { enabled })
export const getListeningPorts = () => request.get('/network/ports')

// Runtimes & managed applications
export const getRuntimes = () => request.get('/runtimes')
export const installRuntime = (kind: string) =>
  request.post('/runtimes/install', { kind }, { timeout: 20 * 60 * 1000 })

export type AppPayload = {
  name: string
  runtime: string
  work_dir: string
  entry: string
  args: string
  env: string
  port: number
  autostart: boolean
}
export const getApps = () => request.get('/apps')
export const createApp = (data: AppPayload) => request.post('/apps', data)
export const updateApp = (id: number, data: AppPayload) => request.post(`/apps/${id}`, data)
export const deleteApp = (id: number) => request.delete(`/apps/${id}`)
export const appAction = (id: number, action: 'start' | 'stop' | 'restart') =>
  request.post(`/apps/${id}/${action}`)
export const getAppLogs = (id: number, lines = 300) =>
  request.get(`/apps/${id}/logs`, { params: { lines } })
export const clearAppLogs = (id: number) => request.delete(`/apps/${id}/logs`)

// Sites
export const getSites = () => request.get('/sites')
export const getSite = (id: number) => request.get(`/sites/${id}`)
export const createSite = (data: any) => request.post('/sites', data)
export const updateSite = (id: number, data: any) => request.put(`/sites/${id}`, data)
export const deleteSite = (id: number) => request.delete(`/sites/${id}`)
export const startSite = (id: number) => request.post(`/sites/${id}/start`)
export const stopSite = (id: number) => request.post(`/sites/${id}/stop`)

// Certificates
export const getCerts = () => request.get('/certs')
export const getAcmeStatus = () => request.get('/certs/acme/status')
export const uploadCert = (data: { name: string; cert_pem: string; key_pem: string }) =>
  request.post('/certs/upload', data)
export const createSelfSignedCert = (data: { name: string; domains: string[]; days: number }) =>
  request.post('/certs/self-signed', data)
export const issueAcmeCert = (data: {
  name: string
  domains: string[]
  email: string
  webroot: string
  staging: boolean
}) => request.post('/certs/acme', data, { timeout: 12 * 60 * 1000 })
export const renewCert = (id: number) =>
  request.post(`/certs/${id}/renew`, {}, { timeout: 12 * 60 * 1000 })
export const deleteCert = (id: number) => request.delete(`/certs/${id}`)
export const bindSiteSsl = (
  siteId: number,
  data: { cert_id: number | null; force_https: boolean; hsts: boolean },
) => request.post(`/sites/${siteId}/ssl`, data)

// Domains
export const getDomains = (siteId: number) => request.get(`/sites/${siteId}/domains`)
export const addDomain = (siteId: number, data: any) => request.post(`/sites/${siteId}/domains`, data)
export const reorderDomains = (siteId: number, data: { ordered_ids: number[] }) =>
  request.post(`/sites/${siteId}/domains/reorder`, data)
export const updateDomain = (siteId: number, id: number, data: { domain: string }) =>
  request.put(`/sites/${siteId}/domains/${id}`, data)
export const removeDomain = (siteId: number, id: number) => request.delete(`/sites/${siteId}/domains/${id}`)

// Settings
export const getSettings = () => request.get('/settings')
export const saveSettingsApi = (data: any) => request.put('/settings', data)
export const updateSettings = (data: Record<string, string>) => request.put('/settings', data)
export const getPanelAccess = () => request.get('/settings/panel-access')
export const setEntranceApi = (entrance: string) => request.post('/settings/entrance', { entrance })
export const setPanelSslApi = (data: { enabled: boolean; cert_id?: number | null; regenerate?: boolean }) =>
  request.post('/settings/panel-ssl', data)
export const restartPanelApi = () => request.post('/settings/panel/restart')
export const setPanelPortApi = (port: number) => request.post('/settings/panel-port', { port })
export const setAllowedIpsApi = (ips: string) => request.post('/settings/allowed-ips', { ips })
export const setBindDomainApi = (domain: string) => request.post('/settings/bind-domain', { domain })
export const setDenyResponseApi = (mode: string) => request.post('/settings/deny-response', { mode })
export const mfaSetupApi = () => request.post('/settings/mfa/setup')
export const mfaEnableApi = (code: string) => request.post('/settings/mfa/enable', { code })
export const mfaDisableApi = (code: string) => request.post('/settings/mfa/disable', { code })
/** On success `data.token` / `data.username` replace the session's. */
export const changeUsernameApi = (data: { username: string; password: string }) =>
  request.post('/settings/username', data)
/** On success `data.token` replaces the session token: older tokens are revoked. */
export const changePasswordApi = (data: { username: string; old_password: string; new_password: string }) =>
  request.post('/settings/password', data)
export const getOpenRestyStatus = () => request.get('/settings/openresty/status')
export const quickInstallOpenResty = () =>
  request.post('/settings/openresty/install', {}, { timeout: 20 * 60 * 1000 })

// Files
export const listFiles = (path: string) => request.get('/files/list', { params: { path } })
export const getFileRoots = () => request.get('/files/roots')
export const readFileContent = (path: string) => request.get('/files/read', { params: { path } })
export const getDirectorySize = (path: string) => request.get('/files/dir-size', { params: { path } })
export const downloadFileApi = async (path: string, fallbackName?: string) => {
  const token = localStorage.getItem('token')
  const response = await fetch(`/api/files/download?path=${encodeURIComponent(path)}`, {
    method: 'GET',
    headers: token ? { Authorization: `Bearer ${token}` } : undefined,
  })

  if (!response.ok) {
    let message = `HTTP ${response.status}`
    try {
      const data = await response.json() as { message?: string }
      if (data?.message) message = data.message
    } catch {
      try {
        const text = (await response.text()).trim()
        if (text) message = text
      } catch {}
    }
    throw new Error(message)
  }

  const blob = await response.blob()
  const suggestedName = fallbackName || path.split('/').filter(Boolean).pop() || 'download'
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = suggestedName
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  URL.revokeObjectURL(url)
}
export const writeFileContent = (path: string, content: string) => request.post('/files/write', { path, content })
export const uploadFilesApi = (path: string, files: File[]) => {
  const formData = new FormData()
  formData.append('path', path)
  files.forEach((file) => {
    formData.append('files', file, file.name)
  })
  return request.post('/files/upload', formData, { timeout: 10 * 60 * 1000 })
}
export const mkdirApi = (path: string) => request.post('/files/mkdir', { path })
export const deletePathApi = (path: string) => request.post('/files/delete', { path })
export const renamePathApi = (from: string, to: string) => request.post('/files/rename', { from, to })
export const execCmd = (cmd: string, cwd?: string) => request.post('/files/exec', { cmd, cwd })
export type ExecStreamChunk = { type: 'out' | 'err' | 'exit'; data?: string; exit_code?: number; done?: boolean }
export const execCmdStream = async (
  cmd: string,
  cwd: string | undefined,
  onChunk: (chunk: ExecStreamChunk) => void,
) => {
  const token = localStorage.getItem('token')
  const response = await fetch('/api/files/exec/stream', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ cmd, cwd }),
  })

  if (!response.ok) {
    let errorText = `HTTP ${response.status}`
    try {
      const body = await response.json() as { message?: string }
      if (body?.message) errorText = body.message
    } catch {}
    throw new Error(errorText)
  }

  if (!response.body) {
    throw new Error('Readable stream is not available')
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  while (true) {
    const { value, done } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    let lineEnd = buffer.indexOf('\n')
    while (lineEnd >= 0) {
      const line = buffer.slice(0, lineEnd).trim()
      buffer = buffer.slice(lineEnd + 1)
      if (line) {
        try {
          onChunk(JSON.parse(line) as ExecStreamChunk)
        } catch {}
      }
      lineEnd = buffer.indexOf('\n')
    }
  }

  const tail = buffer.trim()
  if (tail) {
    try {
      onChunk(JSON.parse(tail) as ExecStreamChunk)
    } catch {}
  }
}
export const fileInfo = (path: string) => request.get('/files/info', { params: { path } })
export const chmodApi = (path: string, mode: string) => request.post('/files/chmod', { path, mode })
