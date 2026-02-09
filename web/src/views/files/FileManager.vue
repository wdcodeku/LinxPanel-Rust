<template>
  <div class="file-manager">
    <div class="fm-toolbar">
      <div class="fm-nav">
        <div class="fm-nav-btns">
          <el-tooltip :content="t('fm.goBack')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" :icon="Back" :disabled="!canGoBack" @click="goBack" />
          </el-tooltip>
          <el-tooltip :content="t('fm.goForward')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" :icon="Right" :disabled="!canGoForward" @click="goForward" />
          </el-tooltip>
          <el-tooltip :content="t('fm.goUp')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" :icon="Top" :disabled="atPathRoot" @click="goUp" />
          </el-tooltip>
          <el-tooltip :content="t('refresh')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" :icon="RefreshRight" @click="refresh" />
          </el-tooltip>
          <el-tooltip :content="showHiddenFiles ? t('fm.hideHidden') : t('fm.showHidden')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" @click="toggleHiddenFiles">
              <el-icon :size="15"><Hide v-if="showHiddenFiles" /><View v-else /></el-icon>
            </el-button>
          </el-tooltip>
        </div>
      </div>
      <div class="fm-path-bar" @click="startEditPath">
        <template v-if="!editingPath">
          <el-icon class="fm-path-icon fm-path-icon-clickable" :size="14" :title="pathIconTooltip" @click.stop="onPathIconClick"><FolderOpened /></el-icon>
          <span class="fm-path-root" @click.stop="goTo(rootPath)">{{ rootLabel }}</span>
          <template v-for="(seg, i) in pathSegments" :key="i">
            <el-icon :size="10" class="fm-path-arrow"><ArrowRight /></el-icon>
            <span class="fm-path-seg" @click.stop="goTo(segmentPath(i))">{{ seg }}</span>
          </template>
        </template>
        <input v-else ref="pathInputRef" v-model="pathInputVal" class="fm-path-input"
          @keyup.enter="confirmPath" @keyup.escape="editingPath = false" @blur="editingPath = false" />
      </div>
      <div class="fm-actions">
        <el-tooltip :content="t('fm.newFolder')" :show-after="400"><el-button size="small" :icon="FolderAdd" @click="showMkdir = true" /></el-tooltip>
        <el-tooltip :content="t('fm.newFile')" :show-after="400"><el-button size="small" :icon="DocumentAdd" @click="showNewFile = true" /></el-tooltip>
        <el-tooltip :content="t('fm.uploadFiles')" :show-after="400">
          <el-button size="small" :icon="Upload" :loading="uploadingFiles" :disabled="isVirtualDriveView" @click="triggerUpload" />
        </el-tooltip>
        <el-divider direction="vertical" />
        <el-tooltip :content="viewMode === 'list' ? t('gridView') : t('listView')" :show-after="400">
          <el-button size="small" @click="toggleView"><el-icon :size="14"><Grid v-if="viewMode === 'list'" /><List v-else /></el-icon></el-button>
        </el-tooltip>
        <el-tooltip :content="t('terminal')" :show-after="400"><el-button size="small" @click="toggleTerminal"><el-icon :size="14"><Monitor /></el-icon></el-button></el-tooltip>
      </div>
      <input ref="uploadInputRef" class="fm-upload-input" type="file" multiple @change="onUploadInputChange" />
    </div>

    <div
      class="fm-body"
      ref="fmBodyRef"
      @mousedown="onBodyMouseDown"
      @contextmenu.prevent="onBodyContext"
      @dragenter.prevent="onUploadDragEnter"
      @dragover.prevent="onUploadDragOver"
      @dragleave.prevent="onUploadDragLeave"
      @drop.prevent="onUploadDrop"
    >
      <div v-if="viewMode === 'list'" class="fm-list" ref="listRef">
        <div class="fm-list-header">
          <span class="fm-col-name">{{ t('fm.name') }}</span>
          <span class="fm-col-size">{{ t('fm.size') }}</span>
          <span class="fm-col-time">{{ t('fm.modified') }}</span>
          <span class="fm-col-perm">{{ t('fm.permissions') }}</span>
        </div>
        <div class="fm-list-body" ref="scrollRef" v-loading="loading">
          <div v-for="f in visibleFiles" :key="f.path" class="fm-row"
            :class="{ selected: selectedSet.has(f.path) }" :data-path="f.path"
            @click.exact="selectOne(f)" @click.ctrl.exact="toggleSelect(f)" @click.shift.exact="shiftSelect(f)"
            @dblclick="handleOpen(f)" @contextmenu.prevent.stop="onContext($event, f)"
            @mousedown.left.stop="onRowMouseDown($event, f)">
            <span class="fm-col-name">
              <el-icon :size="18" :color="f.is_dir ? 'var(--lp-warning)' : fileIconColor(f.name)">
                <Folder v-if="f.is_dir" /><Document v-else />
              </el-icon>
              <span class="fm-fname">{{ f.name }}</span>
            </span>
            <span class="fm-col-size">{{ f.is_dir ? '-' : fmtSize(f.size) }}</span>
            <span class="fm-col-time">{{ f.modified }}</span>
            <span class="fm-col-perm">{{ f.permissions }}</span>
          </div>
          <div v-if="!loading && visibleFiles.length === 0" class="fm-empty">
            <el-empty :description="t('fm.emptyDir')" :image-size="64" />
          </div>
        </div>
      </div>

      <div v-else class="fm-grid" ref="scrollRef" v-loading="loading">
        <div v-for="f in visibleFiles" :key="f.path" class="fm-grid-item"
          :class="{ selected: selectedSet.has(f.path) }" :data-path="f.path"
          @click.exact="selectOne(f)" @click.ctrl.exact="toggleSelect(f)" @click.shift.exact="shiftSelect(f)"
          @dblclick="handleOpen(f)" @contextmenu.prevent.stop="onContext($event, f)"
          @mousedown.left.stop="onRowMouseDown($event, f)">
          <el-icon :size="40" :color="f.is_dir ? 'var(--lp-warning)' : fileIconColor(f.name)">
            <Folder v-if="f.is_dir" /><Document v-else />
          </el-icon>
          <span class="fm-grid-name" :title="f.name">{{ f.name }}</span>
        </div>
        <div v-if="!loading && visibleFiles.length === 0" class="fm-empty" style="grid-column:1/-1">
          <el-empty :description="t('fm.emptyDir')" :image-size="64" />
        </div>
      </div>

      <div v-if="dragSelecting" class="fm-drag-rect" :style="dragRectStyle"></div>
      <transition name="fade">
        <div v-if="uploadDragActive" class="fm-upload-dropzone">
          <el-icon :size="28"><UploadFilled /></el-icon>
          <span>{{ t('fm.dropToUpload') }}</span>
        </div>
      </transition>

      <div v-if="termVisible" class="fm-terminal" :style="{ height: termHeight + 'px' }">
        <div class="fm-term-resize" @mousedown.prevent="startTermResize"></div>
        <div class="fm-term-header">
          <span class="fm-term-title"><el-icon :size="14"><Monitor /></el-icon> {{ t('terminal') }}</span>
          <span class="fm-term-cwd">{{ currentPath }}</span>
          <span class="fm-term-status" :class="{ online: termConnected }">
            {{ termConnected ? t('fm.termConnected') : t('fm.termDisconnected') }}
          </span>
          <span class="fm-term-follow">
            <el-checkbox v-model="termFollowPath" size="small">{{ t('fm.termFollowDir') }}</el-checkbox>
          </span>
          <div class="fm-term-actions">
            <el-tooltip :content="t('fm.reconnectTerminal')" :show-after="300">
              <el-icon class="fm-term-action" :size="14" @click="reconnectTerminal"><RefreshRight /></el-icon>
            </el-tooltip>
            <el-icon class="fm-term-close" @click="closeTerminal" :size="14"><Close /></el-icon>
          </div>
        </div>
        <div ref="termContainerRef" class="fm-term-xterm"></div>
      </div>
    </div>

    <!-- Context menus -->
    <teleport to="body">
      <transition name="fade">
        <div v-if="ctxVisible" class="fm-ctx-menu" :style="{ left: ctxX + 'px', top: ctxY + 'px' }">
          <div class="fm-ctx-item" @click="ctxAction('open')"><el-icon><FolderOpened /></el-icon> {{ t('open') }}</div>
          <div class="fm-ctx-item" @click="ctxAction('rename')"><el-icon><Edit /></el-icon> {{ t('rename') }}</div>
          <div class="fm-ctx-item" @click="ctxAction('copy')"><el-icon><CopyDocument /></el-icon> {{ t('fm.copyPath') }}</div>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item" @click="ctxAction('terminal')"><el-icon><Monitor /></el-icon> {{ t('fm.openTerminal') }}</div>
          <div class="fm-ctx-item" @click="ctxAction('chmod')"><el-icon><Lock /></el-icon> {{ t('fm.chmod') }}</div>
          <div class="fm-ctx-item" @click="ctxAction('props')"><el-icon><InfoFilled /></el-icon> {{ t('fm.properties') }}</div>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item fm-ctx-danger" @click="ctxAction('delete')"><el-icon><Delete /></el-icon> {{ t('delete') }}</div>
        </div>
      </transition>
    </teleport>
    <teleport to="body">
      <transition name="fade">
        <div v-if="bgCtxVisible" class="fm-ctx-menu" :style="{ left: bgCtxX + 'px', top: bgCtxY + 'px' }">
          <div class="fm-ctx-item" @click="bgCtxAction('newFile')"><el-icon><DocumentAdd /></el-icon> {{ t('fm.newFile') }}</div>
          <div class="fm-ctx-item" @click="bgCtxAction('newDir')"><el-icon><FolderAdd /></el-icon> {{ t('fm.newFolder') }}</div>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item" @click="bgCtxAction('terminal')"><el-icon><Monitor /></el-icon> {{ t('fm.openInTerminal') }}</div>
          <div class="fm-ctx-item" @click="bgCtxAction('refresh')"><el-icon><RefreshRight /></el-icon> {{ t('refresh') }}</div>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item" @click="bgCtxAction('selectAll')"><el-icon><Select /></el-icon> {{ t('selectAll') }}</div>
        </div>
      </transition>
    </teleport>

    <!-- Dialogs -->
    <el-dialog v-model="showEditor" :title="editingFile" width="75%" top="3vh" destroy-on-close v-drag :modal="false" :close-on-click-modal="false">
      <div class="fm-editor-wrap">
        <textarea v-model="editContent" class="fm-editor" spellcheck="false" @keydown.tab.prevent="insertTab"></textarea>
      </div>
      <template #footer>
        <span class="fm-editor-info">{{ editContent.split('\n').length }} {{ t('fm.lines') }} {{ fmtSize(editContent.length) }}</span>
        <el-button @click="showEditor = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="saveFile">{{ t('save') }}</el-button>
      </template>
    </el-dialog>
    <el-dialog v-model="showMkdir" :title="t('fm.newFolder')" width="400px" v-drag :modal="false" :close-on-click-modal="false">
      <el-input v-model="newDirName" :placeholder="t('fm.folderName')" @keyup.enter="doMkdir" autofocus />
      <template #footer><el-button @click="showMkdir = false">{{ t('cancel') }}</el-button><el-button type="primary" @click="doMkdir">{{ t('create') }}</el-button></template>
    </el-dialog>
    <el-dialog v-model="showNewFile" :title="t('fm.newFile')" width="400px" v-drag :modal="false" :close-on-click-modal="false">
      <el-input v-model="newFileName" :placeholder="t('fm.fileName')" @keyup.enter="doNewFile" autofocus />
      <template #footer><el-button @click="showNewFile = false">{{ t('cancel') }}</el-button><el-button type="primary" @click="doNewFile">{{ t('create') }}</el-button></template>
    </el-dialog>
    <el-dialog v-model="showRename" :title="t('rename')" width="400px" v-drag :modal="false" :close-on-click-modal="false">
      <el-input v-model="renameTo" @keyup.enter="doRename" autofocus />
      <template #footer><el-button @click="showRename = false">{{ t('cancel') }}</el-button><el-button type="primary" @click="doRename">{{ t('ok') }}</el-button></template>
    </el-dialog>
    <el-dialog v-model="showChmod" :title="t('fm.chmodTitle')" width="400px" v-drag :modal="false" :close-on-click-modal="false">
      <div style="margin-bottom:8px;color:var(--lp-text-secondary);font-size:13px">{{ chmodPath }}</div>
      <el-input v-model="chmodMode" placeholder="755" @keyup.enter="doChmod" autofocus />
      <template #footer><el-button @click="showChmod = false">{{ t('cancel') }}</el-button><el-button type="primary" @click="doChmod">{{ t('ok') }}</el-button></template>
    </el-dialog>
    <el-dialog v-model="showProps" :title="t('fm.fileProps')" width="450px" v-drag :modal="false" :close-on-click-modal="false">
      <el-descriptions :column="1" border size="small">
        <el-descriptions-item :label="t('fm.name')">{{ propsData.name }}</el-descriptions-item>
        <el-descriptions-item :label="t('fm.path')">{{ propsData.path }}</el-descriptions-item>
        <el-descriptions-item :label="t('fm.type')">{{ propsData.is_dir ? t('fm.folder') : t('fm.file') }}</el-descriptions-item>
        <el-descriptions-item :label="t('fm.size')">{{ fmtSize(propsData.size || 0) }}</el-descriptions-item>
        <el-descriptions-item :label="t('fm.modified')">{{ propsData.modified }}</el-descriptions-item>
        <el-descriptions-item :label="t('fm.permissions')">{{ propsData.permissions }}</el-descriptions-item>
      </el-descriptions>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { useRoute } from 'vue-router'
