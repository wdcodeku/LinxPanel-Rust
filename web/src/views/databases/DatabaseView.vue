<template>
  <div class="lp-page">
    <div class="lp-page-head">
      <div>
        <h1 class="lp-page-title">{{ t('nav.databases') }}</h1>
        <p class="lp-page-sub">{{ t('dbm.subtitle') }}</p>
      </div>
      <div class="lp-page-actions">
        <el-button size="small" :icon="Refresh" :loading="loading" @click="loadAll">{{ t('refresh') }}</el-button>
      </div>
    </div>

    <!-- Engine switcher -->
    <div class="engines" role="tablist">
      <template v-if="loaded">
        <button
          v-for="item in engines"
          :key="item.engine"
          type="button"
          role="tab"
          class="engine"
          :class="{ 'is-active': item.engine === engine }"
          :aria-selected="item.engine === engine"
          @click="selectEngine(item.engine)"
        >
          <span class="engine__logo" :data-engine="item.engine">{{ logo(item.engine) }}</span>
          <span class="engine__text">
            <span class="engine__name">{{ item.display_name }}</span>
            <span class="engine__sub">
              <span class="dot" :class="item.running ? 'dot--ok' : item.installed ? 'dot--warn' : ''"></span>
              {{ item.running ? shortVersion(item) : item.installed ? t('dbm.stopped') : t('dbm.notInstalled') }}
            </span>
          </span>
        </button>
      </template>
      <el-skeleton v-else animated class="engines__skeleton">
        <template #template>
          <el-skeleton-item v-for="i in 3" :key="i" variant="rect" class="engine-skel" />
        </template>
      </el-skeleton>
    </div>

    <template v-if="loaded && current">
      <!-- Not installed: choose a version and install -->
      <section v-if="!current.installed" class="lp-card hero">
        <div class="hero__head">
          <span class="engine__logo engine__logo--lg" :data-engine="current.engine">{{ logo(current.engine) }}</span>
          <div>
            <h2 class="hero__title">{{ current.display_name }}</h2>
            <p class="hero__desc">{{ t(`dbm.desc.${current.engine}`) }}</p>
          </div>
        </div>
        <div class="hero__label">{{ t('dbm.chooseVersion') }}</div>
        <div v-if="versionsLoading" class="versions">
          <el-skeleton-item v-for="i in 2" :key="i" variant="rect" class="version-skel" />
        </div>
        <div v-else-if="versions.length" class="versions">
          <button
            v-for="v in versions"
            :key="v.id"
            type="button"
            class="version"
            :class="{ 'is-active': chosenVersion === v.id }"
            @click="chosenVersion = v.id"
          >
            <span class="version__label">{{ v.label }}</span>
            <span class="version__tags">
              <span v-if="v.default" class="lp-pill lp-pill--ok">{{ t('dbm.recommended') }}</span>
              <span v-if="v.id === 'mysql'" class="lp-pill lp-pill--accent">{{ t('dbm.mysqlCompat') }}</span>
            </span>
            <span class="version__full lp-mono">{{ v.version }}</span>
          </button>
        </div>
        <p v-else class="lp-dim hero__note">{{ t('dbm.versionsUnknown') }}</p>
        <div class="hero__actions">
          <el-button type="primary" :loading="busy === `install-${current.engine}`" @click="install(current.engine)">
            {{ t('dbm.installVersion', { name: chosenLabel }) }}
          </el-button>
          <span class="lp-dim hero__note">{{ t('dbm.installHint') }}</span>
        </div>
      </section>

      <template v-else>
        <!-- Status -->
        <section class="lp-card status">
          <div class="status__main">
            <span class="lp-pill" :class="current.running ? 'lp-pill--ok' : 'lp-pill--warn'">
              <span class="lp-pill__dot"></span>{{ current.running ? t('dbm.running') : t('dbm.stopped') }}
            </span>
            <span class="status__item"><span class="lp-dim">{{ t('dbm.version') }}</span> <span class="lp-mono">{{ shortVersion(current) }}</span></span>
            <span class="status__item"><span class="lp-dim">{{ t('dbm.port') }}</span> <span class="lp-mono">{{ current.port }}</span></span>
            <button
              v-if="current.running && network"
              type="button"
              class="remote-chip"
              :class="{ 'is-on': network.public }"
              @click="openSettings('conn')"
            >
              <el-icon><Connection /></el-icon>{{ network.public ? t('dbs.remoteOnShort') : t('dbs.remoteOffShort') }}
            </button>
          </div>
          <div class="status__actions">
            <el-button
              v-if="current.engine === 'mysql' && current.running && !current.manageable"
              size="small" type="warning" plain @click="adminDialog = true"
            >{{ t('dbm.connect') }}</el-button>
            <el-button v-if="!current.running" size="small" type="primary" plain :loading="busy === `start-${current.engine}`" @click="startEngine(current.engine)">
              {{ t('dbm.start') }}
            </el-button>
            <template v-else>
              <el-button size="small" :loading="busy === `restart-${current.engine}`" @click="restart(current.engine)">{{ t('dbm.restart') }}</el-button>
              <el-button size="small" :loading="busy === `stop-${current.engine}`" @click="stopEngine(current.engine)">{{ t('dbs.stop') }}</el-button>
            </template>
            <el-button size="small" type="primary" :icon="Setting" :disabled="!current.running" @click="openSettings()">{{ t('dbs.settings') }}</el-button>
            <el-dropdown trigger="click" @command="(c: string) => c === 'uninstall' && openUninstall()">
              <el-button size="small" :icon="MoreFilled" />
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item command="uninstall" class="danger-item">{{ t('dbs.uninstall') }}</el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
          </div>
        </section>

        <el-alert
          v-if="!current.manageable"
          :title="engineMessage(current)"
          type="warning"
          :closable="false"
          show-icon
          class="block"
        />

        <!-- Databases -->
        <section v-if="current.engine !== 'redis' && current.manageable" class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title">
              <el-icon><Coin /></el-icon>{{ t('dbm.databases') }}
              <span v-if="!listLoading" class="count">{{ rows.length }}</span>
            </h2>
            <div class="head-tools">
              <el-input v-model="keyword" :prefix-icon="Search" size="small" clearable :placeholder="t('dbm.search')" class="search" />
              <el-button type="primary" size="small" :icon="Plus" @click="openCreate">{{ t('dbm.create') }}</el-button>
            </div>
          </header>
          <div v-if="listLoading" class="lp-card-body">
            <el-skeleton animated :rows="4" />
          </div>
          <div v-else-if="!rows.length && !hasExternal" class="empty">
            <el-icon :size="34" class="empty__icon"><Coin /></el-icon>
            <p class="empty__title">{{ t('dbm.empty') }}</p>
            <p class="lp-dim empty__hint">{{ t('dbm.emptyHint') }}</p>
            <el-button type="primary" :icon="Plus" @click="openCreate">{{ t('dbm.create') }}</el-button>
          </div>
          <p v-if="hasExternal" class="external-hint">
            <el-icon><InfoFilled /></el-icon>{{ t('dbm.externalHint') }}
          </p>
          <el-table v-if="rows.length" :data="filteredRows" :empty-text="t('dbm.noMatch')">
            <el-table-column :label="t('dbm.name')" min-width="160">
              <template #default="{ row }">
                <button type="button" class="db-name" :title="t('dbm.viewTables')" @click="openTables(row)">
                  <span class="lp-mono strong">{{ row.name }}</span>
                </button>
                <el-tag v-if="!row.managed" size="small" type="info" class="ml">{{ t('dbm.external') }}</el-tag>
                <div v-if="row.note" class="lp-dim note">{{ row.note }}</div>
              </template>
            </el-table-column>
            <el-table-column :label="t('dbm.tablesSize')" min-width="120">
              <template #default="{ row }">
                <button type="button" class="db-stats" @click="openTables(row)">
                  <template v-if="row.tables !== null && row.tables !== undefined">{{ t('dbm.tableCount', { n: row.tables }) }} · </template>{{ row.size !== null && row.size !== undefined ? formatBytes(row.size) : '—' }}
                </button>
              </template>
            </el-table-column>
            <el-table-column :label="t('dbm.user')" min-width="130">
              <template #default="{ row }">
                <span v-if="row.managed" class="lp-mono">{{ row.username || '—' }}</span>
                <span v-else-if="row.users?.length" class="lp-mono lp-dim users" :title="row.users.join('\n')">{{ row.users.join(', ') }}</span>
                <span v-else class="lp-dim">—</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('dbm.password')" min-width="200">
              <template #default="{ row }">
                <template v-if="row.password">
                  <span class="lp-mono">{{ shown[row.name] ? row.password : '••••••••••' }}</span>
                  <el-button link size="small" :icon="shown[row.name] ? Hide : View" @click="shown[row.name] = !shown[row.name]" />
                  <el-button link size="small" :icon="CopyDocument" @click="copy(row.password)" />
                </template>
                <span v-else class="lp-dim">—</span>
              </template>
            </el-table-column>
            <el-table-column v-if="engine === 'mysql'" :label="t('dbm.host')" min-width="130">
              <template #default="{ row }">
                <button v-if="row.managed" type="button" class="host-chip" :title="t('dbm.changeHost')" @click="openHost(row)">
                  {{ hostLabel(row.host) }} <el-icon><EditPen /></el-icon>
                </button>
                <span v-else class="lp-mono">{{ row.host || '—' }}</span>
              </template>
            </el-table-column>
            <el-table-column :label="t('actions')" width="320" align="right">
              <template #default="{ row }">
                <el-button v-if="!row.managed" link type="primary" size="small" @click="openAdopt(row)">{{ t('dbm.adopt') }}</el-button>
                <el-button link type="primary" size="small" @click="openConnection(row)">{{ t('dbm.connInfo') }}</el-button>
                <el-button link type="primary" size="small" @click="openBackups(row)">{{ t('dbm.backups') }}</el-button>
                <el-button v-if="row.managed" link type="primary" size="small" @click="resetPassword(row)">{{ t('dbm.changePassword') }}</el-button>
                <el-button link type="danger" size="small" @click="remove(row)">{{ t('delete') }}</el-button>
              </template>
            </el-table-column>
          </el-table>
        </section>

        <!-- Redis -->
        <section v-if="current.engine === 'redis' && current.running" class="lp-card">
          <header class="lp-card-head">
            <h2 class="lp-card-title"><el-icon><Coin /></el-icon>Redis</h2>
            <el-button size="small" :icon="Setting" @click="openSettings('password')">{{ t('dbs.settings') }}</el-button>
          </header>
          <div class="lp-card-body">
            <p class="lp-dim">{{ t('dbm.redisHint') }}</p>
            <dl class="kv">
              <div><dt>{{ t('dbm.address') }}</dt><dd class="lp-mono">{{ network?.public ? `${serverHost}:${current.port}` : `127.0.0.1:${current.port}` }}</dd></div>
              <div>
                <dt>{{ t('dbm.password') }}</dt>
                <dd>
                  <template v-if="current.redis_password">
                    <span class="lp-mono">{{ shown.__redis ? current.redis_password : '••••••••••' }}</span>
                    <el-button link size="small" :icon="shown.__redis ? Hide : View" @click="shown.__redis = !shown.__redis" />
                    <el-button link size="small" :icon="CopyDocument" @click="copy(current.redis_password)" />
                  </template>
                  <span v-else class="lp-dim">{{ t('dbm.noPassword') }}</span>
                </dd>
              </div>
              <div><dt>{{ t('dbs.remote') }}</dt><dd>{{ network?.public ? t('dbs.remoteOnShort') : t('dbs.remoteOffShort') }}</dd></div>
            </dl>
          </div>
        </section>
      </template>
    </template>

    <!-- Create -->
    <el-dialog v-model="createDialog" :title="t('dbm.create')" width="520px" append-to-body>
      <el-form label-position="top" @submit.prevent="submitCreate">
        <el-form-item :label="t('dbm.name')">
          <el-input v-model="form.name" class="lp-mono" placeholder="mysite" @input="syncUser" />
          <span class="hint">{{ t('dbm.nameHint') }}</span>
        </el-form-item>
        <el-form-item :label="t('dbm.user')">
          <el-input v-model="form.username" class="lp-mono" @input="userEdited = true" />
        </el-form-item>
        <el-form-item :label="t('dbm.password')">
          <div class="inline-row">
            <el-input v-model="form.password" class="lp-mono" />
            <el-button @click="form.password = randomPassword()">{{ t('dbm.generate') }}</el-button>
          </div>
        </el-form-item>
        <el-form-item v-if="engine === 'mysql'" :label="t('dbm.host')">
          <el-radio-group v-model="form.hostMode">
            <el-radio-button value="localhost">{{ t('dbm.hostLocal') }}</el-radio-button>
            <el-radio-button value="%">{{ t('dbm.hostAny') }}</el-radio-button>
            <el-radio-button value="custom">{{ t('dbm.hostCustom') }}</el-radio-button>
          </el-radio-group>
          <el-input v-if="form.hostMode === 'custom'" v-model="form.host" class="lp-mono mt" placeholder="203.0.113.7" />
          <span v-if="form.hostMode !== 'localhost'" class="hint">{{ t('dbm.hostRemoteHint') }}</span>
        </el-form-item>
        <el-form-item :label="t('dbm.note')">
          <el-input v-model="form.note" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="createDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="busy === 'create'" @click="submitCreate">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>

    <!-- Tables of a database (whoever created them) -->
    <el-drawer v-model="tablesDrawer" :size="drawerSize" append-to-body class="tables-drawer">
      <template #header>
        <div class="drawer-head">
          <span class="drawer-head__title lp-mono">{{ tablesTarget?.name }}</span>
          <span class="lp-dim">{{ t('dbm.tableCount', { n: tableList.length }) }} · {{ formatBytes(tablesTotal) }}</span>
        </div>
      </template>
      <div class="tables-tools">
        <el-input v-model="tableKeyword" :prefix-icon="Search" size="small" clearable :placeholder="t('dbm.searchTables')" class="search" />
        <el-button size="small" :icon="Refresh" :loading="tablesLoading" @click="loadTables">{{ t('refresh') }}</el-button>
      </div>
      <el-skeleton v-if="tablesLoading && !tableList.length" animated :rows="6" />
      <el-table v-else :data="filteredTables" size="small" :empty-text="tableKeyword ? t('dbm.noMatch') : t('dbm.noTables')">
        <el-table-column :label="t('dbm.table')" min-width="170">
          <template #default="{ row }">
            <span class="lp-mono strong">{{ row.name }}</span>
            <div v-if="row.comment" class="lp-dim note">{{ row.comment }}</div>
          </template>
        </el-table-column>
        <el-table-column :label="t('dbm.rows')" width="100" align="right">
          <template #default="{ row }"><span class="lp-num">{{ row.rows === null ? '—' : '≈ ' + row.rows.toLocaleString() }}</span></template>
        </el-table-column>
        <el-table-column :label="t('dbm.size')" width="100" align="right">
          <template #default="{ row }"><span class="lp-num">{{ formatBytes(row.size) }}</span></template>
        </el-table-column>
        <el-table-column v-if="engine === 'mysql'" :label="t('dbm.engine')" width="90">
          <template #default="{ row }"><span class="lp-dim">{{ row.engine || '—' }}</span></template>
        </el-table-column>
        <el-table-column v-if="engine === 'mysql'" :label="t('dbm.updated')" width="160">
          <template #default="{ row }"><span class="lp-dim lp-num">{{ row.updated || '—' }}</span></template>
        </el-table-column>
      </el-table>
    </el-drawer>

    <!-- Adopt a database created outside the panel -->
    <el-dialog v-model="adoptDialog" :title="t('dbm.adoptTitle', { name: adoptTarget?.name || '' })" width="480px" append-to-body>
      <p class="adopt-desc">{{ t('dbm.adoptDesc') }}</p>
      <el-form label-position="top" @submit.prevent>
        <el-form-item :label="t('dbm.user')">
          <el-input v-model="adoptForm.username" class="lp-mono" />
          <span v-if="adoptTarget?.users?.length" class="hint">{{ t('dbm.adoptExisting', { users: adoptTarget.users.join(', ') }) }}</span>
        </el-form-item>
        <el-form-item :label="t('dbm.password')">
          <div class="inline-row">
            <el-input v-model="adoptForm.password" class="lp-mono" />
            <el-button @click="adoptForm.password = randomPassword()">{{ t('dbm.generate') }}</el-button>
          </div>
        </el-form-item>
        <el-form-item v-if="engine === 'mysql'" :label="t('dbm.host')">
          <el-radio-group v-model="adoptForm.hostMode">
            <el-radio-button value="localhost">{{ t('dbm.hostLocal') }}</el-radio-button>
            <el-radio-button value="%">{{ t('dbm.hostAny') }}</el-radio-button>
            <el-radio-button value="custom">{{ t('dbm.hostCustom') }}</el-radio-button>
          </el-radio-group>
          <el-input v-if="adoptForm.hostMode === 'custom'" v-model="adoptForm.host" class="lp-mono mt" placeholder="203.0.113.7" />
        </el-form-item>
        <el-checkbox v-model="adoptForm.reset">{{ t('dbm.adoptReset') }}</el-checkbox>
        <p class="hint">{{ adoptForm.reset ? t('dbm.adoptResetOn') : t('dbm.adoptResetOff') }}</p>
      </el-form>
      <template #footer>
        <el-button @click="adoptDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="busy === 'adopt'" @click="submitAdopt">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>

    <!-- Change access host -->
    <el-dialog v-model="hostDialog" :title="t('dbm.changeHost')" width="460px" append-to-body>
      <el-form label-position="top">
        <el-form-item :label="t('dbm.host')">
          <el-radio-group v-model="hostForm.mode">
            <el-radio-button value="localhost">{{ t('dbm.hostLocal') }}</el-radio-button>
            <el-radio-button value="%">{{ t('dbm.hostAny') }}</el-radio-button>
            <el-radio-button value="custom">{{ t('dbm.hostCustom') }}</el-radio-button>
          </el-radio-group>
          <el-input v-if="hostForm.mode === 'custom'" v-model="hostForm.host" class="lp-mono mt" :placeholder="network?.your_ip || '203.0.113.7'" />
          <span v-if="hostForm.mode !== 'localhost'" class="hint">{{ t('dbm.hostRemoteHint') }}</span>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="hostDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="busy === 'host'" @click="submitHost">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>

    <!-- Connect an existing MySQL -->
    <el-dialog v-model="adminDialog" :title="t('dbm.connect')" width="460px" append-to-body>
      <p class="dialog-text">{{ t('dbm.connectHint') }}</p>
      <el-form label-position="top">
        <el-form-item :label="t('dbm.user')"><el-input v-model="adminForm.user" class="lp-mono" /></el-form-item>
        <el-form-item :label="t('dbm.password')">
          <el-input v-model="adminForm.password" type="password" show-password />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="adminDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="busy === 'admin'" @click="submitAdmin">{{ t('confirm') }}</el-button>
      </template>
    </el-dialog>

    <!-- Engine settings (connection, passwords, tuning, logs, files) -->
    <DatabaseSettings
      v-if="current"
      v-model="settingsOpen"
      :engine="engine"
      :display-name="current.display_name"
      :users="rows.filter((r) => r.managed)"
      :initial-tab="settingsTab"
      @changed="onSettingsChanged"
    />

    <!-- Uninstall -->
    <el-dialog v-model="uninstallDialog" :title="t('dbs.uninstallTitle', { name: current?.display_name || '' })" width="500px" append-to-body>
      <p class="dialog-text">{{ t('dbs.uninstallText') }}</p>
      <el-checkbox v-model="uninstallPurge">{{ t('dbs.uninstallPurge') }}</el-checkbox>
      <template v-if="uninstallPurge">
        <el-alert :title="t('dbs.uninstallPurgeWarn')" type="error" :closable="false" show-icon class="purge-warn" />
        <el-input v-model="uninstallConfirm" class="lp-mono" :placeholder="t('dbs.uninstallType', { word: engine })" />
      </template>
      <template #footer>
        <el-button @click="uninstallDialog = false">{{ t('cancel') }}</el-button>
        <el-button
          type="danger"
          :loading="busy === 'uninstall'"
          :disabled="uninstallPurge && uninstallConfirm.trim() !== engine"
          @click="submitUninstall"
        >{{ t('dbs.uninstall') }}</el-button>
      </template>
    </el-dialog>

    <!-- Connection info -->
    <el-drawer v-model="connDrawer" :title="`${t('dbm.connInfo')} · ${connTarget?.name || ''}`" size="520px" append-to-body>
      <template v-if="connTarget">
        <dl class="conn">
          <div v-for="item in connItems" :key="item.label" class="conn__row">
            <dt>{{ item.label }}</dt>
            <dd>
              <span class="lp-mono">{{ item.value }}</span>
              <el-button v-if="item.copy" link size="small" :icon="CopyDocument" @click="copy(item.value)" />
            </dd>
          </div>
        </dl>
        <h4 class="conn__h">{{ t('dbm.connLocal') }}</h4>
        <p class="lp-dim conn__p">{{ t('dbm.connLocalText') }}</p>
        <h4 class="conn__h">{{ t('dbm.connNavicat') }}</h4>
        <ol class="conn__steps">
          <li>{{ t('dbm.navicat1', { host: serverHost }) }}</li>
          <li>{{ t('dbm.navicat2', { port: current?.port }) }}</li>
          <li>{{ t('dbm.navicat3') }}</li>
        </ol>
        <h4 class="conn__h">{{ t('dbm.connStrings') }}</h4>
        <div v-for="item in connStrings" :key="item.label" class="snippet">
          <span class="snippet__label">{{ item.label }}</span>
          <code class="snippet__code">{{ item.value }}</code>
          <el-button link size="small" :icon="CopyDocument" @click="copy(item.value)" />
        </div>
      </template>
    </el-drawer>

    <!-- Backups -->
    <el-drawer v-model="backupDrawer" :title="`${t('dbm.backups')} · ${backupTarget?.name || ''}`" size="560px" append-to-body>
      <div class="drawer-actions">
        <el-button type="primary" size="small" :loading="busy === 'backup'" @click="backupNow">{{ t('dbm.backupNow') }}</el-button>
        <span class="hint hint--inline">{{ t('dbm.backupHint') }}</span>
      </div>
      <el-table :data="backups" :empty-text="t('dbm.noBackups')" size="small">
        <el-table-column :label="t('dbm.file')" min-width="200">
          <template #default="{ row }"><span class="lp-mono">{{ row.file }}</span></template>
        </el-table-column>
        <el-table-column :label="t('dbm.size')" width="90">
          <template #default="{ row }">{{ formatBytes(row.size) }}</template>
        </el-table-column>
        <el-table-column :label="t('actions')" width="150">
          <template #default="{ row }">
            <el-button link type="primary" size="small" @click="downloadFileApi(row.path, row.file)">{{ t('dbm.download') }}</el-button>
            <el-button link type="warning" size="small" @click="restore(row)">{{ t('dbm.restore') }}</el-button>
            <el-button link type="danger" size="small" @click="removeBackup(row)">{{ t('delete') }}</el-button>
          </template>
        </el-table-column>
      </el-table>
    </el-drawer>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { usePage } from '@/utils/page'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  CircleCheckFilled, CircleCloseFilled, Coin, Connection, CopyDocument, EditPen, Hide, InfoFilled,
  MoreFilled, Plus, Refresh, Search, Setting, View, WarningFilled,
} from '@element-plus/icons-vue'
import {
  backupDatabase, changeDatabaseHost, changeDatabasePassword, createDatabase, deleteDatabase,
  deleteDatabaseBackup, downloadFileApi, getDbEngines, getDbNetwork, getDbVersions, installDbEngine,
  listDatabaseBackups, listDatabases, restartDbEngine, getDatabaseTables, adoptDatabase, restoreDatabase, setMysqlAdmin,
  startDbEngine, stopDbEngine, uninstallDbEngine,
} from '@/api'
import { useI18n } from '@/i18n'
import { formatBytes } from '@/utils/format'
import DatabaseSettings from './DatabaseSettings.vue'

