<template>
  <div class="file-manager">
    <div class="fm-toolbar">
      <div class="fm-nav">
        <el-button-group size="small">
          <el-button :icon="Back" :disabled="!canGoBack" @click="goBack" />
          <el-button :icon="Right" :disabled="!canGoForward" @click="goForward" />
          <el-button :icon="Top" :disabled="currentPath === '/'" @click="goUp" />
        </el-button-group>
      </div>
      <div class="fm-path-bar" @click="startEditPath">
        <template v-if="!editingPath">
          <el-icon class="fm-path-icon" :size="14"><FolderOpened /></el-icon>
          <span class="fm-path-root" @click.stop="goTo('/')">/</span>
          <template v-for="(seg, i) in pathSegments" :key="i">
            <el-icon :size="10" class="fm-path-arrow"><ArrowRight /></el-icon>
            <span class="fm-path-seg" @click.stop="goTo('/' + pathSegments.slice(0, i + 1).join('/'))">{{ seg }}</span>
          </template>
        </template>
        <input v-else ref="pathInputRef" v-model="pathInputVal" class="fm-path-input"
          @keyup.enter="confirmPath" @keyup.escape="editingPath = false" @blur="editingPath = false" />
      </div>
      <div class="fm-actions">
        <el-tooltip :content="t('refresh')" :show-after="400"><el-button size="small" :icon="RefreshRight" @click="refresh" /></el-tooltip>
        <el-tooltip :content="t('fm.newFolder')" :show-after="400"><el-button size="small" :icon="FolderAdd" @click="showMkdir = true" /></el-tooltip>
        <el-tooltip :content="t('fm.newFile')" :show-after="400"><el-button size="small" :icon="DocumentAdd" @click="showNewFile = true" /></el-tooltip>
        <el-divider direction="vertical" />
        <el-tooltip :content="viewMode === 'list' ? t('gridView') : t('listView')" :show-after="400">
          <el-button size="small" @click="toggleView"><el-icon :size="14"><Grid v-if="viewMode === 'list'" /><List v-else /></el-icon></el-button>
        </el-tooltip>
        <el-tooltip :content="t('terminal')" :show-after="400"><el-button size="small" @click="toggleTerminal"><el-icon :size="14"><Monitor /></el-icon></el-button></el-tooltip>
      </div>
    </div>

    <transition name="fade">
      <div v-if="selectedSet.size > 0" class="fm-selection-bar">
        <span>{{ t('fm.selected') }} {{ selectedSet.size }} {{ t('fm.items') }}</span>
        <el-button size="small" text @click="selectAll">{{ t('selectAll') }}</el-button>
        <el-button size="small" text @click="selectedSet.clear()">{{ t('cancel') }}</el-button>
        <el-divider direction="vertical" />
        <el-button size="small" type="danger" text :icon="Delete" @click="batchDelete">{{ t('delete') }}</el-button>
      </div>
    </transition>

    <div class="fm-body" ref="fmBodyRef" @mousedown="onBodyMouseDown" @contextmenu.prevent="onBodyContext">
      <div v-if="viewMode === 'list'" class="fm-list" ref="listRef">
        <div class="fm-list-header">
          <span class="fm-col-name">{{ t('fm.name') }}</span>
          <span class="fm-col-size">{{ t('fm.size') }}</span>
          <span class="fm-col-time">{{ t('fm.modified') }}</span>
          <span class="fm-col-perm">{{ t('fm.permissions') }}</span>
        </div>
        <div class="fm-list-body" ref="scrollRef" v-loading="loading">
          <div v-for="f in files" :key="f.path" class="fm-row"
            :class="{ selected: selectedSet.has(f.path) }" :data-path="f.path"
            @click.exact="selectOne(f)" @click.ctrl.exact="toggleSelect(f)" @click.shift.exact="shiftSelect(f)"
            @dblclick="handleOpen(f)" @contextmenu.prevent.stop="onContext($event, f)"
            @mousedown.left.stop="onRowMouseDown($event, f)">
            <span class="fm-col-name">
              <el-icon :size="16" :color="f.is_dir ? 'var(--lp-warning)' : fileIconColor(f.name)">
                <Folder v-if="f.is_dir" /><Document v-else />
              </el-icon>
              <span class="fm-fname">{{ f.name }}</span>
            </span>
            <span class="fm-col-size">{{ f.is_dir ? '-' : fmtSize(f.size) }}</span>
            <span class="fm-col-time">{{ f.modified }}</span>
            <span class="fm-col-perm">{{ f.permissions }}</span>
          </div>
          <div v-if="!loading && files.length === 0" class="fm-empty">
            <el-empty :description="t('fm.emptyDir')" :image-size="64" />
          </div>
        </div>
      </div>

      <div v-else class="fm-grid" ref="scrollRef" v-loading="loading">
        <div v-for="f in files" :key="f.path" class="fm-grid-item"
          :class="{ selected: selectedSet.has(f.path) }" :data-path="f.path"
          @click.exact="selectOne(f)" @click.ctrl.exact="toggleSelect(f)" @click.shift.exact="shiftSelect(f)"
          @dblclick="handleOpen(f)" @contextmenu.prevent.stop="onContext($event, f)"
          @mousedown.left.stop="onRowMouseDown($event, f)">
          <el-icon :size="40" :color="f.is_dir ? 'var(--lp-warning)' : fileIconColor(f.name)">
            <Folder v-if="f.is_dir" /><Document v-else />
          </el-icon>
          <span class="fm-grid-name" :title="f.name">{{ f.name }}</span>
        </div>
        <div v-if="!loading && files.length === 0" class="fm-empty" style="grid-column:1/-1">
          <el-empty :description="t('fm.emptyDir')" :image-size="64" />
        </div>
      </div>

      <div v-if="dragSelecting" class="fm-drag-rect" :style="dragRectStyle"></div>

      <div v-if="termVisible" class="fm-terminal">
        <div class="fm-term-header">
          <span class="fm-term-title"><el-icon :size="14"><Monitor /></el-icon> {{ t('terminal') }} {{ currentPath }}</span>
          <el-icon class="fm-term-close" @click="termVisible = false" :size="14"><Close /></el-icon>
        </div>
        <div class="fm-term-output" ref="termOutputRef">
          <div v-for="(line, i) in termHistory" :key="i" class="fm-term-line" :class="line.type">
            <span v-if="line.type === 'cmd'" class="fm-term-prompt">$ </span>
            <span v-html="line.text"></span>
          </div>
        </div>
        <div class="fm-term-input-row">
          <span class="fm-term-prompt">$</span>
          <input ref="termInputRef" v-model="termCmd" class="fm-term-input" :placeholder="t('fm.enterCmd')"
            @keyup.enter="runCmd" @keydown.up.prevent="historyUp" @keydown.down.prevent="historyDown" spellcheck="false" />
        </div>
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
import { listFiles, readFileContent, writeFileContent, mkdirApi, deletePathApi, renamePathApi, execCmd, fileInfo, chmodApi } from '@/api'
import { useI18n } from '@/i18n'
import { RefreshRight, FolderAdd, DocumentAdd, Back, Right, Top, ArrowRight, Close, CopyDocument, Grid, List, Delete, Lock, InfoFilled, Select } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'