import { Terminal } from 'xterm'
import { FitAddon } from '@xterm/addon-fit'
import 'xterm/css/xterm.css'
import { listFiles, getFileRoots, readFileContent, writeFileContent, uploadFilesApi, mkdirApi, deletePathApi, renamePathApi, fileInfo, chmodApi } from '@/api'
import { useI18n } from '@/i18n'
import { RefreshRight, FolderAdd, DocumentAdd, Upload, UploadFilled, Back, Right, Top, ArrowRight, Close, CopyDocument, Grid, List, Delete, Lock, InfoFilled, Select, View, Hide } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'

const { t } = useI18n()

const props = defineProps<{ initPath?: string; pickerMode?: boolean }>()
const emit = defineEmits<{ (e: 'select', path: string): void }>()

type FileRoot = { path: string; label: string }

const route = useRoute()
const routePath = computed(() => {
  const p = route.query.path
  return typeof p === 'string' ? p : ''
})
const currentPath = ref(normPath(props.initPath || routePath.value || '/'))
const files = ref<any[]>([])
const loading = ref(false)
const viewMode = ref<'list' | 'grid'>('list')

const selectedSet = reactive(new Set<string>())
const lastClicked = ref('')

const navHistory = ref<string[]>([currentPath.value])
const navIndex = ref(0)
const canGoBack = computed(() => navIndex.value > 0)
const canGoForward = computed(() => navIndex.value < navHistory.value.length - 1)
const isWindowsHost = ref(false)
const fileRoots = ref<FileRoot[]>([])
const windowsDriveRoots = computed(() => fileRoots.value.filter((item) => isWindowsPath(item.path)))
const WINDOWS_ROOTS_VIEW = '__LINXPANEL_WINDOWS_DRIVES__'
const SHOW_HIDDEN_STORAGE_KEY = 'fm_show_hidden_files'
const showHiddenFiles = ref(localStorage.getItem(SHOW_HIDDEN_STORAGE_KEY) === '1')

