<template>
  <div ref="host" class="code-editor" :style="{ fontSize: `${fontSize}px` }"></div>
</template>

<script setup lang="ts">
/**
 * CodeMirror 6 wrapper: highlighting, folding, search / replace (Ctrl+F,
 * Ctrl+H), multiple cursors, bracket matching, autocompletion of words,
 * Ctrl+S to save. Keeps the document's own line endings.
 */
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Compartment, EditorState } from '@codemirror/state'
import {
  EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection,
  dropCursor, rectangularSelection, crosshairCursor, highlightSpecialChars,
} from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { searchKeymap, highlightSelectionMatches, openSearchPanel, gotoLine } from '@codemirror/search'
import {
  bracketMatching, foldGutter, foldKeymap, indentOnInput, indentUnit, syntaxHighlighting, defaultHighlightStyle,
} from '@codemirror/language'
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from '@codemirror/autocomplete'
import { oneDark } from '@codemirror/theme-one-dark'
import { loadLanguage } from './languages'

const props = withDefaults(defineProps<{
  modelValue: string
  language?: string
  dark?: boolean
  wrap?: boolean
  readonly?: boolean
  fontSize?: number
  /** "\r\n" keeps Windows line endings when the text is read back. */
  lineEnding?: string
}>(), { language: 'text', dark: false, wrap: false, readonly: false, fontSize: 13, lineEnding: '\n' })

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
  (e: 'save'): void
  (e: 'cursor', pos: { line: number; col: number; selected: number; lines: number }): void
}>()

const host = ref<HTMLElement | null>(null)
let view: EditorView | null = null
const languageSlot = new Compartment()
const themeSlot = new Compartment()
const wrapSlot = new Compartment()
const readonlySlot = new Compartment()
let applying = false

/** Indentation the file already uses: tabs, or the smallest space step. */
function detectIndent(text: string): string {
  const lines = text.split('\n', 400)
  let tabs = 0
  const steps = new Map<number, number>()
  for (const line of lines) {
    if (line.startsWith('\t')) tabs++
    const m = /^( +)\S/.exec(line)
    if (m) {
      const n = m[1].length
      if (n === 2 || n === 4 || n === 8) steps.set(n, (steps.get(n) || 0) + 1)
    }
  }
  const spaces = [...steps.values()].reduce((a, b) => a + b, 0)
  if (tabs > spaces) return '\t'
  if (steps.get(2) && (steps.get(2) || 0) >= (steps.get(4) || 0) / 2) return '  '
  return '    '
}

function themeFor(dark: boolean) {
  const base = EditorView.theme({
    '&': { height: '100%' },
    '.cm-scroller': { fontFamily: "'JetBrains Mono', 'Cascadia Code', 'Fira Code', Menlo, Consolas, monospace", lineHeight: '1.55' },
    '.cm-gutters': { borderRight: '1px solid var(--lp-border-light, #e5e7eb)' },
  }, { dark })
  return dark ? [oneDark, base] : [base, syntaxHighlighting(defaultHighlightStyle, { fallback: true })]
}

function emitCursor(state: EditorState) {
  const range = state.selection.main
  const line = state.doc.lineAt(range.head)
  emit('cursor', {
    line: line.number,
    col: range.head - line.from + 1,
    selected: state.selection.ranges.reduce((n, r) => n + (r.to - r.from), 0),
    lines: state.doc.lines,
  })
}

async function setLanguage(id: string) {
  const extension = await loadLanguage(id).catch(() => [])
  view?.dispatch({ effects: languageSlot.reconfigure(extension) })
}

onMounted(() => {
  const state = EditorState.create({
    doc: props.modelValue,
    extensions: [
      EditorState.lineSeparator.of(props.lineEnding === '\r\n' ? '\r\n' : '\n'),
      lineNumbers(),
      highlightActiveLineGutter(),
      highlightSpecialChars(),
      history(),
      foldGutter(),
      drawSelection(),
      dropCursor(),
      EditorState.allowMultipleSelections.of(true),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      autocompletion(),
      rectangularSelection(),
      crosshairCursor(),
      highlightActiveLine(),
      highlightSelectionMatches(),
      indentUnit.of(detectIndent(props.modelValue)),
      keymap.of([
        { key: 'Mod-s', preventDefault: true, run: () => { emit('save'); return true } },
        { key: 'Mod-g', preventDefault: true, run: gotoLine },
        ...closeBracketsKeymap, ...defaultKeymap, ...searchKeymap, ...historyKeymap,
        ...foldKeymap, ...completionKeymap, indentWithTab,
      ]),
      languageSlot.of([]),
      themeSlot.of(themeFor(props.dark)),
      wrapSlot.of(props.wrap ? EditorView.lineWrapping : []),
      readonlySlot.of(EditorState.readOnly.of(props.readonly)),
      EditorView.updateListener.of((update) => {
        // sliceDoc joins lines with the document's own separator (CRLF stays CRLF).
        if (update.docChanged && !applying) emit('update:modelValue', update.state.sliceDoc())
        if (update.docChanged || update.selectionSet) emitCursor(update.state)
      }),
    ],
  })
  view = new EditorView({ state, parent: host.value! })
  void setLanguage(props.language)
  emitCursor(state)
  view.focus()
})

onBeforeUnmount(() => {
  view?.destroy()
  view = null
})

watch(() => props.language, (id) => void setLanguage(id))
watch(() => props.dark, (dark) => view?.dispatch({ effects: themeSlot.reconfigure(themeFor(dark)) }))
watch(() => props.wrap, (wrap) => view?.dispatch({ effects: wrapSlot.reconfigure(wrap ? EditorView.lineWrapping : []) }))
watch(() => props.readonly, (ro) => view?.dispatch({ effects: readonlySlot.reconfigure(EditorState.readOnly.of(ro)) }))
// Replaced from outside (reload from disk): swap the document, keep the view.
watch(() => props.modelValue, (value) => {
  if (!view || value === view.state.sliceDoc()) return
  applying = true
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } })
  applying = false
})

defineExpose({
  focus: () => view?.focus(),
  search: () => view && openSearchPanel(view),
  gotoLine: () => view && gotoLine(view),
})
</script>

<style scoped>
.code-editor { height: 100%; overflow: hidden; }
.code-editor :deep(.cm-editor) { height: 100%; }
.code-editor :deep(.cm-editor.cm-focused) { outline: none; }
</style>
