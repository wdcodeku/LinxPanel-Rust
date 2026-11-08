<template>
  <!-- The software a page depends on (OpenResty, a database engine, a
       language runtime): name, version, state, and its controls in one bar. -->
  <section class="app-status">
    <span class="app-status__logo" :data-kind="kind">{{ logo || name.slice(0, 2) }}</span>
    <span class="app-status__name">{{ name }}</span>
    <span v-if="version" class="app-status__tag">{{ version }}</span>
    <span class="app-status__state" :class="`is-${state}`">
      <span class="app-status__dot"></span>{{ stateLabel }}
    </span>
    <span v-if="$slots.default" class="app-status__actions"><slot /></span>
    <span class="app-status__spacer"></span>
    <span v-if="$slots.extra" class="app-status__extra"><slot name="extra" /></span>
  </section>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from '@/i18n'

const props = defineProps<{
  name: string
  /** Short logo text; defaults to the first letters of the name. */
  logo?: string
  /** Selects the logo color (see the data-kind rules). */
  kind?: string
  version?: string
  state: 'running' | 'stopped' | 'missing' | 'unknown'
  /** Overrides the state's text. */
  label?: string
}>()
const { t } = useI18n()
const stateLabel = computed(
  () =>
    props.label ||
    { running: t('ui.running'), stopped: t('ui.stopped'), missing: t('ui.notInstalled'), unknown: t('ui.unknown') }[props.state],
)
</script>

<style scoped lang="scss">
.app-status {
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  flex-wrap: wrap;
  min-height: 56px;
  margin-bottom: var(--lp-3);
  padding: var(--lp-2) var(--lp-5);
  background: var(--lp-surface);
  border: 1px solid var(--lp-card-line);
  border-radius: var(--lp-r-lg);
}
.app-status__logo {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  flex: none;
  border-radius: var(--lp-r);
  background: var(--lp-accent);
  color: #fff;
  font-size: 12px;
  font-weight: 700;
  text-transform: capitalize;
  &[data-kind='openresty'] { background: #19a15f; }
  &[data-kind='mysql'] { background: #00758f; }
  &[data-kind='postgresql'] { background: #336791; }
  &[data-kind='redis'] { background: #d82c20; }
  &[data-kind='mongodb'] { background: #13aa52; }
  &[data-kind='sqlite'] { background: #0f80cc; }
}
.app-status__name { font-size: var(--lp-text-lg); font-weight: 600; color: var(--lp-text); }
.app-status__tag {
  padding: 1px 8px;
  border-radius: var(--lp-r-sm);
  background: var(--lp-surface-sunken);
  border: 1px solid var(--lp-line);
  color: var(--lp-text-soft);
  font-size: var(--lp-text-sm);
  font-family: var(--lp-font-mono);
}
.app-status__state {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 24px;
  padding: 0 10px;
  border-radius: var(--lp-r-full);
  font-size: var(--lp-text-sm);
  background: var(--lp-muted-soft);
  color: var(--lp-muted);
  &.is-running { background: var(--lp-ok-soft); color: var(--lp-ok); }
  &.is-stopped { background: var(--lp-danger-soft); color: var(--lp-danger); }
  &.is-missing { background: var(--lp-warn-soft); color: var(--lp-warn); }
}
.app-status__dot { width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
.app-status__actions {
  display: inline-flex;
  align-items: center;
  flex-wrap: wrap;
  padding-left: var(--lp-3);
  border-left: 1px solid var(--lp-line);
  /* Text buttons separated by thin rules, as in a toolbar. */
  :deep(.el-button) { margin: 0; padding: 0 10px; height: 24px; }
  :deep(.el-button + .el-button) { border-left: 1px solid var(--lp-line-soft); border-radius: 0; }
}
.app-status__spacer { flex: 1; }
.app-status__extra { display: inline-flex; align-items: center; gap: var(--lp-2); }
@media (max-width: 768px) {
  .app-status { padding: var(--lp-2) var(--lp-3); }
  .app-status__actions { padding-left: 0; border-left: 0; }
}
</style>