const editingPath = ref(false)
const pathInputVal = ref('')
const pathInputRef = ref<HTMLInputElement>()
const uploadInputRef = ref<HTMLInputElement>()
const uploadingFiles = ref(false)
const uploadDragActive = ref(false)
let uploadDragDepth = 0

const showEditor = ref(false)
const editingFile = ref('')
const editContent = ref('')
const saving = ref(false)
const showMkdir = ref(false)
const newDirName = ref('')
const showNewFile = ref(false)
const newFileName = ref('')
const showRename = ref(false)
const renameFrom = ref('')
const renameTo = ref('')
const showChmod = ref(false)
const chmodPath = ref('')
const chmodMode = ref('755')
const showProps = ref(false)
const propsData = ref<any>({})

const ctxVisible = ref(false)
const ctxX = ref(0)
const ctxY = ref(0)
const ctxFile = ref<any>(null)
const bgCtxVisible = ref(false)
const bgCtxX = ref(0)
const bgCtxY = ref(0)

const termVisible = ref(false)
const termConnected = ref(false)
const TERM_FOLLOW_STORAGE_KEY = 'fm_term_follow_dir'
const termFollowPath = ref(localStorage.getItem(TERM_FOLLOW_STORAGE_KEY) !== '0')
const termHeight = ref(240)
const termContainerRef = ref<HTMLElement>()
const TERM_MIN_HEIGHT = 140
let termResizeStartY = 0
let termResizeStartHeight = 240
let termSocket: WebSocket | null = null
let termInstance: Terminal | null = null
let termFitAddon: FitAddon | null = null
let termResizeObserver: ResizeObserver | null = null
let termThemeObserver: MutationObserver | null = null
let termReconnectTimer: number | null = null
let termDisableAutoReconnect = false
let termApplyingSyncedPath = false

const fmBodyRef = ref<HTMLElement>()
const scrollRef = ref<HTMLElement>()
const listRef = ref<HTMLElement>()
const gridRef = ref<HTMLElement>()

const dragSelecting = ref(false)
const dragStartX = ref(0)
const dragStartY = ref(0)
const dragCurX = ref(0)
const dragCurY = ref(0)
let preSelectSnapshot = new Set<string>()

const dragRectStyle = computed(() => {
  const x1 = Math.min(dragStartX.value, dragCurX.value)
  const y1 = Math.min(dragStartY.value, dragCurY.value)
  const x2 = Math.max(dragStartX.value, dragCurX.value)
  const y2 = Math.max(dragStartY.value, dragCurY.value)
  return { left: x1 + 'px', top: y1 + 'px', width: (x2 - x1) + 'px', height: (y2 - y1) + 'px' }
})

function isWindowsPath(path: string) {
  return /^\/?[A-Za-z]:(?:\/|$)/.test(path.trim())
}

function normPath(path: string) {
  const raw = (path || '').trim()
  if (raw === WINDOWS_ROOTS_VIEW || raw === `/${WINDOWS_ROOTS_VIEW}`) {
    return WINDOWS_ROOTS_VIEW
  }
  let normalized = (path || '').trim().replace(/\\/g, '/').replace(/\/+/g, '/')
  if (!normalized) return '/'

  if (normalized.startsWith('/') && isWindowsPath(normalized)) {
    normalized = normalized.slice(1)
  }
  if (/^[A-Za-z]:$/.test(normalized)) {
    normalized = `${normalized}/`
  }
  if (/^[A-Za-z]:\//.test(normalized)) {
    normalized = `${normalized[0].toUpperCase()}${normalized.slice(1)}`
    if (normalized.length > 3 && normalized.endsWith('/')) {
      normalized = normalized.slice(0, -1)
    }
    return normalized
  }

  if (!normalized.startsWith('/')) normalized = '/' + normalized
  if (normalized.length > 1 && normalized.endsWith('/')) normalized = normalized.slice(0, -1)
  return normalized
}

