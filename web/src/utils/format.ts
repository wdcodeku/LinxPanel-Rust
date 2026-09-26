import { t } from '@/i18n'

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']

/** 1536 -> "1.5 KB". Uses 1024 steps, which is what every other panel surface shows. */
export function formatBytes(bytes: number | undefined | null, digits = 1): string {
  const value = Number(bytes || 0)
  if (!Number.isFinite(value) || value <= 0) return '0 B'
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), BYTE_UNITS.length - 1)
  const scaled = value / 1024 ** index
  return `${scaled.toFixed(index === 0 ? 0 : digits)} ${BYTE_UNITS[index]}`
}

/** Bytes per second, shown as a rate. */
export function formatRate(bytesPerSecond: number | undefined | null): string {
  return `${formatBytes(bytesPerSecond)}/s`
}

/** Compact rate for axis labels: drops the decimal below 10 MB/s. */
export function formatRateShort(bytesPerSecond: number): string {
  const value = Number(bytesPerSecond || 0)
  if (value <= 0) return '0'
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), BYTE_UNITS.length - 1)
  const scaled = value / 1024 ** index
  return `${scaled >= 10 ? scaled.toFixed(0) : scaled.toFixed(1)}${BYTE_UNITS[index]}`
}

/** 93784 -> "1d 2h 3m". Always at most two units, largest first. */
export function formatDuration(seconds: number | undefined | null): string {
  const total = Math.max(0, Math.floor(Number(seconds || 0)))
  const days = Math.floor(total / 86400)
  const hours = Math.floor((total % 86400) / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const secs = total % 60

  if (days > 0) return `${days}${t('unit.day')} ${hours}${t('unit.hour')}`
  if (hours > 0) return `${hours}${t('unit.hour')} ${minutes}${t('unit.minute')}`
  if (minutes > 0) return `${minutes}${t('unit.minute')} ${secs}${t('unit.second')}`
  return `${secs}${t('unit.second')}`
}

export function formatPercent(value: number | undefined | null, digits = 1): string {
  const num = Number(value || 0)
  return `${Number.isFinite(num) ? num.toFixed(digits) : '0.0'}%`
}

export function formatDateTime(epochSeconds: number | undefined | null): string {
  const value = Number(epochSeconds || 0)
  if (!value) return '-'
  return new Date(value * 1000).toLocaleString()
}

/** Shared threshold ramp: keeps meters, pills and charts agreeing on what "hot" means. */
export function usageLevel(percent: number): 'ok' | 'warn' | 'danger' {
  if (percent >= 88) return 'danger'
  if (percent >= 70) return 'warn'
  return 'ok'
}
