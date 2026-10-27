<template>
  <Teleport to="body">
    <transition name="palette">
      <div v-if="open" class="palette" @mousedown.self="close">
        <div class="palette__box" role="dialog" aria-modal="true" :aria-label="t('nav.search')">
          <div class="palette__search">
            <el-icon :size="17" class="palette__search-icon"><Search /></el-icon>
            <input
              ref="inputRef"
              v-model="query"
              class="palette__input"
              :placeholder="t('nav.searchPlaceholder')"
              spellcheck="false"
              autocomplete="off"
              @keydown.down.prevent="move(1)"
              @keydown.up.prevent="move(-1)"
              @keydown.enter.prevent="choose(results[cursor])"
              @keydown.esc.prevent="close"
            />
            <kbd class="palette__kbd">Esc</kbd>
          </div>
          <div ref="listRef" class="palette__list">
            <button
              v-for="(entry, index) in results"
              :key="entry.path"
              type="button"
              class="palette__item"
              :class="{ 'palette__item--on': index === cursor }"
              @mousemove="cursor = index"
              @click="choose(entry)"
            >
              <span class="palette__icon"><el-icon :size="16"><component :is="entry.icon" /></el-icon></span>
              <span class="palette__label">{{ entry.label }}</span>
              <span class="palette__group">{{ entry.group }}</span>
              <el-icon v-if="index === cursor" :size="14" class="palette__enter"><Right /></el-icon>
            </button>
            <div v-if="!results.length" class="palette__empty">{{ t('nav.noMatch') }}</div>
          </div>
          <div class="palette__foot">
            <span><kbd>↑</kbd><kbd>↓</kbd> {{ t('nav.searchMove') }}</span>
            <span><kbd>Enter</kbd> {{ t('nav.searchOpen') }}</span>
          </div>
        </div>
      </div>
    </transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Right, Search } from '@element-plus/icons-vue'
import { useI18n } from '@/i18n'
import { navGroups, quickLinks } from './nav'

const open = defineModel<boolean>({ default: false })
const router = useRouter()
const { t } = useI18n()

const query = ref('')
const cursor = ref(0)
const inputRef = ref<HTMLInputElement>()
const listRef = ref<HTMLElement>()

const entries = computed(() => {
  const list: Array<{ path: string; label: string; group: string; icon: string }> = []
  for (const group of navGroups) {
    for (const item of group.items) {
      list.push({ path: item.path, label: t(item.titleKey), group: t(group.titleKey), icon: item.icon })
    }
  }
  for (const link of quickLinks) {
    list.push({ path: link.path, label: t(link.titleKey), group: t(link.parentKey), icon: link.icon })
  }
  return list
})

const results = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return entries.value
  return entries.value.filter((entry) =>
    [entry.label, entry.group, entry.path].some((text) => text.toLowerCase().includes(q)),
  )
})

watch(query, () => {
  cursor.value = 0
})

watch(open, async (value) => {
  if (!value) return
  query.value = ''
  cursor.value = 0
  await nextTick()
  inputRef.value?.focus()
})

function move(step: number) {
  const count = results.value.length
  if (!count) return
  cursor.value = (cursor.value + step + count) % count
  nextTick(() => {
    listRef.value?.querySelector('.palette__item--on')?.scrollIntoView({ block: 'nearest' })
  })
}

function choose(entry?: { path: string }) {
  if (!entry) return
  open.value = false
  router.push(entry.path)
}

function close() {
  open.value = false
}
</script>

<style scoped lang="scss">
.palette {
  position: fixed;
  inset: 0;
  z-index: 3000;
  display: flex;
  justify-content: center;
  align-items: flex-start;
  padding: 12vh 16px 16px;
  background: var(--lp-scrim);
  backdrop-filter: blur(2px);
}

.palette__box {
  width: 100%;
  max-width: 560px;
  overflow: hidden;
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  background: var(--lp-surface-raised);
  box-shadow: var(--lp-shadow-xl);
}

.palette__search {
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  padding: 0 var(--lp-4);
  height: 54px;
  border-bottom: 1px solid var(--lp-line-soft);
}
.palette__search-icon { color: var(--lp-text-dim); }
.palette__input {
  flex: 1;
  min-width: 0;
  height: 100%;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--lp-text);
  font: inherit;
  font-size: var(--lp-text-lg);
}
.palette__input::placeholder { color: var(--lp-text-faint); }

.palette__list {
  max-height: min(380px, 52vh);
  overflow-y: auto;
  padding: var(--lp-2);
}

.palette__item {
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  width: 100%;
  height: 44px;
  padding: 0 var(--lp-3);
  border: 0;
  border-radius: var(--lp-r);
  background: transparent;
  color: var(--lp-text);
  font: inherit;
  font-size: var(--lp-text-md);
  text-align: left;
  cursor: pointer;

  &--on { background: var(--lp-accent-soft); }
  &--on .palette__icon { background: var(--lp-accent); color: #fff; }
}
[data-theme='dark'] .palette__item--on .palette__icon { color: var(--lp-text-invert); }

.palette__icon {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  flex: none;
  border-radius: var(--lp-r-sm);
  background: var(--lp-surface-sunken);
  color: var(--lp-text-soft);
  transition: background var(--lp-fast), color var(--lp-fast);
}
.palette__label { font-weight: 500; }
.palette__group { margin-left: auto; font-size: var(--lp-text-sm); color: var(--lp-text-dim); }
.palette__enter { color: var(--lp-accent); }

.palette__empty {
  padding: var(--lp-8) 0;
  text-align: center;
  color: var(--lp-text-dim);
}

.palette__foot {
  display: flex;
  gap: var(--lp-4);
  padding: var(--lp-2) var(--lp-4);
  border-top: 1px solid var(--lp-line-soft);
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
}

kbd,
.palette__kbd {
  display: inline-grid;
  place-items: center;
  min-width: 20px;
  height: 20px;
  padding: 0 5px;
  margin-right: 2px;
  border: 1px solid var(--lp-line);
  border-bottom-width: 2px;
  border-radius: 5px;
  background: var(--lp-surface);
  font-family: var(--lp-font);
  font-size: 11px;
  color: var(--lp-text-soft);
}

.palette-enter-active,
.palette-leave-active { transition: opacity 140ms ease; }
.palette-enter-active .palette__box,
.palette-leave-active .palette__box { transition: transform 160ms cubic-bezier(0.2, 0.8, 0.2, 1); }
.palette-enter-from,
.palette-leave-to { opacity: 0; }
.palette-enter-from .palette__box,
.palette-leave-to .palette__box { transform: translateY(-6px) scale(0.98); }
</style>
