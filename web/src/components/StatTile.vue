<template>
  <div class="stat-tile" :class="{ 'stat-tile--clickable': clickable }">
    <div class="stat-tile__head">
      <span class="stat-tile__label">{{ label }}</span>
      <span v-if="badge" class="lp-pill" :class="badgeClass">{{ badge }}</span>
    </div>

    <div class="stat-tile__value lp-num">
      {{ value }}<span v-if="unit" class="stat-tile__unit">{{ unit }}</span>
    </div>

    <div v-if="typeof percent === 'number'" class="stat-tile__meter" :aria-label="`${label} ${percent.toFixed(0)}%`">
      <div class="stat-tile__meter-fill" :style="{ width: clampedPercent + '%', background: levelColor }"></div>
    </div>

    <div v-if="sub" class="stat-tile__sub">{{ sub }}</div>

    <!-- Sparkline: a plain SVG polyline is far cheaper than a chart instance per tile. -->
    <svg v-if="spark && spark.length > 1" class="stat-tile__spark" viewBox="0 0 100 28" preserveAspectRatio="none">
      <polygon :points="`0,28 ${sparkPoints} 100,28`" :fill="levelColor" opacity="0.08" />
      <polyline :points="sparkPoints" fill="none" :stroke="levelColor" stroke-width="1.5"
        stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke" />
    </svg>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    label: string
    value: string | number
    unit?: string
    sub?: string
    /** Drives the meter bar and the color ramp. */
    percent?: number
    spark?: number[]
    badge?: string
    badgeLevel?: 'ok' | 'warn' | 'danger' | 'accent' | 'muted'
    clickable?: boolean
    /** Forces a color instead of deriving it from `percent`. */
    tone?: 'accent' | 'ok' | 'warn' | 'danger'
  }>(),
  { badgeLevel: 'muted', clickable: false },
)

const clampedPercent = computed(() => Math.min(100, Math.max(0, props.percent ?? 0)))

const badgeClass = computed(() => `lp-pill--${props.badgeLevel}`)

/** Green under 70%, amber to 88%, red above — the thresholds the meters use everywhere. */
const levelColor = computed(() => {
  if (props.tone) return `var(--lp-${props.tone === 'accent' ? 'accent' : props.tone})`
  const value = clampedPercent.value
  if (value >= 88) return 'var(--lp-danger)'
  if (value >= 70) return 'var(--lp-warn)'
  return 'var(--lp-accent)'
})

const sparkPoints = computed(() => {
  const data = props.spark || []
  if (data.length < 2) return ''
  const max = Math.max(...data, 1)
  const min = Math.min(...data, 0)
  const range = max - min || 1
  const step = 100 / (data.length - 1)
  return data
    .map((value, index) => {
      const x = index * step
      const y = 26 - ((value - min) / range) * 24
      return `${x.toFixed(2)},${y.toFixed(2)}`
    })
    .join(' ')
})
</script>

<style scoped lang="scss">
.stat-tile {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 6px;
  /* The bottom band belongs to the sparkline: text never runs into it. */
  padding: var(--lp-4) var(--lp-4) 34px;
  background: var(--lp-surface);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  box-shadow: var(--lp-shadow-sm);
  overflow: hidden;
  transition: border-color var(--lp-fast), box-shadow var(--lp-fast), transform var(--lp-fast);

  &--clickable {
    cursor: pointer;
    &:hover {
      border-color: var(--lp-accent);
      box-shadow: var(--lp-shadow);
      transform: translateY(-1px);
    }
  }
}

.stat-tile__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--lp-2);
}

.stat-tile__label {
  font-size: var(--lp-text-sm);
  font-weight: 600;
  color: var(--lp-text-dim);
}

.stat-tile__value {
  font-size: var(--lp-text-2xl);
  font-weight: 640;
  line-height: 1.1;
  letter-spacing: -0.02em;
  color: var(--lp-text);
}

.stat-tile__unit {
  margin-left: 3px;
  font-size: var(--lp-text-md);
  font-weight: 500;
  color: var(--lp-text-dim);
}

.stat-tile__meter {
  height: 4px;
  border-radius: var(--lp-r-full);
  background: var(--lp-surface-sunken);
  overflow: hidden;
}
.stat-tile__meter-fill {
  height: 100%;
  border-radius: inherit;
  transition: width 600ms cubic-bezier(0.4, 0, 0.2, 1), background var(--lp-base);
}

.stat-tile__sub {
  font-size: var(--lp-text-sm);
  color: var(--lp-text-dim);
  font-variant-numeric: tabular-nums;
}

.stat-tile__spark {
  position: absolute;
  inset: auto 0 0 0;
  width: 100%;
  height: 28px;
  opacity: 0.55;
  pointer-events: none;
}
</style>
