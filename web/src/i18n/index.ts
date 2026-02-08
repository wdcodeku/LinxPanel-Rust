import { ref, computed } from 'vue'
import zhMessages from './zh'
import enMessages from './en'

export type Locale = 'zh' | 'en'

const messages: Record<Locale, any> = { zh: zhMessages, en: enMessages }

const currentLocale = ref<Locale>((localStorage.getItem('locale') as Locale) || 'zh')

export function setLocale(locale: Locale) {
  currentLocale.value = locale
  localStorage.setItem('locale', locale)
}

export function getLocale(): Locale {
  return currentLocale.value
}

/**
 * Translation function. Supports dot notation: t('fm.name')
 * Supports interpolation: t('fm.confirmDeleteN', { n: 3 })
 */
export function t(key: string, params?: Record<string, any>): string {
  const locale = currentLocale.value
  const keys = key.split('.')
  let val: any = messages[locale]
  for (const k of keys) {
    if (val && typeof val === 'object' && k in val) val = val[k]
    else return key
  }
  if (typeof val !== 'string') return key
  if (params) {
    return val.replace(/\{(\w+)\}/g, (_: string, k: string) => String(params[k] ?? ''))
  }
  return val
}

/** Reactive composable for use in templates */
export function useI18n() {
  return {
    t: (key: string, params?: Record<string, any>) => {
      // Access currentLocale.value to make it reactive
      const _ = currentLocale.value
      return t(key, params)
    },
    locale: currentLocale,
    setLocale,
  }
}
