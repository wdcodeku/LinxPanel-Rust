import { getLocale, t } from '@/i18n'

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

// ===== Dates in the viewer's own language and time zone =====

/** BCP 47 tag for Intl: the UI language, refined by the browser's region (zh-TW, en-GB...). */
export function intlLocale(): string {
  const ui = getLocale()
  const browser = (typeof navigator !== 'undefined' && navigator.language) || ''
  if (browser.toLowerCase().startsWith(ui)) return browser
  return ui === 'zh' ? 'zh-CN' : 'en-US'
}

const formatters = new Map<string, Intl.DateTimeFormat>()
function dtf(options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = intlLocale() + JSON.stringify(options)
  let f = formatters.get(key)
  if (!f) {
    f = new Intl.DateTimeFormat(intlLocale(), options)
    formatters.set(key, f)
  }
  return f
}

/**
 * Compact time for lists: "14:03" today, "Yesterday 14:03", "Nov 7, 14:03"
 * this year, the full date before that. Local time zone of the viewer.
 */
export function formatShortTime(epochSeconds: number | undefined | null, now = Date.now()): string {
  const value = Number(epochSeconds || 0)
  if (!value) return '-'
  const date = new Date(value * 1000)
  const today = new Date(now)
  const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime()
  const time = dtf({ hour: '2-digit', minute: '2-digit' }).format(date)
  const ms = date.getTime()
  if (ms >= startOfToday && ms < startOfToday + 86400000) return `${t('time.today')} ${time}`
  if (ms >= startOfToday - 86400000 && ms < startOfToday) return `${t('time.yesterday')} ${time}`
  if (date.getFullYear() === today.getFullYear()) {
    return dtf({ month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(date)
  }
  return dtf({ year: 'numeric', month: '2-digit', day: '2-digit' }).format(date)
}

/** Full date and time with seconds and the time zone, e.g. for tooltips. */
export function formatFullTime(epochSeconds: number | undefined | null): string {
  const value = Number(epochSeconds || 0)
  if (!value) return '-'
  return dtf({
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit', second: '2-digit', timeZoneName: 'short',
  }).format(new Date(value * 1000))
}

/** "3 minutes ago" / "3 分钟前". */
export function formatRelative(epochSeconds: number | undefined | null, now = Date.now()): string {
  const value = Number(epochSeconds || 0)
  if (!value) return ''
  const diff = Math.round(value - now / 1000)
  const abs = Math.abs(diff)
  const rtf = new Intl.RelativeTimeFormat(intlLocale(), { numeric: 'auto' })
  if (abs < 60) return rtf.format(diff, 'second')
  if (abs < 3600) return rtf.format(Math.round(diff / 60), 'minute')
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), 'hour')
  if (abs < 86400 * 30) return rtf.format(Math.round(diff / 86400), 'day')
  // Whole months / years elapsed (1.7 years ago is "1 year ago").
  if (abs < 86400 * 365) return rtf.format(Math.trunc(diff / (86400 * 30)), 'month')
  return rtf.format(Math.trunc(diff / (86400 * 365)), 'year')
}
