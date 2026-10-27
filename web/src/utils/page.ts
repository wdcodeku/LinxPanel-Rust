import { onActivated, onDeactivated, ref } from 'vue'

/**
 * Pages stay alive in the background (KeepAlive), so switching back to one is
 * instant and shows what it showed before instead of an empty, loading page.
 *
 * - `active` is false while the page is hidden: pollers skip their tick then.
 * - `onReturn` runs each time a cached page is shown again (not on its first
 *   mount) to refresh its data quietly behind what is already on screen.
 */
export function usePage(onReturn?: () => unknown) {
  const active = ref(true)
  let shown = false
  onActivated(() => {
    active.value = true
    if (shown) void onReturn?.()
    shown = true
  })
  onDeactivated(() => {
    active.value = false
  })
  return { active }
}
