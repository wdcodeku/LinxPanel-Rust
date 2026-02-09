import request from '@/utils/request'

// Auth
export const login = (data: { username: string; password: string }) =>
  request.post('/auth/login', data)

// System
export const getSystemInfo = () => request.get('/system/info')

// Sites
export const getSites = () => request.get('/sites')
export const getSite = (id: number) => request.get(`/sites/${id}`)
export const createSite = (data: any) => request.post('/sites', data)
export const updateSite = (id: number, data: any) => request.put(`/sites/${id}`, data)
export const deleteSite = (id: number) => request.delete(`/sites/${id}`)
export const startSite = (id: number) => request.post(`/sites/${id}/start`)
export const stopSite = (id: number) => request.post(`/sites/${id}/stop`)

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
