<template>
  <el-dialog
    :model-value="modelValue"
    :width="maximized ? '100%' : 'min(1100px, 96vw)'"
    :fullscreen="maximized"
    top="5vh"
    append-to-body
    destroy-on-close
    draggable
    class="term-dialog"
    :class="{ 'is-max': maximized }"
    @update:model-value="(v: boolean) => emit('update:modelValue', v)"
  >
    <template #header>
      <div class="td-head">
        <el-icon class="td-head__icon"><Monitor /></el-icon>
        <span class="td-head__title">{{ title || t('nav.terminal') }}</span>
        <span class="td-head__cwd lp-mono lp-truncate" :title="currentCwd">{{ currentCwd }}</span>
        <button type="button" class="td-btn" :title="maximized ? t('tm.restore') : t('tm.maximize')" @click="maximized = !maximized">
          <el-icon><component :is="maximized ? 'CopyDocument' : 'FullScreen'" /></el-icon>
        </button>
      </div>
    </template>
    <!-- destroy-on-close: closing the dialog ends the shell session. -->
    <div class="td-body" :class="{ 'is-max': maximized }">
      <TerminalPane :cwd="cwd" :show-bar="false" flat @update:cwd="(p: string) => (currentCwd = p)" />
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { Monitor } from '@element-plus/icons-vue'
import TerminalPane from '@/components/TerminalPane.vue'
import { useI18n } from '@/i18n'

const props = defineProps<{ modelValue: boolean; cwd?: string; title?: string }>()
const emit = defineEmits<{ (e: 'update:modelValue', v: boolean): void }>()
const { t } = useI18n()

const maximized = ref(false)
const currentCwd = ref(props.cwd || '')
watch(
  () => [props.modelValue, props.cwd] as const,
  ([open, cwd]) => {
    if (open) currentCwd.value = cwd || ''
  },
)
</script>

<style scoped lang="scss">
.td-head { display: flex; align-items: center; gap: 8px; min-width: 0; padding-right: 28px; }
.td-head__icon { color: var(--lp-accent); }
.td-head__title { font-weight: 600; white-space: nowrap; }
.td-head__cwd { flex: 1; min-width: 0; font-size: 12px; color: var(--lp-text-dim); }
.td-btn {
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  border: 0;
  border-radius: var(--lp-r-sm);
  background: transparent;
  color: var(--lp-text-dim);
  cursor: pointer;
  &:hover { background: var(--lp-muted-soft); color: var(--lp-text); }
}
.td-body { height: min(62vh, 640px); border: 1px solid var(--lp-line); border-radius: var(--lp-r-sm); overflow: hidden; }
.td-body.is-max { height: calc(100vh - 90px); }
</style>

<style lang="scss">
.term-dialog .el-dialog__body { padding-top: 4px; }
</style>