function getDriveRoot(path: string) {
  const normalized = normPath(path)
  const matched = normalized.match(/^([A-Za-z]):\//)
  if (matched) return `${matched[1].toUpperCase()}:/`
  const fallback = windowsDriveRoots.value[0]?.path || 'C:/'
  return normPath(fallback)
}

const useWindowsPathStyle = computed(() =>
  (isWindowsHost.value && !isVirtualDriveView.value) || isWindowsPath(currentPath.value),
)
const isVirtualDriveView = computed(() => currentPath.value === WINDOWS_ROOTS_VIEW)
const rootPath = computed(() => {
  if (isVirtualDriveView.value) return WINDOWS_ROOTS_VIEW
  return useWindowsPathStyle.value ? getDriveRoot(currentPath.value) : '/'
})
const rootLabel = computed(() => (isVirtualDriveView.value ? t('fm.allDrives') : rootPath.value))
const currentDriveRoot = computed(() => {
  if (useWindowsPathStyle.value) return getDriveRoot(currentPath.value)
  const fallback = windowsDriveRoots.value[0]?.path || 'C:/'
  return normPath(fallback)
})
const atPathRoot = computed(() => isVirtualDriveView.value || normPath(currentPath.value) === normPath(rootPath.value))
const pathIconTooltip = computed(() => (isWindowsHost.value ? t('fm.showDrives') : t('fm.goRoot')))
const pathSegments = computed(() => {
  if (isVirtualDriveView.value) return []
  const normalized = normPath(currentPath.value)
  if (useWindowsPathStyle.value) {
    const root = getDriveRoot(normalized)
    const rest = normalized.startsWith(root) ? normalized.slice(root.length) : ''
    return rest.split('/').filter(Boolean)
  }
  return normalized.split('/').filter(Boolean)
})

function segmentPath(index: number) {
  if (isVirtualDriveView.value) return WINDOWS_ROOTS_VIEW
  const segments = pathSegments.value.slice(0, index + 1)
  if (useWindowsPathStyle.value) {
    return segments.length ? `${rootPath.value}${segments.join('/')}` : rootPath.value
  }
  return '/' + segments.join('/')
}

function parentPath(path: string) {
  if (path === WINDOWS_ROOTS_VIEW) return WINDOWS_ROOTS_VIEW
  const normalized = normPath(path)
  if (isWindowsPath(normalized)) {
    const driveRoot = getDriveRoot(normalized)
    if (normalized === driveRoot) return driveRoot
    const index = normalized.lastIndexOf('/')
    if (index <= 2) return driveRoot
    return normalized.slice(0, index)
  }
  if (normalized === '/') return '/'
  const index = normalized.lastIndexOf('/')
  if (index <= 0) return '/'
  return normalized.slice(0, index)
}

function fileNameOfPath(path: string) {
  if (path === WINDOWS_ROOTS_VIEW) return t('fm.allDrives')
  const normalized = normPath(path)
  if (isWindowsPath(normalized)) {
    const driveRoot = getDriveRoot(normalized)
    if (normalized === driveRoot) return driveRoot.replace('/', '')
    const index = normalized.lastIndexOf('/')
    return index >= 0 ? normalized.slice(index + 1) : normalized
  }
  if (normalized === '/') return '/'
  const index = normalized.lastIndexOf('/')
  return index >= 0 ? normalized.slice(index + 1) : normalized
}

function onPathIconClick() {
  if (isWindowsHost.value && windowsDriveRoots.value.length > 0) {
    navigate(WINDOWS_ROOTS_VIEW)
    return
  }
  goTo('/')
}

function toggleHiddenFiles() {
  showHiddenFiles.value = !showHiddenFiles.value
  localStorage.setItem(SHOW_HIDDEN_STORAGE_KEY, showHiddenFiles.value ? '1' : '0')
}

function isHiddenFileEntry(item: any) {
  const name = String(item?.name || '')
  if (!name || item?.is_virtual_drive) return false
  return name.startsWith('.')
}

const visibleFiles = computed(() => {
  if (isVirtualDriveView.value) return files.value
  if (showHiddenFiles.value) return files.value
  return files.value.filter((item) => !isHiddenFileEntry(item))
})

function triggerUpload() {
  if (isVirtualDriveView.value) {
    ElMessage.warning(t('fm.uploadNeedDir'))
    return
  }
  uploadInputRef.value?.click()
}

async function uploadPickedFiles(fileList: File[] | FileList) {
  const filesToUpload = Array.from(fileList || [])
  if (filesToUpload.length === 0) return
  if (isVirtualDriveView.value) {
    ElMessage.warning(t('fm.uploadNeedDir'))
    return
  }

  uploadingFiles.value = true
  try {
    const res: any = await uploadFilesApi(currentPath.value, filesToUpload)
    if (res.success) {
      const uploadedCount = Number(res.data?.uploaded || filesToUpload.length)
      ElMessage.success(t('fm.uploadedN', { n: uploadedCount }))
      await loadFiles()
    } else {
      ElMessage.error(res.message)
    }
  } finally {
    uploadingFiles.value = false
    if (uploadInputRef.value) uploadInputRef.value.value = ''
  }
}

async function onUploadInputChange(event: Event) {
  const input = event.target as HTMLInputElement
  const filesToUpload = input.files ? Array.from(input.files) : []
  input.value = ''
  await uploadPickedFiles(filesToUpload)
}

function hasDraggedFiles(event: DragEvent) {
  return Array.from(event.dataTransfer?.types || []).includes('Files')
}

function getDroppedFiles(event: DragEvent) {
  const dataTransfer = event.dataTransfer
  if (!dataTransfer) return [] as File[]
  if (dataTransfer.items?.length) {
    return Array.from(dataTransfer.items)
      .filter((item) => item.kind === 'file')
      .map((item) => item.getAsFile())
      .filter((file): file is File => !!file)
  }
  return Array.from(dataTransfer.files || [])
}

function resetUploadDragState() {
  uploadDragDepth = 0
  uploadDragActive.value = false
}

function onUploadDragEnter(event: DragEvent) {
  if (!hasDraggedFiles(event)) return
  uploadDragDepth += 1
  if (!isVirtualDriveView.value) {
    uploadDragActive.value = true
  }
}

function onUploadDragOver(event: DragEvent) {
  if (!hasDraggedFiles(event)) return
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = isVirtualDriveView.value ? 'none' : 'copy'
  }
  if (!isVirtualDriveView.value) {
    uploadDragActive.value = true
  }
}

function onUploadDragLeave(event: DragEvent) {
  if (!hasDraggedFiles(event)) return
  uploadDragDepth = Math.max(0, uploadDragDepth - 1)
  if (uploadDragDepth === 0) {
    uploadDragActive.value = false
  }
}

async function onUploadDrop(event: DragEvent) {
  if (!hasDraggedFiles(event)) return
  resetUploadDragState()
  if (isVirtualDriveView.value) {
    ElMessage.warning(t('fm.uploadNeedDir'))
    return
  }
  await uploadPickedFiles(getDroppedFiles(event))
}

async function loadFileRoots() {
  try {
    const res: any = await getFileRoots()
    if (!res.success) return
    isWindowsHost.value = !!res.data?.is_windows
    fileRoots.value = Array.isArray(res.data?.roots)
      ? res.data.roots.map((item: FileRoot) => ({ ...item, path: normPath(item.path) }))
      : []
  } catch {
    isWindowsHost.value = isWindowsPath(currentPath.value)
  }

  if (
    isWindowsHost.value &&
    windowsDriveRoots.value.length > 0 &&
    !isWindowsPath(currentPath.value) &&
    currentPath.value !== WINDOWS_ROOTS_VIEW
  ) {
    const firstDrive = normPath(windowsDriveRoots.value[0].path)
    currentPath.value = firstDrive
    navHistory.value = [firstDrive]
    navIndex.value = 0
  }
}

async function loadFiles() {
  loading.value = true
  selectedSet.clear()
  try {
    if (isVirtualDriveView.value) {
      files.value = windowsDriveRoots.value.map((root) => ({
        name: root.label,
        path: normPath(root.path),
        is_dir: true,
        size: 0,
        modified: '-',
        permissions: '-',
        is_virtual_drive: true,
      }))
      return
    }
    const res: any = await listFiles(currentPath.value)
    if (res.success) files.value = res.data
    else ElMessage.error(res.message)
  } catch { ElMessage.error(t('fm.loadFailed')) }
  finally { loading.value = false }
}

function navigate(p: string) {
  const np = p === WINDOWS_ROOTS_VIEW ? WINDOWS_ROOTS_VIEW : normPath(p || rootPath.value)
  if (np === currentPath.value) return
  currentPath.value = np
  navHistory.value = navHistory.value.slice(0, navIndex.value + 1)
  navHistory.value.push(np)
  navIndex.value = navHistory.value.length - 1
  void loadFiles()
}

function goTo(p: string) { navigate(p) }
function goUp() { navigate(parentPath(currentPath.value)) }
function goBack() { if (canGoBack.value) { navIndex.value--; currentPath.value = normPath(navHistory.value[navIndex.value]); void loadFiles() } }
function goForward() { if (canGoForward.value) { navIndex.value++; currentPath.value = normPath(navHistory.value[navIndex.value]); void loadFiles() } }
function refresh() { void loadFiles() }
function toggleView() { viewMode.value = viewMode.value === 'list' ? 'grid' : 'list' }

function selectOne(f: any) { selectedSet.clear(); selectedSet.add(f.path); lastClicked.value = f.path }
function toggleSelect(f: any) {
  if (selectedSet.has(f.path)) selectedSet.delete(f.path)
  else selectedSet.add(f.path)
  lastClicked.value = f.path
}
function shiftSelect(f: any) {
  if (!lastClicked.value) { selectOne(f); return }
  const paths = visibleFiles.value.map(x => x.path)
  const a = paths.indexOf(lastClicked.value), b = paths.indexOf(f.path)
  if (a < 0 || b < 0) return
  const [lo, hi] = a < b ? [a, b] : [b, a]
  for (let i = lo; i <= hi; i++) selectedSet.add(paths[i])
}
function selectAll() { visibleFiles.value.forEach(f => selectedSet.add(f.path)) }