type EngineKind = 'mysql' | 'postgresql' | 'redis'
interface EngineInfo {
  engine: EngineKind
  display_name: string
  installed: boolean
  version: string
  running: boolean
  port: number
  manageable: boolean
  message: string
  redis_password?: string
}
interface DbRow {
  name: string
  username: string | null
  password: string | null
  host: string | null
  note: string | null
  managed: boolean
  tables?: number | null
  size?: number | null
  users?: string[]
}
interface TableRow {
  name: string
  rows: number | null
  size: number
  engine: string | null
  collation: string | null
  comment: string | null
  updated: string | null
}
interface VersionOption { id: string; label: string; version: string; default: boolean }
interface NetworkInfo {
  port: number
  listeners: { address: string; process: string }[]
  public: boolean
  remote_enabled: boolean
  foreign_listener: string | null
  firewall: { backend: string; enabled: boolean; allowed_from: string[] }
  your_ip: string
}

const { t } = useI18n()
const engines = ref<EngineInfo[]>([])
const engine = ref<EngineKind>('mysql')
const rows = ref<DbRow[]>([])
const loaded = ref(false)
const loading = ref(false)
const listLoading = ref(false)
const busy = ref('')
const keyword = ref('')
const shown = reactive<Record<string, boolean>>({})

const current = computed(() => engines.value.find((e) => e.engine === engine.value))
const filteredRows = computed(() => {
  const q = keyword.value.trim().toLowerCase()
  if (!q) return rows.value
  return rows.value.filter((r) => [r.name, r.username, r.note].some((v) => v?.toLowerCase().includes(q)))
})
// How a visitor reaches this server is the best guess at its address.
const serverHost = window.location.hostname

