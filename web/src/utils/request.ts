import axios from 'axios'
import { ElMessage } from 'element-plus'
import router from '@/router'
import { t } from '@/i18n'

const request = axios.create({
  baseURL: '/api',
  timeout: 30000,
})

function isApiEnvelope(data: unknown): data is { success: boolean; message?: unknown } {
  return !!data && typeof data === 'object' && 'success' in data
}

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
  'Path is not a directory': 'api.pathNotDirectory',
  'File not found': 'api.fileNotFound',
  'Download supports file only': 'api.downloadFileOnly',
  'File too large (>10MB)': 'api.fileTooLarge',
  'File saved': 'api.fileSaved',
  'Directory created': 'api.directoryCreated',
  'Deleted': 'api.deleted',
  'Renamed': 'api.renamed',
  'Upload target path is required': 'api.uploadPathRequired',
  'Upload target path is not a directory': 'api.uploadPathNotDirectory',
  'No file uploaded': 'api.noFileUploaded',
  'Invalid upload file name': 'api.invalidUploadFileName',
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
  'OpenResty is required. Please install OpenResty first.': 'api.openrestyRequired',
  'OpenResty quick install is supported on Windows and Linux only': 'api.openrestyInstallWinLinuxOnly',
  'OpenResty sync is supported on Windows and Linux only': 'api.openrestySyncWinLinuxOnly',
  'OpenResty quick install requires root privileges or passwordless sudo': 'api.openrestyInstallNeedRoot',
  'OpenResty quick install requires Administrator privileges': 'api.openrestyInstallNeedAdmin',
  'No supported package manager found (winget/choco/scoop)': 'api.openrestyInstallNoPkgManager',
  'All OpenResty quick install methods failed. Please review install logs.': 'api.openrestyInstallAllMethodsFailed',
  'No installer method available': 'api.openrestyInstallNoMethod',
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
  if (text.startsWith('OpenResty apply failed:')) {
    return t('api.openrestyApplyFailed', { detail: text.replace(/^OpenResty apply failed:\s*/i, '') })
  }
  if (text.startsWith('OpenResty install failed:')) {
    return t('api.openrestyInstallFailed', { detail: text.replace(/^OpenResty install failed:\s*/i, '') })
  }
  if (text.startsWith('Failed to read file:')) {
    return t('api.readFileFailed', { detail: text.replace(/^Failed to read file:\s*/i, '') })
  }
  if (text.startsWith('Failed to parse upload request:')) {
    return t('api.uploadParseFailed', { detail: text.replace(/^Failed to parse upload request:\s*/i, '') })
  }
  if (text.startsWith('Failed to read upload field:')) {
    return t('api.uploadReadFailed', { detail: text.replace(/^Failed to read upload field:\s*/i, '') })
  }
  if (text.startsWith('Failed to save upload:')) {
    return t('api.uploadSaveFailed', { detail: text.replace(/^Failed to save upload:\s*/i, '') })
  }
  return text
}

request.interceptors.request.use((config) => {
  const token = localStorage.getItem('token')
  if (token) config.headers.Authorization = `Bearer ${token}`
  return config
})

request.interceptors.response.use(
  (res: any): any => {
    if (!isApiEnvelope(res.data)) {
      const rawText = typeof res.data === 'string' ? res.data : ''
      const isHtml = /^\s*<!doctype html>/i.test(rawText) || /^\s*<html/i.test(rawText)
      const message = isHtml ? t('api.unexpectedHtmlResponse') : t('api.unexpectedApiResponse')
      ElMessage.error(message)
      return Promise.reject(new Error(message))
    }

    if (!res.data.success) {
      const message = localizeApiMessage(res.data.message)
      ;(res.data as { message?: string }).message = message
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