async function batchDelete() {
  const count = selectedSet.size
  await ElMessageBox.confirm(t('fm.confirmDeleteN', { n: count }), t('confirm'), { type: 'warning' })
  let ok = 0
  for (const p of selectedSet) { try { const r: any = await deletePathApi(p); if (r.success) ok++ } catch {} }
  ElMessage.success(t('fm.deletedN', { n: ok }))
  selectedSet.clear(); loadFiles()
}

function onBodyMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  if (editingPath.value) {
    editingPath.value = false
    pathInputRef.value?.blur()
    return
  }
  const tgt = e.target as HTMLElement
  if (tgt.closest('.fm-row') || tgt.closest('.fm-grid-item') || tgt.closest('.fm-toolbar') || tgt.closest('.fm-terminal') || tgt.closest('.fm-list-header')) return
  e.preventDefault()
  if (!e.ctrlKey) selectedSet.clear()
  preSelectSnapshot = new Set(selectedSet)
  dragStartX.value = e.clientX; dragStartY.value = e.clientY
  dragCurX.value = e.clientX; dragCurY.value = e.clientY
  dragSelecting.value = true
  document.addEventListener('mousemove', onDragMove)
  document.addEventListener('mouseup', onDragEnd)
}

function onRowMouseDown(_e: MouseEvent, _f: any) { /* handled by click */ }

function onDragMove(e: MouseEvent) {
  dragCurX.value = e.clientX; dragCurY.value = e.clientY
  updateDragSelection()
}
function onDragEnd() {
  dragSelecting.value = false
  document.removeEventListener('mousemove', onDragMove)
  document.removeEventListener('mouseup', onDragEnd)
}
function updateDragSelection() {
  const rx1 = Math.min(dragStartX.value, dragCurX.value)
  const ry1 = Math.min(dragStartY.value, dragCurY.value)
  const rx2 = Math.max(dragStartX.value, dragCurX.value)
  const ry2 = Math.max(dragStartY.value, dragCurY.value)
  selectedSet.clear()
  preSelectSnapshot.forEach(p => selectedSet.add(p))
  const container = scrollRef.value
  if (!container) return
  container.querySelectorAll('[data-path]').forEach(el => {
    const r = el.getBoundingClientRect()
    if (r.right >= rx1 && r.left <= rx2 && r.bottom >= ry1 && r.top <= ry2) {
      const path = el.getAttribute('data-path')
      if (path) selectedSet.add(path)
    }
  })
}

function startEditPath() {
  editingPath.value = true
  pathInputVal.value = isVirtualDriveView.value ? currentDriveRoot.value : currentPath.value
  nextTick(() => { pathInputRef.value?.focus(); pathInputRef.value?.select() })
}
async function confirmPath() {
  editingPath.value = false
  const input = pathInputVal.value.trim()
  if (!input) return
  const np = normPath(input)
  try {
    const res: any = await fileInfo(np)
    if (res.success) {
      if (res.data.is_dir) { navigate(np) }
      else {
        const fileName = fileNameOfPath(np)
        const parentDir = parentPath(np)
        if (parentDir !== currentPath.value) {
          currentPath.value = parentDir
          navHistory.value = navHistory.value.slice(0, navIndex.value + 1)
          navHistory.value.push(parentDir)
          navIndex.value = navHistory.value.length - 1
          loading.value = true
          try {
            const lr: any = await listFiles(parentDir)
            if (lr.success) {
              files.value = lr.data
              selectedSet.clear()
              const match = files.value.find((f: any) => f.name === fileName)
              if (match) selectedSet.add(match.path)
            }
          } finally { loading.value = false }
        } else {
          const match = files.value.find((f: any) => f.name === fileName)
          if (match) { selectedSet.clear(); selectedSet.add(match.path) }
        }
      }
    } else {
      const fileName = fileNameOfPath(np)
      const parentDir = parentPath(np)
      try {
        const pr: any = await fileInfo(parentDir)
        if (pr.success && pr.data.is_dir) {
          navigate(parentDir)
          nextTick(() => ElMessage.warning(t('fm.fileNotExist') + ': ' + fileName))
        } else { ElMessage.error(t('fm.pathNotExist')) }
      } catch { ElMessage.error(t('fm.pathNotExist')) }
    }
  } catch { ElMessage.error(t('fm.pathNotExist')) }
}

function handleOpen(f: any) {
  if (f.is_dir) { if (props.pickerMode) { emit('select', f.path); return }; navigate(f.path) }
  else openEditor(f.path)
}

function onContext(e: MouseEvent, f: any) {
  if (f?.is_virtual_drive) return
  bgCtxVisible.value = false
  ctxX.value = e.clientX; ctxY.value = e.clientY; ctxFile.value = f; ctxVisible.value = true
  if (!selectedSet.has(f.path)) { selectedSet.clear(); selectedSet.add(f.path) }
}
function ctxAction(action: string) {
  ctxVisible.value = false
  const f = ctxFile.value; if (!f) return
  if (action === 'open') handleOpen(f)
  else if (action === 'rename') startRename(f)
  else if (action === 'delete') handleDelete(f)
  else if (action === 'copy') { navigator.clipboard?.writeText(f.path); ElMessage.success(t('fm.copied')) }
  else if (action === 'terminal') openTerminal()
  else if (action === 'chmod') { chmodPath.value = f.path; chmodMode.value = '755'; showChmod.value = true }
  else if (action === 'props') { propsData.value = { ...f }; showProps.value = true }
}

function onBodyContext(e: MouseEvent) {
  const tgt = e.target as HTMLElement
  if (tgt.closest('.fm-row') || tgt.closest('.fm-grid-item') || tgt.closest('.fm-toolbar') || tgt.closest('.fm-terminal')) return
  ctxVisible.value = false
  bgCtxX.value = e.clientX; bgCtxY.value = e.clientY; bgCtxVisible.value = true
}
function bgCtxAction(action: string) {
  bgCtxVisible.value = false
  if (action === 'newFile') showNewFile.value = true
  else if (action === 'newDir') showMkdir.value = true
  else if (action === 'terminal') openTerminal()
  else if (action === 'refresh') loadFiles()
  else if (action === 'selectAll') selectAll()
}

function closeAllCtx() { ctxVisible.value = false; bgCtxVisible.value = false }

