<template>
  <!-- A module's sub-pages as a row of buttons in the page header bar; the
       active one is filled. Scrolls sideways on narrow screens. -->
  <nav class="page-tabs" role="tablist">
    <button
      v-for="item in items"
      :key="item.name"
      type="button"
      role="tab"
      class="page-tabs__item"
      :class="{ 'is-active': item.name === modelValue }"
      :aria-selected="item.name === modelValue"
      @click="select(item.name)"
    >
      <el-icon v-if="item.icon" :size="15"><component :is="item.icon" /></el-icon>
      <span>{{ item.label }}</span>
      <span v-if="item.dot" class="page-tabs__dot" :class="`page-tabs__dot--${item.dot}`"></span>
      <span v-if="item.badge !== undefined && item.badge !== ''" class="page-tabs__badge">{{ item.badge }}</span>
    </button>
  </nav>
</template>

<script setup lang="ts">
export interface PageTab {
  name: string
  label: string
  icon?: any
  /** Status dot after the label: ok (green), warn (amber), off (grey). */
  dot?: 'ok' | 'warn' | 'off' | ''
  badge?: string | number
}

const props = defineProps<{ items: PageTab[]; modelValue: string }>()
const emit = defineEmits<{ 'update:modelValue': [name: string]; change: [name: string] }>()

function select(name: string) {
  if (name === props.modelValue) return
  emit('update:modelValue', name)
  emit('change', name)
}
</script>

<style scoped lang="scss">
.page-tabs {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  overflow-x: auto;
  scrollbar-width: none;
  &::-webkit-scrollbar { display: none; }
}
.page-tabs__item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 32px;
  padding: 0 14px;
  flex: none;
  border: 0;
  border-radius: var(--lp-r-sm);
  background: transparent;
  color: var(--lp-text-soft);
  font: inherit;
  font-size: var(--lp-text-base);
  white-space: nowrap;
  cursor: pointer;
  transition: background var(--lp-fast), color var(--lp-fast);

  &:hover { color: var(--lp-accent); background: var(--lp-accent-soft); }
  &.is-active,
  &.is-active:hover {
    background: var(--lp-accent);
    color: #fff;
    font-weight: 500;
  }
}

.page-tabs__dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--lp-text-faint);
  &--ok { background: var(--lp-ok); }
  &--warn { background: var(--lp-warn); }
}
.is-active .page-tabs__dot { box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.85); }

.page-tabs__badge {
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: var(--lp-r-full);
  background: var(--lp-surface-sunken);
  color: var(--lp-text-dim);
  font-size: 11px;
  line-height: 18px;
  text-align: center;
}
.is-active .page-tabs__badge { background: rgba(255, 255, 255, 0.22); color: inherit; }
</style>
