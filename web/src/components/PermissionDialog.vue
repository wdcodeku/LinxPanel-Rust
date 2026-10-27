<template>
  <el-dialog
    :model-value="modelValue"
    :title="t('perm.title')"
    width="520px"
    append-to-body
    :close-on-click-modal="false"
    @update:model-value="(v: boolean) => emit('update:modelValue', v)"
    @open="onOpen"
  >
    <div class="perm-target lp-mono" :title="targets.map((x) => x.path).join('\n')">{{ targetLabel }}</div>

    <table class="perm-grid">
      <thead>
        <tr>
          <th></th>
          <th>{{ t('perm.read') }}</th>
          <th>{{ t('perm.write') }}</th>
          <th>{{ t('perm.exec') }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(who, row) in WHO" :key="who">
          <th>{{ t(`perm.${who}`) }}</th>
          <td v-for="(bit, col) in BITS" :key="bit">
            <el-checkbox :model-value="has(row, bit)" :aria-label="`${t(`perm.${who}`)} ${t(`perm.${['read', 'write', 'exec'][col]}`)}`" @change="(v) => toggle(row, bit, v === true)" />
          </td>
        </tr>
      </tbody>
    </table>

    <div class="perm-row">
      <label class="perm-label">{{ t('perm.numeric') }}</label>
      <el-input v-model="octal" class="lp-mono perm-octal" maxlength="4" @input="onOctalInput" />
      <span class="perm-symbolic lp-mono">{{ symbolic }}</span>
    </div>
    <div class="perm-presets">
      <button v-for="p in PRESETS" :key="p.mode" type="button" class="perm-preset" :class="{ 'is-active': octal === p.mode }" @click="setOctal(p.mode)">
        <span class="lp-mono">{{ p.mode }}</span>
        <span class="perm-preset__hint">{{ t(`perm.preset${p.mode}`) }}</span>
      </button>
    </div>
    <p v-if="mode !== null && (mode & 0o002)" class="perm-warn">{{ t('perm.worldWritable') }}</p>

    <div class="perm-owner">
      <div class="perm-field">
        <label class="perm-label">{{ t('perm.owner') }}</label>
        <el-select v-model="owner" filterable allow-create default-first-option :placeholder="t('perm.keep')" clearable>
          <el-option v-for="u in users" :key="u" :label="u" :value="u" />
        </el-select>
      </div>
      <div class="perm-field">
        <label class="perm-label">{{ t('perm.group') }}</label>
        <el-select v-model="group" filterable allow-create default-first-option :placeholder="t('perm.keep')" clearable>
          <el-option v-for="g in groups" :key="g" :label="g" :value="g" />
        </el-select>
      </div>
    </div>
    <p class="perm-hint">{{ t('perm.webHint') }}</p>

    <el-checkbox v-if="hasDir" v-model="recursive">{{ t('perm.recursive') }}</el-checkbox>

    <template #footer>
      <el-button @click="emit('update:modelValue', false)">{{ t('cancel') }}</el-button>
      <el-button type="primary" :loading="saving" :disabled="mode === null" @click="apply">{{ t('ok') }}</el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { chmodApi, chownApi, getOwnerChoices } from '@/api'
import { useI18n } from '@/i18n'

type Target = { path: string; name?: string; is_dir?: boolean; permissions?: string; owner?: string; group?: string }

const props = defineProps<{ modelValue: boolean; targets: Target[] }>()
const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
  (e: 'done'): void
}>()

const { t } = useI18n()
const WHO = ['owner', 'group', 'public'] as const
const BITS = [4, 2, 1] as const
const PRESETS = [{ mode: '755' }, { mode: '644' }, { mode: '600' }, { mode: '700' }, { mode: '777' }]

const octal = ref('755')
// What the dialog opened with: an untouched mode is not applied (several
// selected items keep their own modes when only the owner changes).
let openedWith = ''
const owner = ref('')
const group = ref('')
const recursive = ref(false)
const saving = ref(false)
const users = ref<string[]>([])
const groups = ref<string[]>([])

const hasDir = computed(() => props.targets.some((x) => x.is_dir))
const targetLabel = computed(() =>
  props.targets.length === 1 ? props.targets[0].path : t('fm.batchTargetN', { n: props.targets.length }),
)

/** The mode bits, or null while the typed value is not valid octal. */
const mode = computed<number | null>(() => (/^[0-7]{3,4}$/.test(octal.value) ? parseInt(octal.value, 8) : null))
const symbolic = computed(() => {
  const m = mode.value
  if (m === null) return '---------'
  return [6, 3, 0]
    .map((shift) => {
      const v = (m >> shift) & 7
      return `${v & 4 ? 'r' : '-'}${v & 2 ? 'w' : '-'}${v & 1 ? 'x' : '-'}`
    })
    .join('')
})

