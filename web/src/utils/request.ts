import axios from 'axios'
import { ElMessage } from 'element-plus'
import router from '@/router'
import { t } from '@/i18n'

const request = axios.create({
  baseURL: '/api',
  timeout: 30000,
})

const API_MESSAGE_MAP: Record<string, string> = {
  'Invalid credentials': 'api.invalidCredentials',
  'Site not found': 'api.siteNotFound',
  'Site name is required': 'api.siteNameRequired',
  'At least one domain is required': 'api.domainRequired',
  'Settings updated': 'api.settingsUpdated',
  'User not found': 'api.userNotFound',
  'Current password is incorrect': 'api.currentPasswordIncorrect',
  'Failed to hash password': 'api.passwordHashFailed',
  'Password changed': 'api.passwordChanged',
  'Path does not exist': 'api.pathNotExist',
  'Path not found': 'api.pathNotFound',
  'File not found': 'api.fileNotFound',
  'File too large (>10MB)': 'api.fileTooLarge',
  'File saved': 'api.fileSaved',
  'Directory created': 'api.directoryCreated',
  'Deleted': 'api.deleted',
  'Renamed': 'api.renamed',
  'Permission changed': 'api.permissionChanged',
  'Permission change not supported on Windows': 'api.permissionNotSupported',
  'Domain removed': 'api.domainRemoved',
  'Domain not found': 'api.domainNotFound',
  'Domain already exists': 'api.domainAlreadyExists',
  'Domain order is invalid': 'api.domainOrderInvalid',
  'Invalid domain format': 'api.invalidDomainFormat',
  'Site deleted': 'api.siteDeleted',
  'Site started': 'api.siteStarted',
  'Site stopped': 'api.siteStopped',
}

function localizeApiMessage(message: unknown): string {
  if (typeof message !== 'string' || !message.trim()) return t('api.requestFailed')
  const text = message.trim()
  const messageKey = API_MESSAGE_MAP[text]
  if (messageKey) return t(messageKey)

  if (text.startsWith('Cannot read:')) {
    return t('api.cannotRead', { detail: text.replace(/^Cannot read:\s*/i, '') })
  }
  if (text.startsWith('Failed:')) {
    return t('api.failedWithDetail', { detail: text.replace(/^Failed:\s*/i, '') })
  }
  if (text.startsWith('Failed to create site directory:')) {
    return t('api.createSiteDirFailed', { detail: text.replace(/^Failed to create site directory:\s*/i, '') })
  }
  return text
}

request.interceptors.request.use((config) => {
  const token = localStorage.getItem('token')
  if (token) config.headers.Authorization = `Bearer ${token}`
  return config
})

request.interceptors.response.use(
  (res) => {
    if (res.data && !res.data.success) {
      const message = localizeApiMessage(res.data.message)
      res.data.message = message
      ElMessage.error(message)
    }
    return res.data
  },
  (err) => {
    if (err.response?.status === 401) {
      localStorage.removeItem('token')
      router.push('/login')
    }
    const message = /timeout/i.test(err?.message || '')
      ? t('api.timeout')
      : t('api.networkError')
    ElMessage.error(message)
    return Promise.reject(err)
  },
)

export default request