function logo(kind: string) {
  return { mysql: 'My', postgresql: 'Pg', redis: 'Rd' }[kind] || '?'
}

/** "mariadb Ver 15.1 Distrib 10.11.14-MariaDB ..." -> "MariaDB 10.11.14". */
function shortVersion(item: EngineInfo) {
  const text = item.version || ''
  const maria = text.match(/(\d+\.\d+\.\d+)-MariaDB/i)
  if (maria) return `MariaDB ${maria[1]}`
  const mysql = text.match(/Ver (\d+\.\d+\.\d+)/)
  if (item.engine === 'mysql' && mysql) return `MySQL ${mysql[1]}`
  const generic = text.match(/(\d+\.\d+(\.\d+)?)/)
  return generic ? generic[1] : text
}

function engineMessage(item: EngineInfo) {
  const m = item.message || ''
  if (m === 'Not installed') return t('dbm.notInstalled')
  if (m === 'Installed but not running') return t('dbm.notRunning')
  if (m.startsWith('The panel has no administrator login')) return t('dbm.noAdmin')
  if (m.startsWith('Stored admin login failed:')) return t('dbm.adminFailed', { detail: m.replace(/^Stored admin login failed:\s*/, '') })
  return m
}

function hostLabel(host: string | null) {
  if (!host || host === 'localhost') return t('dbm.hostLocalShort')
  if (host === '%') return t('dbm.hostAnyShort')
  return host
}

