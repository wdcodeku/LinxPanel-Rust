import type { App } from 'vue'

function makeDraggable(dialog: HTMLElement) {
  if ((dialog as any).__lp_drag) return
  ;(dialog as any).__lp_drag = true

  const header = dialog.querySelector('.el-dialog__header') as HTMLElement
  if (!header) return
  header.style.cursor = 'move'
  header.style.userSelect = 'none'

  let ox = 0, oy = 0, tx = 0, ty = 0

  header.addEventListener('mousedown', (e: MouseEvent) => {
    if ((e.target as HTMLElement).closest('.el-dialog__headerbtn')) return
    e.preventDefault()
    ox = e.clientX - tx
    oy = e.clientY - ty

    const onMove = (ev: MouseEvent) => {
      tx = ev.clientX - ox
      ty = ev.clientY - oy
      dialog.style.transform = `translate(${tx}px, ${ty}px)`
    }
    const onUp = () => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseup', onUp)
    }
    document.addEventListener('mousemove', onMove)
    document.addEventListener('mouseup', onUp)
  })
}

// Auto-attach to all dialogs via MutationObserver
let observer: MutationObserver | null = null

export function installDragPlugin(app: App) {
  app.directive('drag', {
    mounted(el: HTMLElement) {
      const dialog = el.querySelector('.el-dialog') as HTMLElement
      if (dialog) makeDraggable(dialog)
    }
  })

  // Global observer for dynamically created dialogs (teleported)
  if (typeof window !== 'undefined' && !observer) {
    observer = new MutationObserver((mutations) => {
      for (const m of mutations) {
        for (const node of m.addedNodes) {
          if (node instanceof HTMLElement) {
            const dialogs = node.classList?.contains('el-dialog')
              ? [node]
              : Array.from(node.querySelectorAll?.('.el-dialog') || [])
            for (const d of dialogs) {
              makeDraggable(d as HTMLElement)
            }
          }
        }
      }
    })
    observer.observe(document.body, { childList: true, subtree: true })
  }
}