async function openEditor(path: string) {
  try {
    const res: any = await readFileContent(path)
    if (res.success) { editingFile.value = path; editContent.value = res.data.content; showEditor.value = true }
    else ElMessage.error(res.message)
  } catch { ElMessage.error(t('fm.readFailed')) }
}
function insertTab(e: KeyboardEvent) {
  const ta = e.target as HTMLTextAreaElement
  const s = ta.selectionStart, end = ta.selectionEnd
  editContent.value = editContent.value.substring(0, s) + '  ' + editContent.value.substring(end)
  nextTick(() => { ta.selectionStart = ta.selectionEnd = s + 2 })
}
async function saveFile() {
  saving.value = true
  try {
    const res: any = await writeFileContent(editingFile.value, editContent.value)
    if (res.success) { ElMessage.success(t('fm.saved')); showEditor.value = false; loadFiles() }
    else ElMessage.error(res.message)
  } finally { saving.value = false }
}
async function doMkdir() {
  if (!newDirName.value.trim()) return
  const res: any = await mkdirApi(normPath(currentPath.value + '/' + newDirName.value.trim()))
  if (res.success) { ElMessage.success(t('fm.created')); showMkdir.value = false; newDirName.value = ''; loadFiles() }
  else ElMessage.error(res.message)
}
async function doNewFile() {
  if (!newFileName.value.trim()) return
  const res: any = await writeFileContent(normPath(currentPath.value + '/' + newFileName.value.trim()), '')
  if (res.success) { ElMessage.success(t('fm.created')); showNewFile.value = false; newFileName.value = ''; loadFiles() }
  else ElMessage.error(res.message)
}
function startRename(f: any) { renameFrom.value = f.path; renameTo.value = f.name; showRename.value = true }
async function doRename() {
  if (!renameTo.value.trim()) return
  const res: any = await renamePathApi(renameFrom.value, normPath(currentPath.value + '/' + renameTo.value.trim()))
  if (res.success) { ElMessage.success(t('fm.renamed')); showRename.value = false; loadFiles() }
  else ElMessage.error(res.message)
}
async function handleDelete(f: any) {
  await ElMessageBox.confirm(t('fm.confirmDeleteMsg', { name: f.name }), t('confirm'), { type: 'warning' })
  const res: any = await deletePathApi(f.path)
  if (res.success) { ElMessage.success(t('fm.deleted')); loadFiles() }
  else ElMessage.error(res.message)
}
async function doChmod() {
  if (!chmodMode.value.trim()) return
  const res: any = await chmodApi(chmodPath.value, chmodMode.value.trim())
  if (res.success) { ElMessage.success(t('fm.permChanged')); showChmod.value = false; loadFiles() }
  else ElMessage.error(res.message)
}

function cssVar(name: string, fallback: string) {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return value || fallback
}
function cleanupTermReconnectTimer() {
  if (termReconnectTimer !== null) {
    window.clearTimeout(termReconnectTimer)
    termReconnectTimer = null
  }
}
function scheduleTerminalReconnect() {
  if (!termVisible.value || termDisableAutoReconnect || termReconnectTimer !== null) return
  termReconnectTimer = window.setTimeout(() => {
    termReconnectTimer = null
    if (termVisible.value) connectTerminal(true)
  }, 1200)
}
function applyTerminalTheme() {
  if (!termInstance) return
  termInstance.options.theme = {
    background: cssVar('--lp-input-bg', '#0f1014'),
    foreground: cssVar('--lp-text-primary', '#d4d4d4'),
    cursor: cssVar('--lp-primary', '#4f6ef7'),
    cursorAccent: cssVar('--lp-card-bg', '#ffffff'),
    selectionBackground: cssVar('--lp-primary-bg', 'rgba(79,110,247,0.24)'),
    black: cssVar('--lp-text-secondary', '#7a7a7a'),
  }
}
function sendTerminalResize() {
  if (!termInstance || !termSocket || termSocket.readyState !== WebSocket.OPEN) return
  const cols = Math.max(20, termInstance.cols)
  const rows = Math.max(8, termInstance.rows)
  termSocket.send(JSON.stringify({ type: 'resize', cols, rows }))
}
function sendTerminalChdir(path: string) {
  if (!termFollowPath.value) return
  if (path === WINDOWS_ROOTS_VIEW || isVirtualDriveView.value) return
  if (!termSocket || termSocket.readyState !== WebSocket.OPEN) return
  termSocket.send(JSON.stringify({ type: 'chdir', path }))
}
function applySyncedTerminalPath(path: string) {
  if (!termFollowPath.value) return
  const targetPath = normPath(path)
  if (!targetPath || targetPath === currentPath.value) return
  termApplyingSyncedPath = true
  navigate(targetPath)
}
function fitTerminal() {
  if (!termFitAddon) return
  try {
    termFitAddon.fit()
    sendTerminalResize()
  } catch {}
}
function getTerminalCwdPath() {
  if (isVirtualDriveView.value) {
    if (isWindowsHost.value && windowsDriveRoots.value.length > 0) {
      return currentDriveRoot.value
    }
    return '/'
  }
  return currentPath.value
}
function buildTerminalWsUrl() {
  const endpoint = new URL('/api/files/terminal/ws', window.location.origin)
  endpoint.protocol = endpoint.protocol === 'https:' ? 'wss:' : 'ws:'
  endpoint.searchParams.set('cwd', getTerminalCwdPath())
  const token = localStorage.getItem('token')
  if (token) endpoint.searchParams.set('token', token)
  return endpoint.toString()
}
function disconnectTerminal(manual = true) {
  termDisableAutoReconnect = manual
  cleanupTermReconnectTimer()
  if (termSocket) {
    try { termSocket.close() } catch {}
    termSocket = null
  }
  termConnected.value = false
}
function ensureTerminal() {
  if (termInstance || !termContainerRef.value) return
  termInstance = new Terminal({
    allowTransparency: true,
    convertEol: true,
    cursorBlink: true,
    fontFamily: 'Cascadia Code, Fira Code, Consolas, monospace',
    fontSize: 12,
    lineHeight: 1.25,
    scrollback: 5000,
  })
  termFitAddon = new FitAddon()
  termInstance.loadAddon(termFitAddon)
  termInstance.open(termContainerRef.value)
  applyTerminalTheme()
  termInstance.focus()
  termInstance.onData((data) => {
    if (!termSocket || termSocket.readyState !== WebSocket.OPEN) return
    termSocket.send(data)
  })
  if (window.ResizeObserver) {
    termResizeObserver = new ResizeObserver(() => fitTerminal())
    termResizeObserver.observe(termContainerRef.value)
  }
  if (window.MutationObserver && !termThemeObserver) {
    termThemeObserver = new MutationObserver(() => applyTerminalTheme())
    termThemeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme', 'style'],
    })
  }
  nextTick(() => fitTerminal())
}
function connectTerminal(force = false) {
  if (!termVisible.value) return
  ensureTerminal()
  if (!termInstance) return
  if (!force && termSocket && (termSocket.readyState === WebSocket.OPEN || termSocket.readyState === WebSocket.CONNECTING)) {
    termInstance.focus()
    return
  }
  disconnectTerminal(false)
  termDisableAutoReconnect = false
  const ws = new WebSocket(buildTerminalWsUrl())
  ws.binaryType = 'arraybuffer'
  termSocket = ws
  ws.onopen = () => {
    if (termSocket !== ws) return
    termConnected.value = true
    cleanupTermReconnectTimer()
    applyTerminalTheme()
    fitTerminal()
    sendTerminalChdir(getTerminalCwdPath())
    termInstance?.focus()
  }
  ws.onmessage = async (event) => {
    if (!termInstance) return
    if (typeof event.data === 'string') {
      try {
        const payload = JSON.parse(event.data) as { type?: string; path?: string }
        if (payload?.type === 'cwd' && typeof payload.path === 'string') {
          applySyncedTerminalPath(payload.path)
          return
        }
      } catch {}
      termInstance.write(event.data)
      return
    }
    if (event.data instanceof ArrayBuffer) {
      termInstance.write(new Uint8Array(event.data))
      return
    }
    if (event.data instanceof Blob) {
      try {
        const buffer = await event.data.arrayBuffer()
        termInstance.write(new Uint8Array(buffer))
      } catch {}
    }
  }
  ws.onerror = () => {
    if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
      ws.close()
    }
  }
  ws.onclose = () => {
    if (termSocket === ws) termSocket = null
    termConnected.value = false
    if (!termDisableAutoReconnect) {
      scheduleTerminalReconnect()
    }
  }
}
function openTerminal() {
  termVisible.value = true
  nextTick(() => connectTerminal(false))
}
function closeTerminal() {
  termVisible.value = false
  disconnectTerminal(true)
}
function toggleTerminal() {
  if (termVisible.value) {
    closeTerminal()
    return
  }
  openTerminal()
}
function reconnectTerminal() {
  if (!termVisible.value) {
    openTerminal()
    return
  }
  connectTerminal(true)
}
function stopTermResize() {
  document.removeEventListener('mousemove', onTermResizeMove)
  document.removeEventListener('mouseup', stopTermResize)
  nextTick(() => fitTerminal())
}
function onTermResizeMove(e: MouseEvent) {
  const delta = termResizeStartY - e.clientY
  const maxHeight = Math.max(280, window.innerHeight - 180)
  termHeight.value = Math.min(maxHeight, Math.max(TERM_MIN_HEIGHT, termResizeStartHeight + delta))
  nextTick(() => fitTerminal())
}
function startTermResize(e: MouseEvent) {
  termResizeStartY = e.clientY
  termResizeStartHeight = termHeight.value
  document.addEventListener('mousemove', onTermResizeMove)
  document.addEventListener('mouseup', stopTermResize)
}

