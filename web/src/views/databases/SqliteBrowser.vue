<template>
  <el-drawer
    :model-value="modelValue"
    size="94%"
    append-to-body
    class="sqb-drawer"
    :close-on-press-escape="!rowDialog"
    @update:model-value="(v: boolean) => emit('update:modelValue', v)"
  >
    <template #header>
      <div class="sqb-head">
        <span class="sqb-head__title lp-mono">{{ fileName }}</span>
        <span class="lp-dim lp-mono sqb-head__path">{{ path }}</span>
      </div>
    </template>

    <div class="sqb">
      <!-- Tables -->
      <aside class="sqb-side">
        <el-input v-model="tableKeyword" size="small" clearable :prefix-icon="Search" :placeholder="t('sqb.searchTables')" />
        <div class="sqb-side__list">
          <el-skeleton v-if="tablesLoading && !tables.length" animated :rows="5" />
          <button
            v-for="item in filteredTables"
            :key="item.name"
            type="button"
            class="sqb-table"
            :class="{ 'is-active': item.name === table }"
            @click="selectTable(item.name)"
          >
            <el-icon class="sqb-table__icon"><View v-if="item.engine === 'view'" /><Grid v-else /></el-icon>
            <span class="sqb-table__name lp-mono" :title="item.name">{{ item.name }}</span>
            <span class="sqb-table__count lp-num">{{ item.rows ?? '' }}</span>
          </button>
          <p v-if="!tablesLoading && !tables.length" class="lp-dim sqb-empty">{{ t('dbm.noTables') }}</p>
        </div>
      </aside>

      <!-- Data / structure / SQL -->
      <section class="sqb-main">
        <el-tabs v-model="tab" class="sqb-tabs">
          <el-tab-pane :label="t('sqb.data')" name="data" :disabled="!table">
            <div class="sqb-toolbar">
              <el-input
                v-model="filterInput"
                size="small"
                class="sqb-filter lp-mono"
                clearable
                :placeholder="t('sqb.filterPlaceholder')"
                @keyup.enter="applyFilter"
                @clear="applyFilter"
              >
                <template #prepend>WHERE</template>
              </el-input>
              <el-button size="small" @click="applyFilter">{{ t('sqb.query') }}</el-button>
              <el-button size="small" :icon="Refresh" :loading="rowsLoading" @click="loadRows" />
              <span class="sqb-toolbar__spacer"></span>
              <el-button v-if="page?.editable" size="small" type="primary" :icon="Plus" @click="openInsert">{{ t('sqb.addRow') }}</el-button>
              <el-button v-if="page?.editable" size="small" type="danger" plain :disabled="!selected.length" @click="removeSelected">
                {{ t('sqb.deleteN', { n: selected.length }) }}
              </el-button>
            </div>
            <el-alert v-if="page && !page.editable" type="info" :closable="false" show-icon class="sqb-note" :title="t('sqb.readOnly')" />
            <el-table
              ref="tableRef"
              :data="tableRows"
              size="small"
              border
              height="100%"
              class="sqb-grid"
              v-loading="rowsLoading"
              :empty-text="filter ? t('dbm.noMatch') : t('sqb.noRows')"
              @selection-change="(rows: any[]) => (selected = rows)"
              @sort-change="onSort"
              @row-dblclick="(row: any) => page?.editable && openEdit(row)"
            >
              <el-table-column v-if="page?.editable" type="selection" width="38" fixed />
              <el-table-column
                v-for="(name, index) in page?.columns || []"
                :key="name"
                :prop="`c${index}`"
                :label="name"
                sortable="custom"
                min-width="120"
                show-overflow-tooltip
              >
                <template #header>
                  <span class="lp-mono">{{ name }}</span>
                  <span v-if="pkColumns.includes(name)" class="sqb-pk">PK</span>
                </template>
                <template #default="{ row }">
                  <span v-if="row.__values[index] === null" class="sqb-null">NULL</span>
                  <span v-else-if="isBlob(row.__values[index])" class="sqb-blob">BLOB · {{ formatBytes(row.__values[index].$blob) }}</span>
                  <span v-else class="lp-mono">{{ display(row.__values[index]) }}</span>
                </template>
              </el-table-column>
              <el-table-column v-if="page?.editable" :label="t('actions')" width="110" fixed="right">
                <template #default="{ row }">
                  <el-button link type="primary" size="small" @click="openEdit(row)">{{ t('cron.edit') }}</el-button>
                  <el-button link type="danger" size="small" @click="removeRows([row])">{{ t('delete') }}</el-button>
                </template>
              </el-table-column>
            </el-table>
            <div class="sqb-pager">
              <span class="lp-dim">{{ t('sqb.totalRows', { n: page?.total ?? 0 }) }}</span>
              <el-pagination
                v-model:current-page="pageNo"
                v-model:page-size="pageSize"
                :page-sizes="[20, 50, 100, 200, 500]"
                :total="page?.total ?? 0"
                layout="sizes, prev, pager, next"
                small
                @current-change="loadRows"
                @size-change="() => { pageNo = 1; loadRows() }"
              />
            </div>
          </el-tab-pane>

          <el-tab-pane :label="t('sqb.structure')" name="structure" :disabled="!table">
            <div v-if="schema" class="sqb-structure">
              <el-table :data="schema.columns" size="small" border>
                <el-table-column :label="t('sqb.column')" min-width="160">
                  <template #default="{ row }"><span class="lp-mono strong">{{ row.name }}</span></template>
                </el-table-column>
                <el-table-column :label="t('sqb.type')" width="140">
                  <template #default="{ row }"><span class="lp-mono">{{ row.type || '—' }}</span></template>
                </el-table-column>
                <el-table-column label="NOT NULL" width="100" align="center">
                  <template #default="{ row }">{{ row.notnull ? '✓' : '' }}</template>
                </el-table-column>
                <el-table-column :label="t('sqb.default')" min-width="120">
                  <template #default="{ row }"><span class="lp-mono">{{ row.default ?? '' }}</span></template>
                </el-table-column>
                <el-table-column :label="t('sqb.primaryKey')" width="90" align="center">
                  <template #default="{ row }">{{ row.pk ? (schema.columns.filter((c) => c.pk).length > 1 ? row.pk : '✓') : '' }}</template>
                </el-table-column>
              </el-table>
              <h4 v-if="schema.indexes.length" class="sqb-h">{{ t('sqb.indexes') }}</h4>
              <ul v-if="schema.indexes.length" class="sqb-indexes">
                <li v-for="index in schema.indexes" :key="index.name">
                  <span class="lp-mono strong">{{ index.name }}</span>
                  <el-tag v-if="index.unique" size="small" type="warning">UNIQUE</el-tag>
                  <code v-if="index.sql" class="sqb-code sqb-code--inline">{{ index.sql }}</code>
                </li>
              </ul>
              <h4 class="sqb-h">{{ t('sqb.createSql') }}</h4>
              <pre class="sqb-code">{{ schema.sql }}</pre>
            </div>
          </el-tab-pane>

          <el-tab-pane :label="t('sqb.sql')" name="sql">
            <div class="sqb-sql">
              <textarea
                ref="sqlRef"
                v-model="sqlText"
                class="sqb-editor lp-mono"
                spellcheck="false"
                :placeholder="t('sqb.sqlPlaceholder')"
                @keydown.enter.ctrl.prevent="runSql"
                @keydown.enter.meta.prevent="runSql"
                @keydown.tab.prevent="insertTab"
              ></textarea>
              <div class="sqb-toolbar">
                <el-button size="small" type="primary" :loading="sqlRunning" @click="runSql">{{ t('sqb.run') }} <span class="sqb-key">Ctrl+Enter</span></el-button>
                <el-button v-if="table" size="small" class="lp-mono" @click="quickSelect">SELECT * FROM {{ table }}</el-button>
                <el-dropdown v-if="history.length" trigger="click" @command="(sql: string) => (sqlText = sql)">
                  <el-button size="small">{{ t('sqb.history') }}</el-button>
                  <template #dropdown>
                    <el-dropdown-menu class="sqb-history">
                      <el-dropdown-item v-for="(item, i) in history" :key="i" :command="item">
                        <span class="lp-mono sqb-history__item">{{ item }}</span>
                      </el-dropdown-item>
                    </el-dropdown-menu>
                  </template>
                </el-dropdown>
                <span class="sqb-toolbar__spacer"></span>
                <span v-if="sqlOutput" class="lp-dim lp-num">{{ t('sqb.took', { ms: sqlOutput.millis }) }}</span>
              </div>
              <el-alert v-if="sqlOutput?.error" type="error" :closable="false" show-icon class="sqb-note" :title="sqlOutput.error" />
              <div v-for="(result, i) in sqlOutput?.results || []" :key="i" class="sqb-result">
                <div class="sqb-result__head">
                  <code class="lp-mono sqb-result__stmt">{{ result.statement || `#${i + 1}` }}</code>
                  <span v-if="result.returned_rows" class="lp-pill lp-pill--ok">{{ t('sqb.rowsN', { n: result.rows.length }) }}{{ result.truncated ? '+' : '' }}</span>
                  <span v-else-if="result.changes" class="lp-pill lp-pill--warn">{{ t('sqb.changedN', { n: result.changes }) }}</span>
                  <span v-else class="lp-pill">{{ t('sqb.ok') }}</span>
                </div>
                <el-table v-if="result.returned_rows" :data="result.rows.map((r) => ({ __values: r }))" size="small" border max-height="360">
                  <el-table-column v-for="(name, index) in result.columns" :key="index" :label="name" min-width="110" show-overflow-tooltip>
                    <template #default="{ row }">
                      <span v-if="row.__values[index] === null" class="sqb-null">NULL</span>
                      <span v-else-if="isBlob(row.__values[index])" class="sqb-blob">BLOB · {{ formatBytes(row.__values[index].$blob) }}</span>
                      <span v-else class="lp-mono">{{ display(row.__values[index]) }}</span>
                    </template>
                  </el-table-column>
                </el-table>
                <p v-if="result.truncated" class="lp-dim sqb-trunc">{{ t('sqb.truncated') }}</p>
              </div>
            </div>
          </el-tab-pane>
        </el-tabs>
      </section>
    </div>

    <!-- Edit / insert a row -->
    <el-dialog v-model="rowDialog" :title="editingKey ? t('sqb.editRow') : t('sqb.addRow')" width="640px" append-to-body top="6vh">
      <el-form label-position="top" class="sqb-form" @submit.prevent>
        <el-form-item v-for="column in schema?.columns || []" :key="column.name">
          <template #label>
            <span class="lp-mono strong">{{ column.name }}</span>
            <span class="lp-dim sqb-form__type">{{ column.type || 'ANY' }}{{ column.pk ? ' · PK' : '' }}{{ column.notnull ? ' · NOT NULL' : '' }}{{ column.default !== null ? ` · ${t('sqb.default')} ${column.default}` : '' }}</span>
          </template>
          <div class="sqb-field">
            <el-input
              v-if="!form[column.name].locked"
              v-model="form[column.name].text"
              :disabled="form[column.name].isNull"
              :type="isLongText(column) ? 'textarea' : 'text'"
              :autosize="{ minRows: 1, maxRows: 8 }"
              class="lp-mono"
              :placeholder="!editingKey && (column.default !== null || isRowidAlias(column)) ? t('sqb.leaveDefault') : ''"
              @input="form[column.name].touched = true"
            />
            <span v-else class="lp-dim sqb-locked">{{ form[column.name].lockedText }}</span>
            <el-checkbox
              v-if="!form[column.name].locked"
              v-model="form[column.name].isNull"
              size="small"
              @change="form[column.name].touched = true"
            >NULL</el-checkbox>
          </div>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="rowDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="saveRow">{{ t('save') }}</el-button>
      </template>
    </el-dialog>
  </el-drawer>