function has(row: number, bit: number) {
  const m = mode.value ?? 0
  return ((m >> ((2 - row) * 3)) & bit) !== 0
}

function toggle(row: number, bit: number, on: boolean) {
  const m = mode.value ?? 0
  const mask = bit << ((2 - row) * 3)
  setOctal(((on ? m | mask : m & ~mask) & 0o7777).toString(8).padStart(3, '0'))
}

function setOctal(value: string) {
  octal.value = value
}

function onOctalInput(value: string) {
  octal.value = value.replace(/[^0-7]/g, '').slice(0, 4)
}

async function onOpen() {
  const first = props.targets[0]
  // Start from what the (first) item has now.
  octal.value = first?.permissions && /^[0-7]{3,4}$/.test(first.permissions) ? first.permissions : first?.is_dir ? '755' : '644'
  const same = (key: 'owner' | 'group') => {
    const values = new Set(props.targets.map((x) => x[key] || ''))
    return values.size === 1 ? [...values][0] : ''
  }
  openedWith = octal.value
  owner.value = same('owner')
  group.value = same('group')
  recursive.value = false
  if (!users.value.length) {
    const res: any = await getOwnerChoices().catch(() => null)
    if (res?.success) {
      users.value = res.data.users || []
      groups.value = res.data.groups || []
    }
  }
}

async function apply() {
  if (mode.value === null) return
  saving.value = true
  let failed = 0
  try {
    for (const target of props.targets) {
      const deep = recursive.value && !!target.is_dir
      if (octal.value !== openedWith || deep) {
        const r1: any = await chmodApi(target.path, octal.value, deep).catch(() => null)
        if (!r1?.success) failed++
      }
      const ownerChanged = owner.value && owner.value !== (target.owner || '')
      const groupChanged = group.value && group.value !== (target.group || '')
      if (ownerChanged || groupChanged || (recursive.value && target.is_dir && (owner.value || group.value))) {
        const r2: any = await chownApi(target.path, owner.value, group.value, recursive.value && !!target.is_dir).catch(() => null)
        if (!r2?.success) failed++
      }
    }
    if (!failed) {
      ElMessage.success(props.targets.length > 1 ? t('fm.permChangedN', { n: props.targets.length }) : t('fm.permChanged'))
      emit('update:modelValue', false)
    }
    emit('done')
  } finally {
    saving.value = false
  }
}
</script>

<style scoped lang="scss">
.perm-target {
  margin-bottom: var(--lp-3);
  padding: 6px 10px;
  border-radius: var(--lp-r-sm);
  background: var(--lp-muted-soft);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.perm-grid {
  width: 100%;
  border-collapse: collapse;
  margin-bottom: var(--lp-3);
  th,
  td {
    padding: 6px 8px;
    text-align: center;
    border-bottom: 1px solid var(--lp-line-soft);
    font-size: 13px;
  }
  thead th { font-weight: 500; color: var(--lp-text-dim); font-size: 12px; }
  tbody th { text-align: left; font-weight: 500; width: 34%; }
}
.perm-row { display: flex; align-items: center; gap: var(--lp-3); margin-bottom: var(--lp-2); }
.perm-label { font-size: 12px; color: var(--lp-text-dim); white-space: nowrap; }
.perm-octal { width: 90px; }
.perm-symbolic { color: var(--lp-text-soft); letter-spacing: 1px; }
.perm-presets { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: var(--lp-2); }
.perm-preset {
  display: inline-flex;
  align-items: baseline;
  gap: 6px;
  padding: 4px 10px;
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-full);
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
  &:hover { border-color: var(--lp-accent); }
  &.is-active { border-color: var(--lp-accent); background: var(--lp-accent-soft); color: var(--lp-accent); }
}
.perm-preset__hint { color: var(--lp-text-dim); font-size: 11px; }
.perm-warn { margin: 0 0 var(--lp-2); font-size: 12px; color: var(--lp-danger); }
.perm-owner { display: grid; grid-template-columns: 1fr 1fr; gap: var(--lp-3); margin-top: var(--lp-3); }
.perm-field { display: flex; flex-direction: column; gap: 4px; }
.perm-hint { margin: 6px 0 var(--lp-3); font-size: 12px; color: var(--lp-text-dim); line-height: 1.5; }
</style>