const { t } = useI18n()

const props = defineProps<{ initPath?: string; pickerMode?: boolean }>()
const emit = defineEmits<{ (e: 'select', path: string): void }>()

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

const editingPath = ref(false)
const pathInputVal = ref('')
const pathInputRef = ref<HTMLInputElement>()

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
const termCmd = ref('')
const termHistory = ref<{ type: string; text: string }[]>([])
const termCmdHistory = ref<string[]>([])
const termHistIdx = ref(-1)
const termOutputRef = ref<HTMLElement>()
const termInputRef = ref<HTMLInputElement>()

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

const pathSegments = computed(() => currentPath.value.split('/').filter(Boolean))

function normPath(p: string) {
  let r = p.replace(/\\/g, '/').replace(/\/+/g, '/')
  if (!r.startsWith('/')) r = '/' + r
  if (r.length > 1 && r.endsWith('/')) r = r.slice(0, -1)
  return r
}

async function loadFiles() {
  loading.value = true
  selectedSet.clear()
  try {
    const res: any = await listFiles(currentPath.value)
    if (res.success) files.value = res.data
    else ElMessage.error(res.message)
  } catch { ElMessage.error(t('fm.loadFailed')) }
  finally { loading.value = false }
}

function navigate(p: string) {
  const np = normPath(p || '/')
  if (np === currentPath.value) return
  currentPath.value = np
  navHistory.value = navHistory.value.slice(0, navIndex.value + 1)
  navHistory.value.push(np)
  navIndex.value = navHistory.value.length - 1
  loadFiles()
}

function goTo(p: string) { navigate(p) }
function goUp() { navigate('/' + pathSegments.value.slice(0, -1).join('/')) }
function goBack() { if (canGoBack.value) { navIndex.value--; currentPath.value = navHistory.value[navIndex.value]; loadFiles() } }
function goForward() { if (canGoForward.value) { navIndex.value++; currentPath.value = navHistory.value[navIndex.value]; loadFiles() } }
function refresh() { loadFiles() }
function toggleView() { viewMode.value = viewMode.value === 'list' ? 'grid' : 'list' }