</template>

<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Grid, Plus, Refresh, Search, View } from '@element-plus/icons-vue'
import {
  deleteSqliteRows, getSqliteRows, getSqliteSchema, getSqliteTables, insertSqliteRow, runSqliteQuery, updateSqliteRow,
} from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes } from '@/utils/format'

type Column = { name: string; type: string; notnull: boolean; default: string | null; pk: number }
type Schema = { name: string; kind: string; sql: string; columns: Column[]; indexes: { name: string; unique: boolean; sql: string | null }[]; without_rowid: boolean; editable: boolean }
type Page = { columns: string[]; rows: any[][]; keys: any[]; total: number; editable: boolean }
type Result = { statement: string; columns: string[]; rows: any[][]; truncated: boolean; changes: number; returned_rows: boolean }
type Output = { results: Result[]; millis: number; error: string | null }
type Field = { text: string; isNull: boolean; touched: boolean; locked: boolean; lockedText: string }

const props = defineProps<{ modelValue: boolean; path: string; initialTable?: string }>()
const emit = defineEmits<{ (e: 'update:modelValue', v: boolean): void; (e: 'changed'): void }>()
const { t } = useI18n()

const fileName = computed(() => props.path.slice(props.path.lastIndexOf('/') + 1))
const tables = ref<{ name: string; rows: number | null; engine: string | null }[]>([])
const tablesLoading = ref(false)
const tableKeyword = ref('')
const table = ref('')
const tab = ref('data')
const filteredTables = computed(() => {
  const q = tableKeyword.value.trim().toLowerCase()
  return q ? tables.value.filter((x) => x.name.toLowerCase().includes(q)) : tables.value
})

