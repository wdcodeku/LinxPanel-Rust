<template>
  <div class="dir-picker">
    <div class="dp-toolbar">
      <div class="dp-path">
        <el-icon class="dp-home" @click="goTo('/')"><HomeFilled /></el-icon>
        <template v-for="(seg, i) in segs" :key="i">
          <span class="dp-sep">/</span>
          <span class="dp-seg" @click="goTo('/' + segs.slice(0, i+1).join('/'))">{{ seg }}</span>
        </template>
      </div>
      <div class="dp-actions">
        <el-tooltip :content="t('refresh')" :show-after="250">
          <el-button circle class="dp-action-btn" :icon="RefreshRight" @click="load" />
        </el-tooltip>
        <el-tooltip :content="t('fm.newFolder')" :show-after="250">
          <el-button circle class="dp-action-btn dp-action-primary" :icon="FolderAdd" @click="showNewDir = !showNewDir" />
        </el-tooltip>
      </div>
    </div>

    <div v-if="showNewDir" class="dp-create-row">
      <el-input v-model="newDirName" :placeholder="t('fm.folderName')" @keyup.enter="createDir" />
      <el-button type="primary" size="small" @click="createDir">{{ t('create') }}</el-button>
      <el-button size="small" @click="cancelCreateDir">{{ t('cancel') }}</el-button>
    </div>

    <div class="dp-list" v-loading="loading">
      <div class="dp-item" v-if="current !== '/'" @click="goUp">
        <el-icon :size="18" color="var(--lp-warning)"><Back /></el-icon>
        <span>..</span>
      </div>
      <div
        v-for="d in pagedDirs" :key="d.path"
        class="dp-item" :class="{ active: selectedDir === d.path }"
        @click="selectedDir = d.path"
        @dblclick="goTo(d.path)"
      >
        <el-icon :size="18" color="var(--lp-warning)"><Folder /></el-icon>
        <span>{{ d.name }}</span>
      </div>
      <div v-if="!loading && dirs.length === 0" class="dp-empty">{{ t('fm.noSubdirs') }}</div>
    </div>

    <div class="dp-pagination" v-if="dirs.length > pageSize">
      <el-pagination
        v-model:current-page="currentPage"
        v-model:page-size="pageSize"
        layout="total, sizes, prev, pager, next"
        :page-sizes="[20, 50, 100]"
        :total="dirs.length"
        size="small"
        background
      />
    </div>

    <div class="dp-footer">
      <el-input v-model="selectedDir" size="small" readonly />
      <el-button type="primary" size="small" @click="$emit('select', selectedDir)" :disabled="!selectedDir">
        {{ t('fm.selectDir') }}
      </el-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { listFiles, mkdirApi } from '@/api'
import { useI18n } from '@/i18n'
import { ElMessage } from 'element-plus'
import { FolderAdd, RefreshRight } from '@element-plus/icons-vue'

const props = defineProps<{ initPath?: string }>()
defineEmits<{ (e: 'select', path: string): void }>()
const { t } = useI18n()

function normPath(path: string) {
  let normalized = (path || '/').replace(/\\/g, '/').replace(/\/+/g, '/')
  if (!normalized.startsWith('/')) normalized = '/' + normalized
  if (normalized.length > 1 && normalized.endsWith('/')) normalized = normalized.slice(0, -1)
  return normalized
}
function joinPath(base: string, name: string) {
  return normPath(`${base}/${name}`)
}

const current = ref(normPath(props.initPath || '/'))
const items = ref<any[]>([])
const loading = ref(false)
const selectedDir = ref('')
const showNewDir = ref(false)
const newDirName = ref('')
const currentPage = ref(1)
const pageSize = ref(50)

const segs = computed(() => current.value.split('/').filter(Boolean))
const dirs = computed(() => items.value.filter((f: any) => f.is_dir))
const pagedDirs = computed(() => {
  const startIndex = (currentPage.value - 1) * pageSize.value
  return dirs.value.slice(startIndex, startIndex + pageSize.value)
})

watch([dirs, pageSize], () => {
  const maxPage = Math.max(1, Math.ceil(dirs.value.length / pageSize.value))
  if (currentPage.value > maxPage) currentPage.value = maxPage
})

async function load() {
  loading.value = true
  try {
    const res: any = await listFiles(current.value)
    if (res.success) {
      items.value = res.data
      currentPage.value = 1
    } else ElMessage.error(res.message)
  } finally { loading.value = false }
}

function goTo(p: string) {
  current.value = normPath(p)
  selectedDir.value = current.value
  showNewDir.value = false
  newDirName.value = ''
  load()
}

function goUp() {
  const s = segs.value.slice(0, -1)
  goTo('/' + s.join('/'))
}

function cancelCreateDir() {
  showNewDir.value = false
  newDirName.value = ''
}
async function createDir() {
  const name = newDirName.value.trim()
  if (!name) return
  if (/[\\/]/.test(name)) {
    ElMessage.warning(t('fm.folderNameInvalid'))
    return
  }
  const target = joinPath(current.value, name)
  const res: any = await mkdirApi(target)
  if (res.success) {
    ElMessage.success(t('fm.created'))
    selectedDir.value = target
    cancelCreateDir()
    await load()
  } else ElMessage.error(res.message)
}

onMounted(() => { selectedDir.value = current.value; load() })
</script>

<style scoped lang="scss">
.dir-picker {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.dp-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 10px;
  border-radius: var(--lp-radius-lg);
  border: 1px solid var(--lp-border);
  background: var(--lp-card-bg);
  box-shadow: var(--lp-shadow-sm);
}
.dp-path {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 13px;
  min-width: 0;
  overflow-x: auto;
  white-space: nowrap;
  padding: 2px 2px;
}
.dp-home { cursor: pointer; color: var(--lp-primary); }
.dp-sep { color: var(--lp-text-placeholder); }
.dp-seg { cursor: pointer; color: var(--lp-text-secondary); &:hover { color: var(--lp-primary); } }
.dp-seg:last-child { color: var(--lp-text-primary); font-weight: 500; }
.dp-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.dp-action-btn {
  border: 1px solid var(--lp-border) !important;
  background: var(--lp-card-bg) !important;
  color: var(--lp-text-secondary) !important;
  &:hover {
    border-color: var(--lp-primary) !important;
    background: var(--lp-primary-bg) !important;
    color: var(--lp-primary) !important;
  }
}
.dp-action-primary {
  border-color: rgba(79,110,247,0.28) !important;
  background: var(--lp-primary-bg) !important;
  color: var(--lp-primary) !important;
}
.dp-create-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px;
  border-radius: var(--lp-radius);
  border: 1px solid var(--lp-border-light);
  background: var(--lp-card-bg);
}
.dp-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  border: 1px solid var(--lp-border);
  border-radius: var(--lp-radius-lg);
  background: var(--lp-card-bg);
  box-shadow: var(--lp-shadow-sm);
}
.dp-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 12px;
  cursor: pointer;
  font-size: 13px;
  color: var(--lp-text-primary);
  transition: background 0.15s, color 0.15s;
  border-bottom: 1px solid var(--lp-border-light);
  &:last-child { border-bottom: 0; }
  &:hover { background: var(--lp-table-row-hover); }
  &.active { background: var(--lp-primary-bg); color: var(--lp-primary); }
  span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
}
.dp-empty { padding: 24px; text-align: center; color: var(--lp-text-placeholder); font-size: 13px; }
.dp-pagination {
  display: flex;
  justify-content: flex-end;
  padding: 2px 2px;
}
.dp-footer { display: flex; gap: 8px; align-items: center; }
</style>