function selectOne(f: any) { selectedSet.clear(); selectedSet.add(f.path); lastClicked.value = f.path }
function toggleSelect(f: any) {
  if (selectedSet.has(f.path)) selectedSet.delete(f.path)
  else selectedSet.add(f.path)
  lastClicked.value = f.path
}
function shiftSelect(f: any) {
  if (!lastClicked.value) { selectOne(f); return }
  const paths = files.value.map(x => x.path)
  const a = paths.indexOf(lastClicked.value), b = paths.indexOf(f.path)
  if (a < 0 || b < 0) return
  const [lo, hi] = a < b ? [a, b] : [b, a]
  for (let i = lo; i <= hi; i++) selectedSet.add(paths[i])
}
function selectAll() { files.value.forEach(f => selectedSet.add(f.path)) }

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
  const tgt = e.target as HTMLElement
  if (tgt.closest('.fm-row') || tgt.closest('.fm-grid-item') || tgt.closest('.fm-toolbar') || tgt.closest('.fm-terminal') || tgt.closest('.fm-list-header') || tgt.closest('.fm-selection-bar')) return
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
  pathInputVal.value = currentPath.value
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
        const parts = np.split('/')
        const fileName = parts.pop()!
        const parentDir = parts.join('/') || '/'
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
      const parts = np.split('/')
      const fileName = parts.pop()!
      const parentDir = parts.join('/') || '/'
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
  else if (action === 'terminal') { termVisible.value = true; nextTick(() => termInputRef.value?.focus()) }
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
  else if (action === 'terminal') { termVisible.value = true; nextTick(() => termInputRef.value?.focus()) }
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

