import request from '@/utils/request'
import { runTask } from '@/utils/tasks'
import { t } from '@/i18n'

const PRODUCT_NAMES: Record<string, string> = {
  mysql: 'MySQL / MariaDB', postgresql: 'PostgreSQL', redis: 'Redis',
  php: 'PHP', node: 'Node.js', python: 'Python', java: 'Java', go: 'Go',
  wordpress: 'WordPress', typecho: 'Typecho', adminer: 'Adminer', phpmyadmin: 'phpMyAdmin', dokuwiki: 'DokuWiki',
  filebrowser: 'File Browser', frps: 'frp Server', 'ddns-go': 'DDNS-GO', gitea: 'Gitea',
}
const productName = (key: string) => PRODUCT_NAMES[key] || key

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

// Active defense
export type DefenseConfig = {
  enabled: boolean
  ban_minutes: number
  whitelist: string
  web_enabled: boolean
  web_window: number
  web_max_requests: number
  web_max_errors: number
  ssh_enabled: boolean
  ssh_window: number
  ssh_max_failures: number
  panel_enabled: boolean
  panel_window: number
  panel_max_denied: number
  cc_enabled: boolean
  cc_rate: number
  cc_burst: number
  cc_conn: number
}
export const getDefense = () => request.get('/defense')
export const saveDefense = (data: DefenseConfig) => request.post('/defense', data)
export const getDefenseBans = () => request.get('/defense/bans')
export const getDefenseEvents = (limit = 200) => request.get('/defense/events', { params: { limit } })
export const banDefenseIp = (data: { ip: string; minutes?: number; reason?: string }) =>
  request.post('/defense/ban', data)
export const unbanDefenseIp = (ip: string) => request.post('/defense/unban', { ip })
export const installNftables = () =>
  runTask(t('task.installNftables'), () => request.post('/defense/install-nft', {}))
export const setKernelHardening = (enabled: boolean) => request.post('/defense/sysctl', { enabled })

// Panel updates
export type UpdateConfig = {
  auto_update: boolean
  auto_time: string
  source: 'github' | 'proxy' | 'mirror' | string
  proxy: string
  mirror: string
}
export type UpdateRelease = { tag: string; notes: string; url: string; published_at: string }
export type UpdateStatus = {
  current: string
  check: { current: string; latest: UpdateRelease | null; has_update: boolean; checked_at: number; error: string } | null
  config: UpdateConfig
  running: { from: string; target: string; started_at: number; auto: boolean } | null
  log: string
  last_result: { target: string; success: boolean; finished_at: number; message: string } | null
  can_update: boolean
}
export const getUpdateStatus = () => request.get('/update')
export const checkUpdate = () => request.post('/update/check', {}, { timeout: 60000 })
export const saveUpdateSettings = (data: UpdateConfig) => request.post('/update/settings', data)
export const runPanelUpdate = (tag: string) => request.post('/update/run', { tag }, { timeout: 60000 })
export const getListeningPorts = () => request.get('/network/ports')

// Runtimes & managed applications
export const getRuntimes = () => request.get('/runtimes')
export const installRuntime = (kind: string) =>
  runTask(t('task.install', { name: productName(kind) }), () => request.post('/runtimes/install', { kind }))

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
/** Writes a sample project, builds it if needed and starts it (as a task). */
export const createAppFromSample = (data: { name: string; runtime: string; work_dir: string; port: number; run_user: string }) =>
  runTask(t('task.sample', { name: data.name }), () => request.post('/apps/from-sample', data), { closeOnSuccess: true })
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

// Databases
const LONG = { timeout: 20 * 60 * 1000 }
export const getDbEngines = () => request.get('/databases/engines')
export const installDbEngine = (engine: string, version?: string) =>
  runTask(t('task.install', { name: productName(engine) }), () =>
    request.post(`/databases/engines/${engine}/install`, version ? { version } : {}),
  )