async function loadTables() {
  tablesLoading.value = true
  try {
    const res: any = await getSqliteTables(props.path)
    if (res.success) tables.value = res.data || []
  } finally {
    tablesLoading.value = false
  }
}

async function onOpen() {
  table.value = ''
  page.value = null
  schema.value = null
  sqlOutput.value = null
  filter.value = filterInput.value = ''
  loadHistory()
  await loadTables()
  const first = props.initialTable && tables.value.some((x) => x.name === props.initialTable)
    ? props.initialTable
    : (tables.value.find((x) => x.engine !== 'view') || tables.value[0])?.name
  if (first) await selectTable(first)
  else tab.value = 'sql'
}

// --- Data ---
const schema = ref<Schema | null>(null)
const page = ref<Page | null>(null)
const rowsLoading = ref(false)
const pageNo = ref(1)
const pageSize = ref(50)
const order = ref('')
const desc = ref(false)
const filter = ref('')
const filterInput = ref('')
const selected = ref<any[]>([])
const tableRef = ref()
const pkColumns = computed(() => (schema.value?.columns || []).filter((c) => c.pk > 0).map((c) => c.name))
const tableRows = computed(() =>
  (page.value?.rows || []).map((values, i) => {
    const row: Record<string, any> = { __values: values, __key: page.value?.keys[i] }
    values.forEach((v, j) => (row[`c${j}`] = v))
    return row
  }),
)

