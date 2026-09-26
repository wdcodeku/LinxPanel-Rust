<template>
  <div ref="host" class="metric-chart" :style="{ height: height + 'px' }"></div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import * as echarts from 'echarts/core'
import { LineChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, LegendComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { useUserStore } from '@/stores/user'

echarts.use([LineChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer])

export type Series = {
  name: string
  color: string
  /** [timestampMs, value] pairs */
  data: [number, number][]
}

const props = withDefaults(
  defineProps<{
    series: Series[]
    height?: number
    /** Fixes the y axis to 0-100 for percentage metrics. */
    percent?: boolean
    /** Formats y-axis labels and tooltip values. */
    formatter?: (value: number) => string
    showLegend?: boolean
  }>(),
  { height: 200, percent: false, showLegend: false },
)

const host = ref<HTMLElement | null>(null)
const userStore = useUserStore()
let chart: echarts.ECharts | null = null
let observer: ResizeObserver | null = null

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}

function defaultFormatter(value: number): string {
  return props.percent ? `${value.toFixed(0)}%` : String(Math.round(value))
}

function buildOption(): echarts.EChartsCoreOption {
  const fmt = props.formatter || defaultFormatter
  const axisColor = cssVar('--lp-text-dim')
  const splitColor = cssVar('--lp-line-soft')

  return {
    animation: false,
    grid: {
      top: props.showLegend ? 28 : 12,
      right: 12,
      bottom: 22,
      left: 46,
    },
    legend: props.showLegend
      ? {
          top: 0,
          right: 0,
          icon: 'roundRect',
          itemWidth: 8,
          itemHeight: 8,
          itemGap: 14,
          textStyle: { color: axisColor, fontSize: 11 },
        }
      : { show: false },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'line', lineStyle: { color: cssVar('--lp-line-strong') } },
      backgroundColor: cssVar('--lp-surface-raised'),
      borderColor: cssVar('--lp-line-strong'),
      borderWidth: 1,
      padding: [6, 10],
      textStyle: { color: cssVar('--lp-text'), fontSize: 12 },
      formatter: (params: any) => {
        const rows = Array.isArray(params) ? params : [params]
        if (!rows.length) return ''
        const time = new Date(rows[0].value[0]).toLocaleTimeString()
        const lines = rows
          .map(
            (row: any) =>
              `<div style="display:flex;gap:8px;align-items:center;justify-content:space-between">
                 <span style="display:flex;gap:6px;align-items:center">
                   <i style="width:8px;height:8px;border-radius:2px;background:${row.color}"></i>${row.seriesName}
                 </span>
                 <b style="font-variant-numeric:tabular-nums">${fmt(row.value[1])}</b>
               </div>`,
          )
          .join('')
        return `<div style="font-size:11px;color:${cssVar('--lp-text-dim')};margin-bottom:4px">${time}</div>${lines}`
      },
    },
    xAxis: {
      type: 'time',
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: axisColor, fontSize: 10, hideOverlap: true },
      splitLine: { show: false },
    },
    yAxis: {
      type: 'value',
      min: 0,
      max: props.percent ? 100 : undefined,
      axisLabel: { color: axisColor, fontSize: 10, formatter: fmt },
      splitLine: { lineStyle: { color: splitColor, type: 'dashed' } },
    },
    series: props.series.map((item) => ({
      name: item.name,
      type: 'line',
      showSymbol: false,
      smooth: 0.2,
      lineStyle: { width: 1.8, color: item.color },
      itemStyle: { color: item.color },
      areaStyle: {
        color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
          { offset: 0, color: withAlpha(item.color, 0.22) },
          { offset: 1, color: withAlpha(item.color, 0) },
        ]),
      },
      data: item.data,
    })),
  }
}

/** echarts needs a concrete color; CSS vars are already resolved by the caller. */
function withAlpha(color: string, alpha: number): string {
  const value = color.trim()
  if (value.startsWith('#')) {
    const hex = value.slice(1)
    const full = hex.length === 3 ? hex.split('').map((c) => c + c).join('') : hex
    const r = parseInt(full.slice(0, 2), 16)
    const g = parseInt(full.slice(2, 4), 16)
    const b = parseInt(full.slice(4, 6), 16)
    return `rgba(${r},${g},${b},${alpha})`
  }
  return value
}

function render() {
  if (!chart) return
  chart.setOption(buildOption(), { notMerge: true, lazyUpdate: true })
}

onMounted(() => {
  if (!host.value) return
  chart = echarts.init(host.value, undefined, { renderer: 'canvas' })
  render()
  observer = new ResizeObserver(() => chart?.resize())
  observer.observe(host.value)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  observer = null
  chart?.dispose()
  chart = null
})

watch(() => props.series, render, { deep: false })
// Chart colors are read from CSS vars, so a theme flip needs a repaint.
watch(() => userStore.theme, () => requestAnimationFrame(render))
</script>

<style scoped>
.metric-chart {
  width: 100%;
}
</style>