function toggleTerminal() { termVisible.value = !termVisible.value; if (termVisible.value) nextTick(() => termInputRef.value?.focus()) }
async function runCmd() {
  const cmd = termCmd.value.trim(); if (!cmd) return
  termCmdHistory.value.push(cmd); termHistIdx.value = termCmdHistory.value.length
  termHistory.value.push({ type: 'cmd', text: escHtml(cmd) }); termCmd.value = ''
  if (cmd.startsWith('cd ')) {
    const target = cmd.slice(3).trim()
    const np = target.startsWith('/') ? target : normPath(currentPath.value + '/' + target)
    navigate(np); termHistory.value.push({ type: 'info', text: np }); scrollTerm(); return
  }
  try {
    const res: any = await execCmd(cmd, currentPath.value)
    if (res.success) {
      if (res.data.stdout) termHistory.value.push({ type: 'out', text: escHtml(res.data.stdout) })
      if (res.data.stderr) termHistory.value.push({ type: 'err', text: escHtml(res.data.stderr) })
      if (res.data.exit_code !== 0) termHistory.value.push({ type: 'err', text: 'exit code: ' + res.data.exit_code })
    } else termHistory.value.push({ type: 'err', text: res.message })
  } catch (e: any) { termHistory.value.push({ type: 'err', text: e.message || 'Error' }) }
  loadFiles(); scrollTerm()
}
function historyUp() { if (termHistIdx.value > 0) { termHistIdx.value--; termCmd.value = termCmdHistory.value[termHistIdx.value] } }
function historyDown() {
  if (termHistIdx.value < termCmdHistory.value.length - 1) { termHistIdx.value++; termCmd.value = termCmdHistory.value[termHistIdx.value] }
  else { termHistIdx.value = termCmdHistory.value.length; termCmd.value = '' }
}
function scrollTerm() { nextTick(() => { if (termOutputRef.value) termOutputRef.value.scrollTop = termOutputRef.value.scrollHeight }) }
function escHtml(s: string) { return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;') }

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

onMounted(() => { loadFiles(); document.addEventListener('click', closeAllCtx) })
onUnmounted(() => { document.removeEventListener('click', closeAllCtx) })
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
    &.is-disabled { opacity: 0.4 !important; color: var(--lp-text-placeholder) !important; background: var(--lp-card-bg) !important; border-color: var(--lp-border-light) !important; }
  }
  :deep(.el-button-group .el-button) { border-color: var(--lp-border) !important; }
  :deep(.el-divider) { border-color: var(--lp-border-light); }
}
.fm-nav { flex-shrink: 0; }
.fm-path-bar {
  flex: 1; min-width: 0; display: flex; align-items: center; gap: 2px;
  padding: 4px 10px; background: var(--lp-input-bg);
  border: 1px solid var(--lp-border-light); border-radius: var(--lp-radius);
  cursor: text; font-size: 13px; height: 32px; overflow: hidden;
  transition: border-color var(--lp-transition);
  &:hover { border-color: var(--lp-border); }
}
.fm-path-icon { color: var(--lp-warning); flex-shrink: 0; margin-right: 4px; }
.fm-path-root { color: var(--lp-text-secondary); cursor: pointer; &:hover { color: var(--lp-primary); } }
.fm-path-arrow { color: var(--lp-text-placeholder); flex-shrink: 0; }
.fm-path-seg {
  color: var(--lp-text-secondary); cursor: pointer; white-space: nowrap;
  max-width: 120px; overflow: hidden; text-overflow: ellipsis;
  &:hover { color: var(--lp-primary); }
  &:last-child { color: var(--lp-text-primary); font-weight: 500; }
}
.fm-path-input {
  flex: 1; border: none; outline: none; font-size: 13px;
  background: transparent; color: var(--lp-text-primary);
  font-family: 'Cascadia Code', 'Fira Code', monospace;
}
.fm-actions { display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
.fm-selection-bar {
  display: flex; align-items: center; gap: 8px;
  padding: 6px 14px; margin-bottom: 8px;
  background: var(--lp-primary-bg); border: 1px solid var(--lp-primary);
  border-radius: var(--lp-radius); font-size: 13px; color: var(--lp-primary);
}
.fm-body { flex: 1; display: flex; flex-direction: column; min-height: 0; position: relative; }
.fm-list {
  flex: 1; background: var(--lp-card-bg); border-radius: var(--lp-radius-lg);
  box-shadow: var(--lp-shadow-sm); display: flex; flex-direction: column; min-height: 0;
  border: 1px solid var(--lp-border-light);
}
.fm-list-header {
  display: flex; padding: 8px 16px; font-size: 11px; font-weight: 600;
  color: var(--lp-text-secondary); text-transform: uppercase; letter-spacing: 0.5px;
  border-bottom: 1px solid var(--lp-border-light); flex-shrink: 0;
  background: var(--lp-table-header-bg);
  border-radius: var(--lp-radius-lg) var(--lp-radius-lg) 0 0;
}
.fm-list-body { flex: 1; overflow-y: auto; }
.fm-col-name { flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; }
.fm-col-size { width: 80px; text-align: right; flex-shrink: 0; }
.fm-col-time { width: 150px; text-align: right; flex-shrink: 0; }
.fm-col-perm { width: 100px; text-align: right; flex-shrink: 0; font-family: monospace; }
.fm-row {
  display: flex; align-items: center; padding: 4px 16px;
  font-size: 13px; cursor: default; transition: background 0.1s;
  border-bottom: 1px solid var(--lp-border-light);
  &:last-child { border-bottom: none; }
  &:hover { background: var(--lp-table-row-hover); }
  &.selected { background: var(--lp-primary-bg); }
  .fm-col-size, .fm-col-time, .fm-col-perm { color: var(--lp-text-secondary); font-size: 12px; }
}
.fm-fname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--lp-text-primary); }
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
.fm-terminal {
  margin-top: 8px; background: #0d0d0d; border-radius: var(--lp-radius-lg);
  display: flex; flex-direction: column; height: 240px; flex-shrink: 0;
  box-shadow: var(--lp-shadow-sm); overflow: hidden; border: 1px solid var(--lp-border-light);
}
.fm-term-header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 6px 12px; background: #1a1a1a; flex-shrink: 0;
}
.fm-term-title { display: flex; align-items: center; gap: 6px; font-size: 12px; color: #888; }
.fm-term-close { cursor: pointer; color: #666; &:hover { color: #fff; } }
.fm-term-output {
  flex: 1; overflow-y: auto; padding: 8px 12px;
  font-family: 'Cascadia Code', 'Fira Code', monospace; font-size: 12px; line-height: 1.6;
}
.fm-term-line {
  white-space: pre-wrap; word-break: break-all;
  &.cmd { color: #6af; } &.out { color: #ccc; } &.err { color: #f66; } &.info { color: #6c6; }
}
.fm-term-prompt { color: #6f6; margin-right: 4px; }
.fm-term-input-row { display: flex; align-items: center; padding: 6px 12px; border-top: 1px solid #222; flex-shrink: 0; }
.fm-term-input {
  flex: 1; border: none; outline: none; background: transparent;
  color: #eee; font-family: 'Cascadia Code', 'Fira Code', monospace; font-size: 12px;
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