async function selectTable(name: string) {
  table.value = name
  pageNo.value = 1
  order.value = ''
  desc.value = false
  filter.value = filterInput.value = ''
  if (tab.value !== 'sql') tab.value = 'data'
  const res: any = await getSqliteSchema(props.path, name).catch(() => null)
  schema.value = res?.success ? res.data : null
  await loadRows()
}

async function loadRows() {
  if (!table.value) return
  rowsLoading.value = true
  try {
    const res: any = await getSqliteRows({
      path: props.path,
      table: table.value,
      offset: (pageNo.value - 1) * pageSize.value,
      limit: pageSize.value,
      order: order.value,
      desc: desc.value,
      filter: filter.value,
    })
    if (res.success) {
      page.value = res.data
      selected.value = []
    }
  } catch {
    // Interceptor reports it.
  } finally {
    rowsLoading.value = false
  }
}

function applyFilter() {
  filter.value = filterInput.value.trim()
  pageNo.value = 1
  void loadRows()
}

function onSort({ prop, order: dir }: { prop: string | null; order: string | null }) {
  const index = prop ? Number(prop.slice(1)) : -1
  order.value = dir && index >= 0 ? page.value?.columns[index] || '' : ''
  desc.value = dir === 'descending'
  void loadRows()
}

function isBlob(value: any) {
  return value && typeof value === 'object' && '$blob' in value
}
function display(value: any) {
  if (value && typeof value === 'object') {
    if ('$int' in value) return value.$int
    if ('$text' in value) return `${value.$text}… (${formatBytes(value.size)})`
  }
  return String(value)
}