function fileIconColor(name: string) {
  const ext = name.split('.').pop()?.toLowerCase() || ''
  const m: Record<string, string> = {
    js:'#f7df1e',ts:'#3178c6',vue:'#42b883',py:'#3776ab',rs:'#dea584',
    html:'#e34f26',css:'#1572b6',scss:'#cc6699',json:'#8bc34a',md:'#42a5f5',
    sh:'#4eaa25',yml:'#cb171e',yaml:'#cb171e',toml:'#9c4121',
    sql:'#e38c00',xml:'#f16529',svg:'#ffb13b',
  }
  return m[ext] || 'var(--lp-info)'
}
function fmtSize(b: number) {
  if (!b) return '0 B'
  const u = ['B','KB','MB','GB','TB']
  const i = Math.floor(Math.log(b) / Math.log(1024))
  return (b / Math.pow(1024, i)).toFixed(1) + ' ' + u[i]
}

watch(
  () => route.query.path,
  (newPath) => {
    if (props.initPath) return
    if (typeof newPath !== 'string' || !newPath.trim()) return
    const targetPath = normPath(newPath)
    if (targetPath !== currentPath.value) navigate(targetPath)
  },
)
watch(
  currentPath,
  (nextPath, prevPath) => {
    if (nextPath === prevPath) return
    if (termApplyingSyncedPath) {
      termApplyingSyncedPath = false
      return
    }
    sendTerminalChdir(nextPath)
  },
)
watch(
  termFollowPath,
  (enabled) => {
    localStorage.setItem(TERM_FOLLOW_STORAGE_KEY, enabled ? '1' : '0')
    if (enabled) {
      sendTerminalChdir(currentPath.value)
    }
  },
)

onMounted(async () => {
  document.addEventListener('click', closeAllCtx)
  await loadFileRoots()
  await loadFiles()
})
onUnmounted(() => {
  document.removeEventListener('click', closeAllCtx)
  resetUploadDragState()
  stopTermResize()
  disconnectTerminal(true)
  cleanupTermReconnectTimer()
  if (termResizeObserver) {
    termResizeObserver.disconnect()
    termResizeObserver = null
  }
  if (termThemeObserver) {
    termThemeObserver.disconnect()
    termThemeObserver = null
  }
  if (termInstance) {
    termInstance.dispose()
    termInstance = null
  }
  termFitAddon = null
})
</script>