export const restartDbEngine = (engine: string) => request.post(`/databases/engines/${engine}/restart`)
export const getDbVersions = (engine: string) => request.get(`/databases/engines/${engine}/versions`, { timeout: 60000 })
export const getDbNetwork = (engine: string) => request.get(`/databases/engines/${engine}/network`)
export const setDbRemote = (engine: string, data: { enabled: boolean; allow_from: string }) =>
  request.post(`/databases/engines/${engine}/remote`, data, { timeout: 180000 })
// Settings changes restart the server (and roll back if it does not come up).
const RESTART = { timeout: 300000 }
export const getDbSettings = (engine: string) =>
  request.get(`/databases/engines/${engine}/settings`, { timeout: 60000 })
export const applyDbSettings = (
  engine: string,
  data: { params?: Record<string, string>; logs?: Record<string, { enabled?: boolean; threshold?: string }> },
) => request.post(`/databases/engines/${engine}/settings`, data, RESTART)
export const setDbSuperuserPassword = (engine: string, password: string) =>
  request.post(`/databases/engines/${engine}/superuser-password`, { password }, RESTART)
export const startDbEngine = (engine: string) => request.post(`/databases/engines/${engine}/start`, {}, RESTART)
export const stopDbEngine = (engine: string) => request.post(`/databases/engines/${engine}/stop`, {}, RESTART)
export const uninstallDbEngine = (engine: string, purgeData: boolean) =>
  runTask(t('task.uninstall', { name: productName(engine) }), () =>
    request.post(`/databases/engines/${engine}/uninstall`, { purge_data: purgeData }),
  )
export const getDbConfig = (engine: string, file?: string) =>
  request.get(`/databases/engines/${engine}/config`, { params: file ? { file } : {} })
export const saveDbConfig = (engine: string, file: string, content: string) =>
  request.post(`/databases/engines/${engine}/config`, { file, content }, RESTART)
export const getDbLog = (engine: string, kind: string, lines = 300) =>
  request.get(`/databases/engines/${engine}/logs/${kind}`, { params: { lines } })
export const clearDbLog = (engine: string, kind: string) =>
  request.post(`/databases/engines/${engine}/logs/${kind}/clear`)
export const purgeBinlogs = () => request.post('/databases/engines/mysql/binlog/purge', {}, { timeout: 120000 })
export const changeDatabaseHost = (name: string, host: string) =>
  request.post(`/databases/mysql/${encodeURIComponent(name)}/host`, { host })
/** MySQL / MongoDB: the administrator login the panel uses (empty user: create one). */
export const setDbAdmin = (engine: string, data: { user: string; password: string }) =>
  request.post(`/databases/engines/${engine}/admin`, data, LONG)

// --- SQLite (files, by path) ---
export const listSqlite = () => request.get('/sqlite', { timeout: 60000 })
export const discoverSqlite = () => request.get('/sqlite/discover', { timeout: 120000 })
export const registerSqlite = (path: string) => request.post('/sqlite/register', { path })
export const createSqlite = (path: string) => request.post('/sqlite/create', { path })
export const removeSqlite = (path: string, deleteFile: boolean) =>
  request.post('/sqlite/remove', { path, delete_file: deleteFile })
// SQLite data editing
export type SqliteRowsParams = { path: string; table: string; offset: number; limit: number; order?: string; desc?: boolean; filter?: string }
export const getSqliteSchema = (path: string, table: string) =>
  request.get('/sqlite/schema', { params: { path, table } })
export const getSqliteRows = (params: SqliteRowsParams) => request.get('/sqlite/rows', { params, timeout: 60000 })
export const updateSqliteRow = (path: string, table: string, key: unknown, values: Record<string, unknown>) =>
  request.post('/sqlite/rows/update', { path, table, key, values })
export const insertSqliteRow = (path: string, table: string, values: Record<string, unknown>) =>
  request.post('/sqlite/rows/insert', { path, table, values })
export const deleteSqliteRows = (path: string, table: string, keys: unknown[]) =>
  request.post('/sqlite/rows/delete', { path, table, keys })
export const runSqliteQuery = (path: string, sql: string) =>
  request.post('/sqlite/query', { path, sql }, { timeout: 180000 })
export const getSqliteTables = (path: string) =>
  request.get('/sqlite/tables', { params: { path }, timeout: 60000 })