// --- Edit / insert ---
const rowDialog = ref(false)
const editingKey = ref<any>(null)
const form = reactive<Record<string, Field>>({})
const saving = ref(false)

function isLongText(column: Column) {
  return /TEXT|CLOB|CHAR|JSON/i.test(column.type || '') || !column.type
}
function isRowidAlias(column: Column) {
  return column.pk === 1 && /^INTEGER$/i.test(column.type) && (schema.value?.columns.filter((c) => c.pk > 0).length || 0) === 1
}

function fillForm(values: any[] | null) {
  for (const key of Object.keys(form)) delete form[key]
  ;(schema.value?.columns || []).forEach((column, i) => {
    const value = values ? values[page.value!.columns.indexOf(column.name)] ?? null : null
    const locked = values !== null && value !== null && typeof value === 'object' && ('$blob' in value || '$text' in value)
    form[column.name] = {
      text: value === null ? '' : display(value),
      isNull: values !== null && value === null,
      touched: false,
      locked,
      lockedText: locked ? (isBlob(value) ? `BLOB · ${formatBytes(value.$blob)}` : t('sqb.tooLong', { size: formatBytes(value.size) })) : '',
    }
    void i
  })
}

function openEdit(row: any) {
  editingKey.value = row.__key
  fillForm(row.__values)
  rowDialog.value = true
}
function openInsert() {
  editingKey.value = null
  fillForm(null)
  rowDialog.value = true
}

/** What the typed text becomes: numbers for numeric columns, else text. */
function typedValue(column: Column, field: Field): unknown {
  if (field.isNull) return null
  const text = field.text
  const numeric = /INT|REAL|FLOA|DOUB|NUM|DEC|BOOL/i.test(column.type || '')
  if (numeric && /^-?\d+$/.test(text.trim())) {
    const n = Number(text.trim())
    return Number.isSafeInteger(n) ? n : { $int: text.trim() }
  }
  if (numeric && text.trim() !== '' && !Number.isNaN(Number(text.trim()))) return Number(text.trim())
  return text
}

async function saveRow() {
  if (!schema.value) return
  const values: Record<string, unknown> = {}
  for (const column of schema.value.columns) {
    const field = form[column.name]
    if (!field || field.locked) continue
    if (editingKey.value) {
      if (field.touched) values[column.name] = typedValue(column, field)
    } else if (field.isNull || field.text !== '') {
      // Empty fields of a new row get the column's default.
      values[column.name] = typedValue(column, field)
    }
  }
  if (editingKey.value && !Object.keys(values).length) {
    rowDialog.value = false
    return
  }
  saving.value = true
  try {
    const res: any = editingKey.value
      ? await updateSqliteRow(props.path, table.value, editingKey.value, values)
      : await insertSqliteRow(props.path, table.value, values)
    if (res.success) {
      rowDialog.value = false
      ElMessage.success(editingKey.value ? t('sqb.saved') : t('sqb.inserted'))
      await Promise.all([loadRows(), loadTables()])
      emit('changed')
    }
  } catch {
    // Interceptor reports it.
  } finally {
    saving.value = false
  }
}

