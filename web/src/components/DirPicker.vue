<template>
  <div class="dir-picker">
    <div class="dp-path">
      <el-icon class="dp-home" @click="goTo('/')"><HomeFilled /></el-icon>
      <template v-for="(seg, i) in segs" :key="i">
        <span class="dp-sep">/</span>
        <span class="dp-seg" @click="goTo('/' + segs.slice(0, i+1).join('/'))">{{ seg }}</span>
      </template>
    </div>
    <div class="dp-list" v-loading="loading">
      <div class="dp-item" v-if="current !== '/'" @click="goUp">
        <el-icon :size="18" color="var(--lp-warning)"><Back /></el-icon>
        <span>..</span>
      </div>
      <div
        v-for="d in dirs" :key="d.path"
        class="dp-item" :class="{ active: selectedDir === d.path }"
        @click="selectedDir = d.path"
        @dblclick="goTo(d.path)"
      >
        <el-icon :size="18" color="var(--lp-warning)"><Folder /></el-icon>
        <span>{{ d.name }}</span>
      </div>
      <div v-if="!loading && dirs.length === 0" class="dp-empty">No subdirectories</div>
    </div>
    <div class="dp-footer">
      <el-input v-model="selectedDir" size="small" readonly />
      <el-button type="primary" size="small" @click="$emit('select', selectedDir)" :disabled="!selectedDir">Select</el-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { listFiles } from '@/api'

const props = defineProps<{ initPath?: string }>()
defineEmits<{ (e: 'select', path: string): void }>()

const current = ref(props.initPath || '/')
const items = ref<any[]>([])
const loading = ref(false)
const selectedDir = ref('')

const segs = computed(() => current.value.split('/').filter(Boolean))
const dirs = computed(() => items.value.filter((f: any) => f.is_dir))

async function load() {
  loading.value = true
  try {
    const res: any = await listFiles(current.value)
    if (res.success) items.value = res.data
  } finally { loading.value = false }
}

function goTo(p: string) {
  current.value = p.replace(/\/+/g, '/') || '/'
  selectedDir.value = current.value
  load()
}

function goUp() {
  const s = segs.value.slice(0, -1)
  goTo('/' + s.join('/'))
}

onMounted(() => { selectedDir.value = current.value; load() })
</script>

<style scoped lang="scss">
.dir-picker { display: flex; flex-direction: column; gap: 8px; }
.dp-path { display: flex; align-items: center; gap: 4px; font-size: 13px; padding: 4px 0; }
.dp-home { cursor: pointer; color: var(--lp-primary); }
.dp-sep { color: var(--lp-text-placeholder); }
.dp-seg { cursor: pointer; color: var(--lp-text-secondary); &:hover { color: var(--lp-primary); } }
.dp-seg:last-child { color: var(--lp-text-primary); font-weight: 500; }
.dp-list {
  max-height: 320px; overflow-y: auto;
  border: 1px solid var(--lp-border-light); border-radius: var(--lp-radius);
  background: var(--lp-input-bg);
}
.dp-item {
  display: flex; align-items: center; gap: 8px;
  padding: 7px 12px; cursor: pointer; font-size: 13px;
  color: var(--lp-text-primary); transition: background 0.15s;
  &:hover { background: var(--lp-table-row-hover); }
  &.active { background: var(--lp-primary-bg); color: var(--lp-primary); }
}
.dp-empty { padding: 24px; text-align: center; color: var(--lp-text-placeholder); font-size: 13px; }
.dp-footer { display: flex; gap: 8px; align-items: center; }
</style>