function randomPassword(length = 20) {
  const alphabet = 'abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789'
  const bytes = crypto.getRandomValues(new Uint8Array(length))
  return Array.from(bytes, (b) => alphabet[b % alphabet.length]).join('')
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    ElMessage.success(t('dbm.copied'))
  } catch {
    ElMessage.warning(text)
  }
}

// --- Loading ---
async function loadEngines() {
  const res: any = await getDbEngines()
  if (res.success) engines.value = res.data
}

async function loadRows() {
  if (!current.value?.manageable || engine.value === 'redis') {
    rows.value = []
    return
  }
  listLoading.value = true
  try {
    const res: any = await listDatabases(engine.value)
    rows.value = res.success ? res.data : []
  } finally {
    listLoading.value = false
  }
}

const network = ref<NetworkInfo | null>(null)
async function loadNetwork() {
  if (!current.value?.running) {
    network.value = null
    return
  }
  const res: any = await getDbNetwork(engine.value).catch(() => null)
  network.value = res?.success ? res.data : null
}

const versions = ref<VersionOption[]>([])
const versionsLoading = ref(false)
const chosenVersion = ref('')
const chosenLabel = computed(() => versions.value.find((v) => v.id === chosenVersion.value)?.label || current.value?.display_name || '')
async function loadVersions() {
  versions.value = []
  chosenVersion.value = ''
  if (!current.value || current.value.installed) return
  versionsLoading.value = true
  try {
    const res: any = await getDbVersions(engine.value)
    versions.value = res.success ? res.data : []
    chosenVersion.value = versions.value.find((v) => v.default)?.id || versions.value[0]?.id || ''
  } catch {
    // Install still works with the distro default.
  } finally {
    versionsLoading.value = false
  }
}