async function removeRows(rows: any[]) {
  if (!rows.length) return
  try {
    await ElMessageBox.confirm(t('sqb.deleteConfirm', { n: rows.length }), t('delete'), {
      type: 'warning',
      confirmButtonText: t('delete'),
      confirmButtonClass: 'el-button--danger',
    })
  } catch {
    return
  }
  const res: any = await deleteSqliteRows(props.path, table.value, rows.map((r) => r.__key)).catch(() => null)
  if (res?.success) {
    ElMessage.success(t('sqb.deletedN', { n: res.data }))
    await Promise.all([loadRows(), loadTables()])
    emit('changed')
  }
}
function removeSelected() {
  void removeRows(selected.value)
}

// --- SQL console ---
const sqlText = ref('')
const sqlRunning = ref(false)
const sqlOutput = ref<Output | null>(null)
const sqlRef = ref<HTMLTextAreaElement>()
const history = ref<string[]>([])
const historyKey = computed(() => `sqb_history:${props.path}`)
function loadHistory() {
  try {
    history.value = JSON.parse(localStorage.getItem(historyKey.value) || '[]')
  } catch {
    history.value = []
  }
}
function remember(sql: string) {
  history.value = [sql, ...history.value.filter((x) => x !== sql)].slice(0, 20)
  try {
    localStorage.setItem(historyKey.value, JSON.stringify(history.value))
  } catch {
    // Not remembered; harmless.
  }
}
function quickSelect() {
  sqlText.value = `SELECT * FROM "${table.value.replace(/"/g, '""')}" LIMIT 100;`
  nextTick(() => sqlRef.value?.focus())
}
function insertTab(event: KeyboardEvent) {
  const el = event.target as HTMLTextAreaElement
  const start = el.selectionStart
  sqlText.value = sqlText.value.slice(0, start) + '  ' + sqlText.value.slice(el.selectionEnd)
  nextTick(() => el.setSelectionRange(start + 2, start + 2))
}

async function runSql() {
  const sql = (() => {
    // Run the selection when there is one.
    const el = sqlRef.value
    if (el && el.selectionEnd > el.selectionStart) return sqlText.value.slice(el.selectionStart, el.selectionEnd)
    return sqlText.value
  })().trim()
  if (!sql) return
  sqlRunning.value = true
  try {
    const res: any = await runSqliteQuery(props.path, sql)
    if (res.success) {
      sqlOutput.value = res.data
      remember(sql)
      if (res.data.results.some((r: Result) => !r.returned_rows)) {
        // Writes or schema changes: refresh what is shown.
        await loadTables()
        if (table.value) {
          const still = tables.value.some((x) => x.name === table.value)
          if (still) await selectTable(table.value)
          else table.value = ''
        }
        emit('changed')
      }
    }
  } catch {
    // Interceptor reports it.
  } finally {
    sqlRunning.value = false
  }
}
// Load whenever it opens (also when created already open) or the file changes.
// Declared last: the immediate run needs every ref above to exist.
let loadedFor = ''
watch(
  () => (props.modelValue ? props.path : ''),
  (openFor) => {
    if (openFor && openFor !== loadedFor) void onOpen()
    loadedFor = openFor
  },
  { immediate: true },
)
</script>