<style scoped lang="scss">
.file-manager { height: calc(100vh - 88px); display: flex; flex-direction: column; user-select: none; }
.fm-toolbar {
  display: flex; align-items: center; gap: 8px;
  padding: 8px 12px; background: var(--lp-card-bg);
  border-radius: var(--lp-radius-lg); margin-bottom: 8px;
  box-shadow: var(--lp-shadow-sm); flex-shrink: 0;
  border: 1px solid var(--lp-border-light);
  :deep(.el-button) {
    background: var(--lp-card-bg) !important;
    border-color: var(--lp-border) !important;
    color: var(--lp-text-secondary) !important;
    &:hover, &:focus { color: var(--lp-primary) !important; border-color: var(--lp-primary) !important; background: var(--lp-primary-bg) !important; }
    &.is-disabled {
      opacity: 0.85 !important;
      color: var(--lp-text-secondary) !important;
      background: var(--lp-table-header-bg) !important;
      border-color: var(--lp-border) !important;
    }
  }
  :deep(.el-divider) { border-color: var(--lp-border-light); }
}
.fm-nav { flex-shrink: 0; }
.fm-nav-btns {
  display: flex; align-items: center; gap: 2px;
}
.fm-nav-btn {
  width: 30px; height: 30px; padding: 0 !important;
  border-radius: 9px !important;
  box-shadow: 0 1px 0 rgba(0,0,0,0.02);
}
.fm-nav-btn.is-disabled {
  color: var(--lp-text-secondary) !important;
  background: var(--lp-table-header-bg) !important;
  border-color: var(--lp-border) !important;
}
.fm-path-bar {
  flex: 1; min-width: 0; display: flex; align-items: center; gap: 3px;
  padding: 4px 10px; background: var(--lp-input-bg);
  border: 1px solid var(--lp-border-light); border-radius: var(--lp-radius);
  cursor: text; font-size: 13px; height: 32px; overflow: hidden;
  transition: border-color var(--lp-transition);
  &:hover { border-color: var(--lp-border); }
}
.fm-path-icon { color: var(--lp-warning); flex-shrink: 0; margin-right: 4px; }
.fm-path-icon-clickable { cursor: pointer; }
.fm-path-root {
  color: var(--lp-text-secondary);
  cursor: pointer;
  padding: 2px 8px;
  border-radius: 8px;
  transition: color var(--lp-transition), background var(--lp-transition);
  &:hover { color: var(--lp-primary); background: var(--lp-primary-bg); }
}
.fm-path-arrow { color: var(--lp-text-placeholder); flex-shrink: 0; }
.fm-path-seg {
  color: var(--lp-text-secondary); cursor: pointer; white-space: nowrap;
  max-width: 138px; overflow: hidden; text-overflow: ellipsis;
  padding: 1px 4px; border-radius: 6px;
  transition: color var(--lp-transition), background var(--lp-transition);
  &:hover { color: var(--lp-primary); background: var(--lp-primary-bg); }
  &:last-child { color: var(--lp-text-primary); font-weight: 500; }
}
.fm-path-input {
  flex: 1; border: none; outline: none; font-size: 13px;
  background: transparent; color: var(--lp-text-primary);
  font-family: 'Cascadia Code', 'Fira Code', monospace;
}
.fm-actions { display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
.fm-upload-input { display: none; }
.fm-body { flex: 1; display: flex; flex-direction: column; min-height: 0; position: relative; }
.fm-list {
  flex: 1; background: var(--lp-card-bg); border-radius: var(--lp-radius-lg);
  box-shadow: var(--lp-shadow-sm); display: flex; flex-direction: column; min-height: 0;
  border: 1px solid var(--lp-border-light);
}
.fm-list-header {
  display: flex; padding: 10px 18px; font-size: 11px; font-weight: 600;
  color: var(--lp-text-secondary); text-transform: uppercase; letter-spacing: 0.5px;
  border-bottom: 1px solid var(--lp-border-light); flex-shrink: 0;
  background: var(--lp-table-header-bg);
  border-radius: var(--lp-radius-lg) var(--lp-radius-lg) 0 0;
}
.fm-list-body { flex: 1; overflow-y: auto; }
.fm-col-name { flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; }
.fm-col-size { width: 96px; text-align: right; flex-shrink: 0; }
.fm-col-time { width: 172px; text-align: right; flex-shrink: 0; }
.fm-col-perm { width: 118px; text-align: right; flex-shrink: 0; font-family: monospace; }
.fm-row {
  display: flex; align-items: center; padding: 9px 18px;
  font-size: 14px; cursor: default; transition: background 0.1s;
  border-bottom: 1px solid var(--lp-border-light);
  &:last-child { border-bottom: none; }
  &:hover { background: var(--lp-table-row-hover); }
  &.selected { background: var(--lp-primary-bg); }
  .fm-col-size, .fm-col-time, .fm-col-perm { color: var(--lp-text-secondary); font-size: 12px; }
}
.fm-fname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--lp-text-primary); font-weight: 500; }
.fm-empty { padding: 40px 0; text-align: center; }
.fm-grid {
  flex: 1; background: var(--lp-card-bg); border-radius: var(--lp-radius-lg);
  box-shadow: var(--lp-shadow-sm); border: 1px solid var(--lp-border-light);
  padding: 16px; overflow-y: auto;
  display: grid; grid-template-columns: repeat(auto-fill, minmax(100px, 1fr));
  gap: 8px; align-content: start;
}
.fm-grid-item {
  display: flex; flex-direction: column; align-items: center; gap: 6px;
  padding: 12px 8px; border-radius: var(--lp-radius);
  cursor: default; transition: background 0.1s;
  &:hover { background: var(--lp-table-row-hover); }
  &.selected { background: var(--lp-primary-bg); outline: 1px solid var(--lp-primary); }
}
.fm-grid-name {
  font-size: 12px; color: var(--lp-text-primary); text-align: center;
  max-width: 90px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.fm-drag-rect {
  position: fixed; border: 1px solid var(--lp-primary);
  background: rgba(79, 110, 247, 0.08); border-radius: 2px;
  pointer-events: none; z-index: 100;
}
.fm-upload-dropzone {
  position: absolute;
  inset: 10px;
  z-index: 120;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border-radius: var(--lp-radius-lg);
  border: 1px dashed var(--lp-primary);
  background: var(--lp-primary-bg);
  color: var(--lp-primary);
  font-size: 13px;
  font-weight: 600;
  pointer-events: none;
  backdrop-filter: blur(2px);
}
.fm-terminal {
  margin-top: 8px; background: var(--lp-card-bg); border-radius: var(--lp-radius-lg);
  display: flex; flex-direction: column; flex-shrink: 0; min-height: 140px;
  box-shadow: var(--lp-shadow-sm); overflow: hidden; border: 1px solid var(--lp-border-light);
}
.fm-term-resize {
  height: 8px; cursor: ns-resize; flex-shrink: 0;
  display: flex; align-items: center; justify-content: center;
  background: var(--lp-table-header-bg);
  &::before {
    content: ''; width: 40px; height: 3px; border-radius: 99px; background: var(--lp-border);
  }
}
.fm-term-header {
  display: flex; align-items: center; gap: 10px;
  padding: 6px 12px; background: var(--lp-table-header-bg); flex-shrink: 0;
  border-top: 1px solid var(--lp-border-light);
  border-bottom: 1px solid var(--lp-border-light);
}
.fm-term-title { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--lp-text-secondary); white-space: nowrap; }
.fm-term-cwd {
  flex: 1; min-width: 0; font-size: 12px;
  color: var(--lp-text-secondary); font-family: 'Cascadia Code', 'Fira Code', monospace;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.fm-term-status {
  font-size: 11px; line-height: 20px; height: 20px;
  padding: 0 8px; border-radius: 99px;
  border: 1px solid var(--lp-danger); color: var(--lp-danger);
  background: var(--lp-danger-bg);
  white-space: nowrap;
}
.fm-term-status.online {
  border-color: var(--lp-success); color: var(--lp-success); background: var(--lp-success-bg);
}
.fm-term-follow {
  display: inline-flex;
  align-items: center;
  :deep(.el-checkbox) { --el-checkbox-font-size: 12px; }
  :deep(.el-checkbox__label) { color: var(--lp-text-secondary); padding-left: 6px; }
}
.fm-term-actions { display: flex; align-items: center; gap: 8px; }
.fm-term-action,
.fm-term-close {
  cursor: pointer; color: var(--lp-text-secondary); border-radius: var(--lp-radius-sm);
  padding: 2px; transition: color var(--lp-transition), background var(--lp-transition);
  &:hover { color: var(--lp-primary); background: var(--lp-primary-bg); }
}
.fm-term-xterm {
  flex: 1; min-height: 0; padding: 8px 10px; background: var(--lp-input-bg);
}
.fm-term-xterm :deep(.xterm),
.fm-term-xterm :deep(.xterm-viewport),
.fm-term-xterm :deep(.xterm-screen) {
  width: 100%;
  height: 100%;
}
.fm-term-xterm :deep(.xterm) { padding: 0; }
.fm-term-xterm :deep(.xterm-viewport) { overflow-y: auto !important; }
.fm-term-xterm :deep(.xterm-cursor-layer) { z-index: 2; }
.fm-term-xterm :deep(.xterm-screen canvas) { image-rendering: auto; }
.fm-term-xterm :deep(.xterm .xterm-rows) {
  font-family: 'Cascadia Code', 'Fira Code', Consolas, monospace;
}
.fm-ctx-menu {
  position: fixed; z-index: 9999; min-width: 180px;
  background: var(--lp-popover-bg); border: 1px solid var(--lp-border);
  border-radius: var(--lp-radius); box-shadow: var(--lp-shadow-lg); padding: 4px 0;
}
.fm-ctx-item {
  display: flex; align-items: center; gap: 8px;
  padding: 7px 14px; font-size: 13px; color: var(--lp-text-primary);
  cursor: pointer; transition: background 0.1s;
  &:hover { background: var(--lp-primary-bg); color: var(--lp-primary); }
}
.fm-ctx-danger { &:hover { background: var(--lp-danger-bg) !important; color: var(--lp-danger) !important; } }
.fm-ctx-sep { height: 1px; background: var(--lp-border-light); margin: 4px 0; }
.fm-editor-wrap { height: 65vh; }
.fm-editor {
  width: 100%; height: 100%; resize: none;
  font-family: 'Cascadia Code', 'Fira Code', monospace;
  font-size: 13px; line-height: 1.6; padding: 12px;
  background: var(--lp-input-bg); color: var(--lp-text-primary);
  border: 1px solid var(--lp-input-border); border-radius: var(--lp-radius);
  outline: none; tab-size: 2;
  &:focus { border-color: var(--lp-primary); }
}
.fm-editor-info { float: left; font-size: 12px; color: var(--lp-text-secondary); line-height: 32px; }
</style>