export const listSqliteBackups = (path: string) => request.get('/sqlite/backups', { params: { path } })
export const backupSqlite = (path: string) => request.post('/sqlite/backups', { path }, LONG)
export const restoreSqlite = (path: string, file: string) =>
  request.post('/sqlite/restore', { path, file }, LONG)
export const deleteSqliteBackup = (path: string, file: string) =>
  request.post('/sqlite/backups/delete', { path, file })
export const setRedisPassword = (password: string) =>
  request.post('/databases/engines/redis/password', { password })
export const listDatabases = (engine: string) => request.get('/databases', { params: { engine } })
export const getDatabaseTables = (engine: string, name: string) =>
  request.get(`/databases/${engine}/${encodeURIComponent(name)}/tables`, { timeout: 60000 })
export const adoptDatabase = (
  engine: string,
  name: string,
  data: { username: string; password: string; host: string; reset_password: boolean },
) => request.post(`/databases/${engine}/${encodeURIComponent(name)}/adopt`, data)
export const createDatabase = (data: {
  engine: string; name: string; username?: string; password?: string; host?: string; note?: string
}) => request.post('/databases', data)
export const deleteDatabase = (engine: string, name: string) =>
  request.post(`/databases/${engine}/${name}/delete`)
export const changeDatabasePassword = (engine: string, name: string, password?: string) =>
  request.post(`/databases/${engine}/${name}/password`, { password })
export const listDatabaseBackups = (engine: string, name: string) =>
  request.get(`/databases/${engine}/${name}/backups`)
export const backupDatabase = (engine: string, name: string) =>
  request.post(`/databases/${engine}/${name}/backups`, {}, LONG)
export const restoreDatabase = (engine: string, name: string, file: string) =>
  request.post(`/databases/${engine}/${name}/restore`, { file }, LONG)
export const deleteDatabaseBackup = (engine: string, name: string, file: string) =>
  request.post(`/databases/${engine}/${name}/backups/delete`, { file })

/** pip / npm / composer install in the app's directory; `build` also runs the npm build script. */
export const installAppDeps = (id: number, build = false) =>
  runTask(t('task.installDeps'), () => request.post(`/apps/${id}/install-deps`, { build }))

// Backups
export const listSiteBackups = (id: number) => request.get(`/sites/${id}/backups`)
export const backupSite = (id: number) => request.post(`/sites/${id}/backups`, {}, { timeout: 60 * 60 * 1000 })
export const restoreSiteBackup = (id: number, file: string) =>
  request.post(`/sites/${id}/backups/restore`, { file }, { timeout: 60 * 60 * 1000 })
export const deleteSiteBackup = (id: number, file: string) => request.post(`/sites/${id}/backups/delete`, { file })
export const getBackupCronPrefix = () => request.get('/backups/cron-prefix')

// App store
export const getAppCatalog = () => request.get('/appstore/apps')
export const getInstalledApps = () => request.get('/appstore/installed')
export const deployApp = (data: {
  app: string; domains: string[]; doc_root?: string; db_name?: string; source_url?: string; locale?: string
}) => runTask(t('task.deploy', { name: productName(data.app) }), () => request.post('/appstore/deploy', data), { closeOnSuccess: true })
/** Service programs (File Browser, frps, ...): installed, upgraded and removed as managed apps. */
export const getServiceInstalls = () => request.get('/appstore/services')
export const installService = (data: {
  app: string; version: string; ports: number[]; public: boolean; domain: string; open_firewall: boolean
  run_user: string; root_dir: string; source_url: string; access_host: string; locale: string
}) => runTask(t('task.install', { name: productName(data.app) }), () => request.post('/appstore/services/install', data), { closeOnSuccess: true })
export const upgradeService = (id: number, name: string, data: { version: string; source_url: string }) =>
  runTask(t('task.upgrade', { name }), () => request.post(`/appstore/services/${id}/upgrade`, data), { closeOnSuccess: true })
