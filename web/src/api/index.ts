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
export const removeDomain = (siteId: number, id: number) => request.delete(`/sites/${siteId}/domains/${id}`)

// Settings
export const getSettings = () => request.get('/settings')
export const saveSettingsApi = (data: any) => request.put('/settings', data)
export const updateSettings = (data: Record<string, string>) => request.put('/settings', data)
export const changePasswordApi = (data: { username: string; old_password: string; new_password: string }) =>
  request.post('/settings/password', data)

// Files
export const listFiles = (path: string) => request.get('/files/list', { params: { path } })
export const readFileContent = (path: string) => request.get('/files/read', { params: { path } })
export const writeFileContent = (path: string, content: string) => request.post('/files/write', { path, content })
export const mkdirApi = (path: string) => request.post('/files/mkdir', { path })
export const deletePathApi = (path: string) => request.post('/files/delete', { path })
export const renamePathApi = (from: string, to: string) => request.post('/files/rename', { from, to })
export const execCmd = (cmd: string, cwd?: string) => request.post('/files/exec', { cmd, cwd })
export const fileInfo = (path: string) => request.get('/files/info', { params: { path } })
export const chmodApi = (path: string, mode: string) => request.post('/files/chmod', { path, mode })