async function loadAll() {
  loading.value = true
  try {
    await loadEngines()
    loaded.value = true
    await Promise.all([loadRows(), loadNetwork(), loadVersions()])
  } catch {
    // Interceptor reports it.
  } finally {
    loading.value = false
  }
}

function selectEngine(kind: EngineKind) {
  if (engine.value === kind) return
  engine.value = kind
  keyword.value = ''
}
watch(engine, () => Promise.all([loadRows(), loadNetwork(), loadVersions()]))

// --- Engine actions ---
async function install(kind: EngineKind) {
  busy.value = `install-${kind}`
  try {
    // The task window shows the live log and the outcome.
    await installDbEngine(kind, chosenVersion.value || undefined)
    await loadAll()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

async function restart(kind: EngineKind) {
  busy.value = `restart-${kind}`
  try {
    const res: any = await restartDbEngine(kind)
    if (res.success) ElMessage.success(t('dbm.restarted'))
    await loadAll()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Start / stop / settings / uninstall ---
async function startEngine(kind: EngineKind) {
  busy.value = `start-${kind}`
  try {
    const res: any = await startDbEngine(kind)
    if (res.success) ElMessage.success(t('dbs.started'))
    await loadAll()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

async function stopEngine(kind: EngineKind) {
  try {
    await ElMessageBox.confirm(t('dbs.stopConfirm', { name: current.value?.display_name || '' }), t('dbs.stop'), { type: 'warning' })
  } catch {
    return
  }
  busy.value = `stop-${kind}`
  try {
    const res: any = await stopDbEngine(kind)
    if (res.success) ElMessage.success(t('dbs.stopped'))
    await loadAll()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

const settingsOpen = ref(false)
const settingsTab = ref('conn')
function openSettings(tab = 'conn') {
  settingsTab.value = tab
  settingsOpen.value = true
}
async function onSettingsChanged() {
  await Promise.all([loadEngines(), loadNetwork()])
}

const uninstallDialog = ref(false)
const uninstallPurge = ref(false)
const uninstallConfirm = ref('')
function openUninstall() {
  uninstallPurge.value = false
  uninstallConfirm.value = ''
  uninstallDialog.value = true
}
async function submitUninstall() {
  busy.value = 'uninstall'
  uninstallDialog.value = false
  try {
    // The task window shows the live log.
    await uninstallDbEngine(engine.value, uninstallPurge.value)
    await loadAll()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Create ---
const createDialog = ref(false)
const userEdited = ref(false)
const form = reactive({ name: '', username: '', password: '', hostMode: 'localhost', host: '', note: '' })

function openCreate() {
  Object.assign(form, { name: '', username: '', password: randomPassword(), hostMode: 'localhost', host: '', note: '' })
  userEdited.value = false
  createDialog.value = true
}

function syncUser() {
  if (!userEdited.value) form.username = form.name
}

async function submitCreate() {
  busy.value = 'create'
  try {
    const res: any = await createDatabase({
      engine: engine.value,
      name: form.name.trim(),
      username: form.username.trim(),
      password: form.password.trim(),
      host: form.hostMode === 'custom' ? form.host.trim() : form.hostMode,
      note: form.note,
    })
    if (res.success) {
      createDialog.value = false
      const d = res.data
      ElMessageBox.alert(
        `${t('dbm.name')}: ${d.name}\n${t('dbm.user')}: ${d.username}\n${t('dbm.password')}: ${d.password}\n${t('dbm.host')}: ${d.host}`,
        t('dbm.created'),
        { customClass: 'credential-box' },
      )
      await loadRows()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Tables ---
const hasExternal = computed(() => rows.value.some((r) => !r.managed))
const tablesDrawer = ref(false)
const tablesTarget = ref<DbRow | null>(null)
const tableList = ref<TableRow[]>([])
const tablesLoading = ref(false)
const tableKeyword = ref('')
const drawerSize = computed(() => (window.innerWidth < 760 ? '100%' : '720px'))
const tablesTotal = computed(() => tableList.value.reduce((sum, row) => sum + (row.size || 0), 0))
const filteredTables = computed(() => {
  const q = tableKeyword.value.trim().toLowerCase()
  return q ? tableList.value.filter((r) => r.name.toLowerCase().includes(q)) : tableList.value
})

function openTables(row: DbRow) {
  if (tablesTarget.value?.name !== row.name) tableList.value = []
  tablesTarget.value = row
  tableKeyword.value = ''
  tablesDrawer.value = true
  void loadTables()
}

async function loadTables() {
  if (!tablesTarget.value) return
  tablesLoading.value = true
  try {
    const res: any = await getDatabaseTables(engine.value, tablesTarget.value.name)
    if (res.success) tableList.value = res.data || []
  } catch {
    // Interceptor reports it.
  } finally {
    tablesLoading.value = false
  }
}

// --- Adopt (databases created outside the panel) ---
const adoptDialog = ref(false)
const adoptTarget = ref<DbRow | null>(null)
const adoptForm = reactive({ username: '', password: '', hostMode: 'localhost', host: '', reset: false })

function openAdopt(row: DbRow) {
  adoptTarget.value = row
  // An existing grant suggests the user that application already uses.
  const first = row.users?.[0] || ''
  const [user, host] = first.includes('@') ? [first.slice(0, first.lastIndexOf('@')), first.slice(first.lastIndexOf('@') + 1)] : [first, '']
  const hostMode = !host || host === 'localhost' ? 'localhost' : host === '%' ? '%' : 'custom'
  Object.assign(adoptForm, {
    username: user || row.name,
    password: user ? '' : randomPassword(),
    hostMode,
    host: hostMode === 'custom' ? host : '',
    reset: false,
  })
  adoptDialog.value = true
}

async function submitAdopt() {
  if (!adoptTarget.value) return
  busy.value = 'adopt'
  try {
    const res: any = await adoptDatabase(engine.value, adoptTarget.value.name, {
      username: adoptForm.username.trim(),
      password: adoptForm.password.trim(),
      host: adoptForm.hostMode === 'custom' ? adoptForm.host.trim() : adoptForm.hostMode,
      reset_password: adoptForm.reset,
    })
    if (res.success) {
      adoptDialog.value = false
      ElMessage.success(t('dbm.adopted'))
      await loadRows()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Access host ---
const hostDialog = ref(false)
const hostTarget = ref<DbRow | null>(null)
const hostForm = reactive({ mode: 'localhost', host: '' })
function openHost(row: DbRow) {
  hostTarget.value = row
  const host = row.host || 'localhost'
  Object.assign(hostForm, host === 'localhost' || host === '%' ? { mode: host, host: '' } : { mode: 'custom', host })
  hostDialog.value = true
}
async function submitHost() {
  if (!hostTarget.value) return
  const host = hostForm.mode === 'custom' ? hostForm.host.trim() : hostForm.mode
  if (!host) return
  busy.value = 'host'
  try {
    const res: any = await changeDatabaseHost(hostTarget.value.name, host)
    if (res.success) {
      hostDialog.value = false
      ElMessage.success(t('dbm.hostChanged'))
      await loadRows()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Connection info ---
const connDrawer = ref(false)
const connTarget = ref<DbRow | null>(null)
function openConnection(row: DbRow) {
  connTarget.value = row
  connDrawer.value = true
}
const connItems = computed(() => {
  const row = connTarget.value
  if (!row) return []
  const remote = engine.value === 'mysql' && network.value?.public && row.host && row.host !== 'localhost'
  return [
    { label: t('dbm.connHost'), value: remote ? serverHost : `127.0.0.1 (${t('dbm.viaSsh')})`, copy: !!remote },
    { label: t('dbm.port'), value: String(current.value?.port ?? ''), copy: false },
    { label: t('dbm.name'), value: row.name, copy: true },
    { label: t('dbm.user'), value: row.username || '—', copy: !!row.username },
    { label: t('dbm.password'), value: row.password || '—', copy: !!row.password },
  ]
})
const connStrings = computed(() => {
  const row = connTarget.value
  if (!row) return []
  const user = row.username || 'user'
  const pass = row.password || 'password'
  const port = current.value?.port
  if (engine.value === 'postgresql') {
    return [
      { label: 'URL', value: `postgresql://${user}:${pass}@127.0.0.1:${port}/${row.name}` },
      { label: 'PHP PDO', value: `pgsql:host=127.0.0.1;port=${port};dbname=${row.name}` },
      { label: 'JDBC', value: `jdbc:postgresql://127.0.0.1:${port}/${row.name}` },
    ]
  }
  return [
    { label: 'URL', value: `mysql://${user}:${pass}@127.0.0.1:${port}/${row.name}` },
    { label: 'PHP PDO', value: `mysql:host=localhost;dbname=${row.name};charset=utf8mb4` },
    { label: 'JDBC', value: `jdbc:mysql://127.0.0.1:${port}/${row.name}?useUnicode=true&characterEncoding=utf8` },
  ]
})

// --- Delete / password ---
async function remove(row: DbRow) {
  try {
    await ElMessageBox.confirm(t('dbm.deleteConfirm', { name: row.name }), t('delete'), {
      type: 'warning',
      confirmButtonText: t('delete'),
      confirmButtonClass: 'el-button--danger',
    })
  } catch {
    return
  }
  const res: any = await deleteDatabase(engine.value, row.name).catch(() => null)
  if (res?.success) {
    ElMessage.success(t('dbm.deleted'))
    await loadRows()
  }
}

async function resetPassword(row: DbRow) {
  try {
    await ElMessageBox.confirm(t('dbm.passwordConfirm', { name: row.username || '' }), t('dbm.changePassword'), { type: 'warning' })
  } catch {
    return
  }
  const res: any = await changeDatabasePassword(engine.value, row.name).catch(() => null)
  if (res?.success) {
    ElMessageBox.alert(`${t('dbm.password')}: ${res.data.password}`, t('dbm.passwordChanged'), { customClass: 'credential-box' })
    await loadRows()
  }
}

// --- MySQL admin / Redis password ---
const adminDialog = ref(false)
const adminForm = reactive({ user: 'root', password: '' })
async function submitAdmin() {
  busy.value = 'admin'
  try {
    const res: any = await setMysqlAdmin({ ...adminForm })
    if (res.success) {
      adminDialog.value = false
      ElMessage.success(t('dbm.connected'))
      await loadAll()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

// --- Backups ---
const backupDrawer = ref(false)
const backupTarget = ref<DbRow | null>(null)
const backups = ref<any[]>([])

async function loadBackups() {
  if (!backupTarget.value) return
  const res: any = await listDatabaseBackups(engine.value, backupTarget.value.name).catch(() => null)
  backups.value = res?.success ? res.data : []
}

async function openBackups(row: DbRow) {
  backupTarget.value = row
  backupDrawer.value = true
  await loadBackups()
}

async function backupNow() {
  if (!backupTarget.value) return
  busy.value = 'backup'
  try {
    const res: any = await backupDatabase(engine.value, backupTarget.value.name)
    if (res.success) ElMessage.success(t('dbm.backedUp'))
    await loadBackups()
  } catch {
    // Interceptor reports it.
  } finally {
    busy.value = ''
  }
}

async function restore(row: any) {
  if (!backupTarget.value) return
  try {
    await ElMessageBox.confirm(t('dbm.restoreConfirm', { file: row.file }), t('dbm.restore'), { type: 'warning' })
  } catch {
    return
  }
  const loadingMsg = ElMessage.info({ message: t('dbm.restoring'), duration: 0 })
  const res: any = await restoreDatabase(engine.value, backupTarget.value.name, row.file).catch(() => null)
  loadingMsg.close()
  if (res?.success) ElMessage.success(t('dbm.restored'))
}

async function removeBackup(row: any) {
  if (!backupTarget.value) return
  try {
    await ElMessageBox.confirm(t('dbm.deleteBackupConfirm', { file: row.file }), t('delete'), { type: 'warning' })
  } catch {
    return
  }
  const res: any = await deleteDatabaseBackup(engine.value, backupTarget.value.name, row.file).catch(() => null)
  if (res?.success) await loadBackups()
}

onMounted(loadAll)
usePage(loadAll)
</script>

<style scoped lang="scss">
/* ---------- Engine switcher ---------- */
.engines {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--lp-3);
  margin-bottom: var(--lp-4);
}
.engines__skeleton { display: contents; }
.engine-skel { height: 64px; border-radius: var(--lp-r-lg); }
.engine {
  display: flex;
  align-items: center;
  gap: var(--lp-3);
  padding: var(--lp-3) var(--lp-4);
  background: var(--lp-surface);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  cursor: pointer;
  text-align: left;
  font: inherit;
  color: inherit;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
  &:hover { border-color: var(--lp-accent); }
  &.is-active { border-color: var(--lp-accent); box-shadow: 0 0 0 1px var(--lp-accent) inset; background: var(--lp-accent-soft); }
}
.engine__logo {
  display: grid;
  place-items: center;
  flex-shrink: 0;
  width: 36px;
  height: 36px;
  border-radius: var(--lp-r-sm);
  font-weight: 700;
  font-size: 13px;
  color: #fff;
  background: #00758f;
  &[data-engine='postgresql'] { background: #336791; }
  &[data-engine='redis'] { background: #d82c20; }
  &--lg { width: 52px; height: 52px; font-size: 17px; border-radius: var(--lp-r-lg); }
}
.engine__text { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
.engine__name { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.engine__sub { display: flex; align-items: center; gap: 6px; font-size: var(--lp-text-xs); color: var(--lp-text-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.dot { width: 7px; height: 7px; border-radius: 50%; background: var(--lp-text-faint); flex-shrink: 0; }
.dot--ok { background: var(--lp-ok); }
.dot--warn { background: var(--lp-warn); }

/* ---------- Install hero ---------- */
.hero { padding: var(--lp-5); }
.hero__head { display: flex; align-items: center; gap: var(--lp-4); margin-bottom: var(--lp-5); }
.hero__title { margin: 0 0 4px; font-size: var(--lp-text-lg); }
.hero__desc { margin: 0; color: var(--lp-text-dim); font-size: var(--lp-text-sm); line-height: 1.6; }
.hero__label { font-size: var(--lp-text-xs); color: var(--lp-text-soft); margin-bottom: var(--lp-2); }
.hero__actions { display: flex; align-items: center; gap: var(--lp-3); flex-wrap: wrap; margin-top: var(--lp-5); }
.hero__note { font-size: var(--lp-text-xs); margin: 0; }
.versions { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: var(--lp-3); }
.version-skel { height: 76px; border-radius: var(--lp-r-lg); }
.version {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: var(--lp-3) var(--lp-4);
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-lg);
  background: var(--lp-surface);
  text-align: left;
  cursor: pointer;
  font: inherit;
  color: inherit;
  &:hover { border-color: var(--lp-accent); }
  &.is-active { border-color: var(--lp-accent); box-shadow: 0 0 0 1px var(--lp-accent) inset; }
}
.version__label { font-weight: 600; }
.version__tags { display: flex; gap: 6px; flex-wrap: wrap; }
.version__full { font-size: 11px; color: var(--lp-text-faint); }

/* ---------- Status bar ---------- */
.status {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--lp-3);
  flex-wrap: wrap;
  padding: var(--lp-3) var(--lp-4);
  margin-bottom: var(--lp-4);
}
.status__main { display: flex; align-items: center; gap: var(--lp-4); flex-wrap: wrap; }
.status__item { font-size: var(--lp-text-sm); }
.status__actions { display: flex; gap: var(--lp-2); flex-wrap: wrap; }
.block { margin-bottom: var(--lp-4); }
.lp-card + .lp-card { margin-top: var(--lp-4); }

/* ---------- Database list ---------- */
.count {
  margin-left: 6px;
  padding: 0 7px;
  border-radius: var(--lp-r-full);
  background: var(--lp-muted-soft);
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  font-weight: 500;
}
.head-tools { display: flex; gap: var(--lp-2); align-items: center; }
.search { width: 200px; }
.strong { font-weight: 600; }
.note { font-size: var(--lp-text-xs); margin-top: 2px; }
.ml { margin-left: var(--lp-2); }
.mt { margin-top: var(--lp-2); }
.host-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border: 1px solid var(--lp-line);
  border-radius: var(--lp-r-full);
  background: transparent;
  color: var(--lp-text);
  font: inherit;
  font-size: var(--lp-text-xs);
  cursor: pointer;
  &:hover { border-color: var(--lp-accent); color: var(--lp-accent); }
}
.empty { display: flex; flex-direction: column; align-items: center; gap: var(--lp-2); padding: var(--lp-6, 40px) var(--lp-4); text-align: center; }
.empty__icon { color: var(--lp-text-faint); }
.empty__title { margin: 0; font-weight: 600; }
.empty__hint { margin: 0 0 var(--lp-2); font-size: var(--lp-text-sm); }

/* ---------- Remote access ---------- */
.remote-desc { margin: 0 0 var(--lp-3); font-size: var(--lp-text-sm); line-height: 1.6; }
.checks { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--lp-3); }
.check { display: flex; gap: var(--lp-3); align-items: flex-start; }
.check__icon { font-size: 17px; line-height: 1; padding-top: 1px; }
.check__icon.is-ok { color: var(--lp-ok); }
.check__icon.is-warn { color: var(--lp-warn); }
.check__icon.is-bad { color: var(--lp-danger); }
.check__text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.check__title { font-weight: 500; font-size: var(--lp-text-sm); }
.check__detail { font-size: var(--lp-text-xs); color: var(--lp-text-dim); word-break: break-word; }
.tip {
  display: flex;
  gap: var(--lp-2);
  margin-top: var(--lp-4);
  padding: var(--lp-3);
  border-radius: var(--lp-r-sm);
  background: var(--lp-accent-soft);
  font-size: var(--lp-text-xs);
  line-height: 1.6;
  .el-icon { color: var(--lp-accent); margin-top: 2px; flex-shrink: 0; }
}

/* ---------- Dialogs & drawers ---------- */
.dialog-text { margin: 0 0 var(--lp-4); font-size: var(--lp-text-sm); line-height: 1.6; color: var(--lp-text-soft); }
.hint {
  display: block;
  flex-basis: 100%;
  margin-top: 4px;
  font-size: var(--lp-text-xs);
  color: var(--lp-text-dim);
  line-height: 1.5;
  &--inline { display: inline; margin: 0 0 0 var(--lp-2); }
}
.inline-row { display: flex; gap: var(--lp-2); width: 100%; .el-input { flex: 1; min-width: 0; } }
.drawer-actions { display: flex; align-items: center; margin-bottom: var(--lp-3); }
.kv {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: var(--lp-4);
  margin: var(--lp-3) 0 0;
  dt { font-size: var(--lp-text-xs); color: var(--lp-text-dim); margin-bottom: 3px; }
  dd { margin: 0; }
}
.conn { margin: 0; border: 1px solid var(--lp-line); border-radius: var(--lp-r-lg); overflow: hidden; }
.conn__row {
  display: grid;
  grid-template-columns: 110px 1fr;
  align-items: center;
  padding: 8px var(--lp-3);
  border-top: 1px solid var(--lp-line-soft);
  &:first-child { border-top: 0; }
  dt { font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
  dd { margin: 0; display: flex; align-items: center; gap: 4px; min-width: 0; word-break: break-all; }
}
.conn__h { margin: var(--lp-5) 0 var(--lp-2); font-size: var(--lp-text-sm); }
.conn__p { margin: 0; font-size: var(--lp-text-xs); line-height: 1.6; }
.conn__steps { margin: 0; padding-left: 18px; list-style: decimal; font-size: var(--lp-text-xs); line-height: 1.8; color: var(--lp-text-soft); }
.snippet { display: grid; grid-template-columns: 70px 1fr auto; align-items: center; gap: var(--lp-2); margin-bottom: 6px; }
.snippet__label { font-size: var(--lp-text-xs); color: var(--lp-text-dim); }
.snippet__code {
  font-family: var(--lp-font-mono);
  font-size: 11px;
  padding: 4px 8px;
  border-radius: var(--lp-r-sm);
  background: var(--lp-muted-soft);
  overflow-x: auto;
  white-space: nowrap;
}

@media (max-width: 720px) {
  .engines { grid-template-columns: 1fr; }
  .search { width: 140px; }
}
</style>

<style lang="scss">
/* Credential pop-ups: keep line breaks so the values can be copied as shown. */
.credential-box .el-message-box__message { white-space: pre-line; font-family: var(--lp-font-mono, monospace); }
.external-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  padding: var(--lp-2) var(--lp-5);
  border-bottom: 1px solid var(--lp-line-soft);
  font-size: var(--lp-text-sm);
  color: var(--lp-text-dim);
}
.db-name,
.db-stats {
  padding: 0;
  border: 0;
  background: none;
  font: inherit;
  color: inherit;
  cursor: pointer;
}
.db-name:hover .strong { color: var(--lp-accent); text-decoration: underline; text-underline-offset: 3px; }
.db-stats { color: var(--lp-text-soft); font-variant-numeric: tabular-nums; }
.db-stats:hover { color: var(--lp-accent); }
.users { display: inline-block; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; vertical-align: bottom; }
.drawer-head { display: flex; flex-direction: column; gap: 2px; }
.drawer-head__title { font-size: var(--lp-text-lg); font-weight: 600; color: var(--lp-text); }
.tables-tools { display: flex; gap: var(--lp-2); margin-bottom: var(--lp-3); }
.tables-tools .search { max-width: 260px; }
.adopt-desc { margin: 0 0 var(--lp-4); color: var(--lp-text-soft); line-height: 1.6; }
.remote-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 24px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--lp-r-full);
  background: var(--lp-muted-soft);
  color: var(--lp-muted);
  font: inherit;
  font-size: var(--lp-text-xs);
  font-weight: 600;
  cursor: pointer;
  &.is-on { background: var(--lp-warn-soft); color: var(--lp-warn); }
  &:hover { filter: brightness(0.96); }
}
.purge-warn { margin: var(--lp-3) 0; }
:global(.danger-item) { color: var(--lp-danger) !important; }
</style>