export const uninstallService = (id: number, deleteData: boolean) =>
  request.post(`/appstore/services/${id}/uninstall`, { delete_data: deleteData })

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
export const setDefaultSite = (data: { mode: 'page' | 'close'; html: string }) =>
  request.post('/settings/openresty/default-site', data)
export const quickInstallOpenResty = () =>
  runTask(t('task.install', { name: 'OpenResty' }), () => request.post('/settings/openresty/install', {}))

// Files
export const listFiles = (path: string) => request.get('/files/list', { params: { path } })
export const getFileRoots = () => request.get('/files/roots')
export const readFileContent = (path: string) => request.get('/files/read', { params: { path } })
export const getDirectorySize = (path: string) => request.get('/files/dir-size', { params: { path } })
/**
 * Starts a download in the browser's own download manager: the panel mints a
 * short-lived link and the browser fetches it directly, so the save dialog and
 * progress appear at once (no waiting for the whole file). Folders download as .tar.gz.
 */
export const downloadFileApi = async (path: string, _fallbackName?: string) => {
  const res: any = await request.post('/files/download-link', { path })
  if (!res?.success) throw new Error(res?.message || 'Download failed')
  const anchor = document.createElement('a')
  anchor.href = res.data.url
  anchor.download = res.data.name
  anchor.rel = 'noopener'
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
}
export const writeFileContent = (path: string, content: string, options: { expected_version?: string; force?: boolean } = {}) =>
  request.post('/files/write', { path, content, ...options })
/** One file per request so each has its own progress and can be canceled. */
export const uploadFileApi = (path: string, file: File, signal: AbortSignal, onProgress: (loaded: number) => void) => {
  const formData = new FormData()
  formData.append('path', path)
  formData.append('files', file, file.name)
  return request.post('/files/upload', formData, {
    // Large files on a slow link take as long as they take.
    timeout: 0,
    signal,
    onUploadProgress: (event) => onProgress(event.loaded),
  })
}
export const mkdirApi = (path: string) => request.post('/files/mkdir', { path })
/** { exists, is_dir } - never an error toast for a missing path. */
export const pathState = (path: string) => request.get('/files/exists', { params: { path } })
export const deletePathApi = (path: string, permanent = false) => request.post('/files/delete', { path, permanent })
/** Deletes several paths in one request; to the recycle bin unless `permanent`. */
export const deletePaths = (paths: string[], permanent = false) => request.post('/files/delete', { paths, permanent })
// Recycle bin
export const getRecycleBin = () => request.get('/files/recycle')
export const restoreRecycled = (ids: number[]) => request.post('/files/recycle/restore', { ids })
export const purgeRecycled = (ids: number[]) => request.post('/files/recycle/delete', { ids })
export const emptyRecycleBin = () => request.post('/files/recycle/empty')
export const saveRecycleSettings = (data: { enabled: boolean; keep_days: number }) => request.post('/files/recycle/settings', data)
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
export const chmodApi = (path: string, mode: string, recursive = false) =>
  request.post('/files/chmod', { path, mode, recursive }, { timeout: 10 * 60 * 1000 })
export const chownApi = (path: string, user: string, group: string, recursive = false) =>
  request.post('/files/chown', { path, user, group, recursive }, { timeout: 10 * 60 * 1000 })
export const getOwnerChoices = () => request.get('/files/owners')
const fileName = (path: string) => path.slice(path.lastIndexOf('/') + 1)
/** Archive operations and copy / move run as tasks (their log shows in the task window). */
export const extractArchive = (path: string, dest: string, overwrite: boolean) =>
  runTask(t('task.extract', { name: fileName(path) }), () => request.post('/files/extract', { path, dest, overwrite }), { closeOnSuccess: true })
export const compressFiles = (dir: string, names: string[], name: string, format: string) =>
  runTask(t('task.compress', { name }), () => request.post('/files/compress', { dir, names, name, format }), { closeOnSuccess: true })
export const transferFiles = (sources: string[], dest: string, mode: 'copy' | 'move', conflict: string) =>
  runTask(t(mode === 'move' ? 'task.move' : 'task.copy', { n: sources.length }), () =>
    request.post('/files/transfer', { sources, dest, mode, conflict }), { closeOnSuccess: true })