<style scoped lang="scss">
.sqb-head { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.sqb-head__title { font-size: var(--lp-text-lg); font-weight: 600; color: var(--lp-text); }
.sqb-head__path { font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sqb { display: flex; gap: var(--lp-4); height: 100%; min-height: 0; }
.sqb-side { width: 230px; flex-shrink: 0; display: flex; flex-direction: column; gap: var(--lp-2); min-height: 0; }
.sqb-side__list { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 2px; }
.sqb-table {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border: 0;
  border-radius: var(--lp-r-sm);
  background: transparent;
  color: var(--lp-text);
  font: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
  &:hover { background: var(--lp-muted-soft); }
  &.is-active { background: var(--lp-accent-soft); color: var(--lp-accent); }
}
.sqb-table__icon { color: var(--lp-text-dim); flex-shrink: 0; }
.sqb-table__name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sqb-table__count { font-size: 11px; color: var(--lp-text-dim); }
.sqb-empty { font-size: 12px; padding: 8px; }
.sqb-main { flex: 1; min-width: 0; display: flex; flex-direction: column; min-height: 0; }
.sqb-tabs { flex: 1; display: flex; flex-direction: column; min-height: 0; }
.sqb-tabs :deep(.el-tabs__content) { flex: 1; min-height: 0; overflow: auto; }
.sqb-tabs :deep(.el-tab-pane) { height: 100%; display: flex; flex-direction: column; min-height: 0; }
.sqb-toolbar { display: flex; align-items: center; gap: var(--lp-2); margin-bottom: var(--lp-2); flex-wrap: wrap; }
.sqb-toolbar__spacer { flex: 1; }
.sqb-filter { width: min(520px, 100%); }
.sqb-note { margin-bottom: var(--lp-2); }
.sqb-grid { flex: 1; min-height: 240px; }
.sqb-pk { margin-left: 6px; padding: 0 4px; border-radius: 3px; background: var(--lp-warn-soft); color: var(--lp-warn); font-size: 10px; font-weight: 700; }
.sqb-null { color: var(--lp-text-faint); font-style: italic; font-size: 12px; }
.sqb-blob { color: var(--lp-accent); font-size: 12px; }
.sqb-pager { display: flex; align-items: center; justify-content: space-between; gap: var(--lp-3); padding-top: var(--lp-2); flex-wrap: wrap; }
.sqb-structure { overflow: auto; }
.sqb-h { margin: var(--lp-4) 0 var(--lp-2); font-size: var(--lp-text-sm); }
.sqb-indexes { margin: 0; padding: 0; list-style: none; display: flex; flex-direction: column; gap: 6px; li { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; } }
.sqb-code {
  margin: 0;
  padding: 10px 12px;
  border-radius: var(--lp-r-sm);
  background: var(--lp-muted-soft);
  font-family: var(--lp-font-mono);
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-word;
}
.sqb-code--inline { padding: 2px 6px; font-size: 11px; }
.sqb-sql { display: flex; flex-direction: column; gap: var(--lp-2); overflow: auto; }
.sqb-editor {
  width: 100%;
  min-height: 160px;
  resize: vertical;
  padding: 10px 12px;
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-sm);
  background: var(--lp-surface);
  color: var(--lp-text);
  font-size: 13px;
  line-height: 1.6;
  box-sizing: border-box;
  &:focus { outline: none; border-color: var(--lp-accent); }
}
.sqb-key { margin-left: 6px; font-size: 11px; opacity: 0.75; }
.sqb-result { border: 1px solid var(--lp-line); border-radius: var(--lp-r-sm); overflow: hidden; }
.sqb-result__head { display: flex; align-items: center; gap: var(--lp-2); padding: 6px 10px; background: var(--lp-muted-soft); }
.sqb-result__stmt { flex: 1; min-width: 0; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sqb-trunc { margin: 0; padding: 6px 10px; font-size: 12px; }
.sqb-form__type { margin-left: 8px; font-size: 11px; font-weight: 400; }
.sqb-field { display: flex; align-items: flex-start; gap: var(--lp-2); width: 100%; .el-input, .el-textarea { flex: 1; min-width: 0; } }
.sqb-locked { font-size: 12px; padding: 6px 0; }
.strong { font-weight: 600; }
.sqb-history__item { display: inline-block; max-width: 520px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
@media (max-width: 760px) {
  .sqb { flex-direction: column; }
  .sqb-side { width: 100%; max-height: 180px; }
}
</style>

<style lang="scss">
/* The drawer is teleported: its body must fill the height for the grid. */
.sqb-drawer .el-drawer__body { display: flex; flex-direction: column; min-height: 0; overflow: hidden; padding-top: 4px; }
.sqb-drawer .el-drawer__header { margin-bottom: 8px; }
</style>
