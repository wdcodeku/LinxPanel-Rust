<template>
  <div class="file-manager">
    <div class="fm-toolbar">
      <div class="fm-nav">
        <div class="fm-nav-btns">
          <el-tooltip :content="t('fm.goBack')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" :icon="Back" :disabled="!canGoBack" @click="goBack" />
          </el-tooltip>
          <el-tooltip :content="t('fm.goForward')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" :icon="Right" :disabled="!canGoForward" @click="goForward" />
          </el-tooltip>
          <el-tooltip :content="t('fm.goUp')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" :icon="Top" :disabled="atPathRoot" @click="goUp" />
          </el-tooltip>
          <el-tooltip :content="t('refresh')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" :icon="RefreshRight" @click="refresh" />
          </el-tooltip>
          <el-tooltip :content="showHiddenFiles ? t('fm.hideHidden') : t('fm.showHidden')" :show-after="250">
            <el-button class="fm-nav-btn" size="small" @click="toggleHiddenFiles">
              <el-icon :size="15"><Hide v-if="showHiddenFiles" /><View v-else /></el-icon>
            </el-button>
          </el-tooltip>
        </div>
      </div>
      <div class="fm-path-bar" @click="startEditPath">
        <template v-if="!editingPath">
          <el-icon class="fm-path-icon fm-path-icon-clickable" :size="14" :title="pathIconTooltip" @click.stop="onPathIconClick"><FolderOpened /></el-icon>
          <span v-if="showRootLabel" class="fm-path-root" :class="{ 'drop-target': dropTarget === rootPath }" @click.stop="goTo(rootPath)"
            @dragover="onFolderDragOver($event, rootPath, false)" @dragleave="onFolderDragLeave($event, rootPath)" @drop="onFolderDrop($event, rootPath)">{{ rootLabel }}</span>
          <template v-for="(seg, i) in pathSegments" :key="i">
            <el-icon v-if="showRootLabel || i > 0" :size="10" class="fm-path-arrow"><ArrowRight /></el-icon>
            <span class="fm-path-seg" :class="{ 'drop-target': dropTarget === segmentPath(i) }" @click.stop="goTo(segmentPath(i))"
              @dragover="onFolderDragOver($event, segmentPath(i), false)" @dragleave="onFolderDragLeave($event, segmentPath(i))" @drop="onFolderDrop($event, segmentPath(i))">{{ seg }}</span>
          </template>
        </template>
        <input v-else ref="pathInputRef" v-model="pathInputVal" class="fm-path-input"
          @keyup.enter="confirmPath" @keyup.escape="editingPath = false" @blur="editingPath = false" />
      </div>
      <div v-if="clipboard" class="fm-clip">
        <el-icon><Scissor v-if="clipboard.mode === 'cut'" /><DocumentCopy v-else /></el-icon>
        <span>{{ t(clipboard.mode === 'cut' ? 'fm.clipCut' : 'fm.clipCopy', { n: clipboard.paths.length }) }}</span>
        <el-button size="small" type="primary" link :disabled="isVirtualDriveView" @click="paste()">{{ t('fm.pasteHere') }}</el-button>
        <el-icon class="fm-clip__close" :title="t('cancel')" @click="clipboard = null"><Close /></el-icon>
      </div>
      <div class="fm-actions">
        <el-tooltip :content="t('fm.newFolder')" :show-after="400"><el-button size="small" :icon="FolderAdd" @click="showMkdir = true" /></el-tooltip>
        <el-tooltip :content="t('fm.newFile')" :show-after="400"><el-button size="small" :icon="DocumentAdd" @click="showNewFile = true" /></el-tooltip>
        <el-dropdown trigger="click" :disabled="isVirtualDriveView" @command="onUploadCommand">
          <el-button size="small" :icon="Upload" :loading="uploadingFiles" :disabled="isVirtualDriveView" :title="t('fm.uploadFiles')" />
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="files"><el-icon><Document /></el-icon>{{ t('fm.uploadFiles') }}</el-dropdown-item>
              <el-dropdown-item command="folder"><el-icon><Folder /></el-icon>{{ t('fm.uploadFolder') }}</el-dropdown-item>
              <el-dropdown-item command="exec" divided>
                <el-checkbox :model-value="uploadExecutable" size="small" class="fm-exec-check" @click.prevent />
                {{ t('fm.uploadExec') }}
              </el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
        <el-divider direction="vertical" />
        <el-tooltip :content="viewMode === 'list' ? t('gridView') : t('listView')" :show-after="400">
          <el-button size="small" @click="toggleView"><el-icon :size="14"><Grid v-if="viewMode === 'list'" /><List v-else /></el-icon></el-button>
        </el-tooltip>
        <el-tooltip :content="t('terminal')" :show-after="400"><el-button size="small" @click="toggleTerminal"><el-icon :size="14"><Monitor /></el-icon></el-button></el-tooltip>
        <el-tooltip v-if="!pickerMode" :content="t('rb.title')" :show-after="400"><el-button size="small" @click="openRecycleBin"><el-icon :size="14"><Delete /></el-icon></el-button></el-tooltip>
      </div>
      <input ref="uploadInputRef" class="fm-upload-input" type="file" multiple @change="onUploadInputChange" />
      <input ref="folderInputRef" class="fm-upload-input" type="file" webkitdirectory multiple @change="onFolderInputChange" />
    </div>

    <div
      class="fm-body"
      ref="fmBodyRef"
      @mousedown="onBodyMouseDown"
      @contextmenu.prevent="onBodyContext"
      @dragenter.prevent="onUploadDragEnter"
      @dragover.prevent="onUploadDragOver"
      @dragleave.prevent="onUploadDragLeave"
      @drop.prevent="onUploadDrop"
    >
      <div v-if="viewMode === 'list'" class="fm-list" ref="listRef">
        <div class="fm-list-header">
          <span class="fm-col-check" @click.stop @mousedown.stop>
            <el-checkbox
              :model-value="allVisibleSelected"
              :indeterminate="indeterminateVisibleSelected"
              @change="toggleAllVisibleSelection"
            />
          </span>
          <span class="fm-col-name fm-col-name-header" @click="setSort('name')">
            {{ t('fm.name') }}
            <el-icon v-if="sortKey === 'name'" class="fm-sort-icon"><CaretTop v-if="sortOrder === 'asc'" /><CaretBottom v-else /></el-icon>
          </span>
          <span class="fm-col-size fm-col-header-sortable" @click="setSort('size')">
            {{ t('fm.size') }}
            <el-icon v-if="sortKey === 'size'" class="fm-sort-icon"><CaretTop v-if="sortOrder === 'asc'" /><CaretBottom v-else /></el-icon>
          </span>
          <span class="fm-col-time fm-col-header-sortable" @click="setSort('modified')">
            {{ t('fm.modified') }}
            <el-icon v-if="sortKey === 'modified'" class="fm-sort-icon"><CaretTop v-if="sortOrder === 'asc'" /><CaretBottom v-else /></el-icon>
          </span>
          <span class="fm-col-owner">{{ t('fm.owner') }}</span>
          <span class="fm-col-perm fm-col-header-sortable" @click="setSort('permissions')">
            {{ t('fm.permissions') }}
            <el-icon v-if="sortKey === 'permissions'" class="fm-sort-icon"><CaretTop v-if="sortOrder === 'asc'" /><CaretBottom v-else /></el-icon>
          </span>
        </div>
        <div class="fm-list-body" :class="{ 'has-selection': selectedSet.size > 0 }" ref="scrollRef" v-loading="loading" @dragover="onListDragOver">
          <div v-for="f in sortedVisibleFiles" :key="f.path" class="fm-row"
            :class="{ selected: selectedSet.has(f.path), 'drop-target': dropTarget === f.path, 'is-dragged': draggedPaths.includes(f.path) }" :data-path="f.path"
            :draggable="canDrag"
            @dragstart="onItemDragStart($event, f)" @dragend="onItemDragEnd"
            @dragover="f.is_dir && onFolderDragOver($event, f.path)"
            @dragleave="f.is_dir && onFolderDragLeave($event, f.path)"
            @drop="f.is_dir && onFolderDrop($event, f.path)"
            @click.exact="selectOne(f)" @click.ctrl.exact="toggleSelect(f)" @click.meta.exact="toggleSelect(f)" @click.shift.exact="shiftSelect(f)"
            @dblclick="handleOpen(f)" @contextmenu.prevent.stop="onContext($event, f)"
            @mousedown.left.stop="onRowMouseDown($event, f)">
            <span class="fm-col-check" @click.stop @mousedown.stop>
              <el-checkbox :model-value="selectedSet.has(f.path)" @change="(checked) => setSelectedByCheckbox(f, checked)" />
            </span>
            <span class="fm-col-name">
              <el-icon :size="18" :color="f.is_dir ? 'var(--lp-warning)' : fileIconColor(f.name)">
                <Folder v-if="f.is_dir" /><Document v-else />
              </el-icon>
              <span class="fm-fname">{{ f.name }}</span>
            </span>
            <span class="fm-col-size">
              <span
                v-if="f.is_dir"
                class="fm-dir-size-btn"
                :class="{ loading: dirSizeLoadingSet.has(f.path) }"
                @click.stop="calculateDirectorySize(f)"
              >
                {{ getDisplaySize(f) }}
              </span>
              <span v-else>{{ fmtSize(f.size) }}</span>
            </span>
            <span class="fm-col-time" :title="timeTitle(f)">{{ formatShortTime(f.mtime) }}</span>
            <span class="fm-col-owner" :title="f.owner ? `${f.owner}:${f.group}` : ''">{{ f.owner ? `${f.owner}:${f.group}` : '-' }}</span>
            <span class="fm-col-perm">{{ permissionDigits(f.permissions) }}</span>
          </div>
          <div v-if="!loading && sortedVisibleFiles.length === 0" class="fm-empty">
            <el-empty :description="t('fm.emptyDir')" :image-size="64" />
          </div>
        </div>
      </div>

      <div v-else class="fm-grid" :class="{ 'has-selection': selectedSet.size > 0 }" ref="scrollRef" v-loading="loading" @dragover="onListDragOver">
        <div v-for="f in sortedVisibleFiles" :key="f.path" class="fm-grid-item"
          :class="{ selected: selectedSet.has(f.path), 'drop-target': dropTarget === f.path, 'is-dragged': draggedPaths.includes(f.path) }" :data-path="f.path"
          :draggable="canDrag" :title="`${f.name}\n${timeTitle(f)}`"
          @dragstart="onItemDragStart($event, f)" @dragend="onItemDragEnd"
          @dragover="f.is_dir && onFolderDragOver($event, f.path)"
          @dragleave="f.is_dir && onFolderDragLeave($event, f.path)"
          @drop="f.is_dir && onFolderDrop($event, f.path)"
          @click.exact="selectOne(f)" @click.ctrl.exact="toggleSelect(f)" @click.meta.exact="toggleSelect(f)" @click.shift.exact="shiftSelect(f)"
          @dblclick="handleOpen(f)" @contextmenu.prevent.stop="onContext($event, f)"
          @mousedown.left.stop="onRowMouseDown($event, f)">
          <el-checkbox
            class="fm-grid-check"
            :model-value="selectedSet.has(f.path)"
            @change="(checked) => setSelectedByCheckbox(f, checked)"
            @click.stop
            @mousedown.stop
          />
          <el-icon :size="40" :color="f.is_dir ? 'var(--lp-warning)' : fileIconColor(f.name)">
            <Folder v-if="f.is_dir" /><Document v-else />
          </el-icon>
          <span class="fm-grid-name" :title="f.name">{{ f.name }}</span>
        </div>
        <div v-if="!loading && sortedVisibleFiles.length === 0" class="fm-empty" style="grid-column:1/-1">
          <el-empty :description="t('fm.emptyDir')" :image-size="64" />
        </div>
      </div>

      <div v-if="dragSelecting" class="fm-drag-rect" :style="dragRectStyle"></div>
      <transition name="fade">
        <div v-if="uploadDragActive" class="fm-upload-dropzone">
          <el-icon :size="28"><UploadFilled /></el-icon>
          <span>{{ t('fm.dropToUpload') }}</span>
        </div>
      </transition>

      <div v-if="uploadQueue.length" class="fm-uploads" @mousedown.stop @contextmenu.stop>
        <div class="fm-uploads__head" @click="uploadPanelOpen = !uploadPanelOpen">
          <el-icon :size="15"><Upload /></el-icon>
          <span class="fm-uploads__title">
            <template v-if="uploadActive">{{ t('fm.uploadingN', { done: uploader.doneCount.value, total: uploadQueue.filter((u) => u.status !== 'canceled').length }) }}</template>
            <template v-else-if="uploader.failedCount.value">{{ t('fm.uploadFailedN', { n: uploader.failedCount.value }) }}</template>
            <template v-else>{{ t('fm.uploadFinished', { n: uploader.doneCount.value }) }}</template>
          </span>
          <span class="fm-uploads__pct lp-num">
            <template v-if="uploadActive && uploader.speed.value">{{ fmtSize(uploader.speed.value) }}/s · </template>{{ uploader.percent.value }}%
          </span>
          <el-button v-if="uploader.failedCount.value && !uploadActive" link size="small" type="primary" @click.stop="uploader.retryFailed">{{ t('fm.retryAll') }}</el-button>
          <el-button v-if="uploadActive" link size="small" @click.stop="uploader.cancelAll">{{ t('fm.cancelAll') }}</el-button>
          <el-button v-else link size="small" @click.stop="uploader.clearFinished">{{ t('close') }}</el-button>
          <el-icon class="fm-uploads__chevron" :class="{ open: uploadPanelOpen }"><CaretTop /></el-icon>
        </div>
        <el-progress v-if="uploadActive" :percentage="uploader.percent.value" :show-text="false" :stroke-width="3" class="fm-uploads__bar" />
        <div v-show="uploadPanelOpen" class="fm-uploads__list">
          <div v-for="u in visibleUploads" :key="u.id" class="fm-upload" :class="`is-${u.status}`">
            <div class="fm-upload__line">
              <span class="fm-upload__name" :title="`${u.dir}/${u.name}`">{{ u.name }}</span>
              <span class="fm-upload__meta lp-num">
                <template v-if="u.status === 'uploading'">{{ fmtSize(u.loaded) }} / {{ fmtSize(u.file.size) }} · {{ fmtSize(u.speed) }}/s</template>
                <template v-else-if="u.status === 'retrying'">{{ t('fm.uploadRetrying', { n: u.attempt + 1 }) }}</template>
                <template v-else-if="u.status === 'failed'">{{ uploadErrorText(u) }}</template>
                <template v-else>{{ fmtSize(u.file.size) }} · {{ t(`fm.upload_${u.status}`) }}</template>
              </span>
              <el-button v-if="['uploading', 'waiting', 'retrying'].includes(u.status)" link size="small" :icon="Close" :title="t('cancel')" @click="uploader.cancel(u)" />
              <el-button v-else-if="u.status === 'failed' || u.status === 'canceled'" link size="small" :icon="RefreshRight" :title="t('fm.retry')" @click="uploader.retry(u)" />
            </div>
            <el-progress
              v-if="['uploading', 'retrying', 'failed'].includes(u.status)"
              :percentage="uploadPercent(u)"
              :status="u.status === 'failed' ? 'exception' : u.status === 'retrying' ? 'warning' : undefined"
              :stroke-width="4"
              :show-text="false"
            />
          </div>
          <div v-if="uploadHidden > 0" class="fm-upload fm-upload--more">{{ t('fm.uploadMore', { n: uploadHidden }) }}</div>
        </div>
      </div>

      <div v-show="termVisible" class="fm-term-resize" @mousedown.stop.prevent="startTermResize"></div>
      <div v-show="termVisible" class="fm-terminal" :style="{ height: termHeight + 'px' }" @contextmenu.prevent.stop="onTerminalContext">
        <div class="fm-term-header">
          <span class="fm-term-title"><el-icon :size="14"><Monitor /></el-icon> {{ t('terminal') }}</span>
          <span class="fm-term-cwd">{{ currentPath }}</span>
          <span class="fm-term-status" :class="{ online: termConnected }">
            {{ termConnected ? t('fm.termConnected') : t('fm.termDisconnected') }}
          </span>
          <span class="fm-term-follow">
            <el-checkbox v-model="termFollowPath" size="small">{{ t('fm.termFollowDir') }}</el-checkbox>
          </span>
          <div class="fm-term-actions">
            <el-tooltip :content="t('fm.reconnectTerminal')" :show-after="300">
              <el-icon class="fm-term-action" :size="14" @click="reconnectTerminal"><RefreshRight /></el-icon>
            </el-tooltip>
            <el-icon class="fm-term-close" @click="closeTerminal" :size="14"><Close /></el-icon>
          </div>
        </div>
        <div ref="termContainerRef" class="fm-term-xterm"></div>
      </div>
    </div>

    <!-- Context menus -->
    <teleport to="body">
      <transition name="fade">
        <div ref="ctxMenuRef" v-if="ctxVisible" class="fm-ctx-menu" :style="{ left: ctxX + 'px', top: ctxY + 'px' }">
          <div class="fm-ctx-item" @click="ctxAction('open')"><el-icon><FolderOpened /></el-icon> {{ t('open') }}</div>
          <div class="fm-ctx-item" :class="{ 'is-disabled': ctxRenameDisabled }" @click="ctxAction('rename')"><el-icon><Edit /></el-icon> {{ t('rename') }}</div>
          <div class="fm-ctx-item" @click="ctxAction('copy')"><el-icon><CopyDocument /></el-icon> {{ t('fm.copyPath') }}</div>
          <div class="fm-ctx-item" :class="{ 'is-disabled': ctxDownloadDisabled }" @click="ctxAction('download')"><el-icon><Download /></el-icon> {{ t('fm.download') }}</div>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item" @click="ctxAction('clipCopy')"><el-icon><DocumentCopy /></el-icon> {{ t('fm.copy') }}<span class="fm-ctx-key">Ctrl+C</span></div>
          <div class="fm-ctx-item" @click="ctxAction('clipCut')"><el-icon><Scissor /></el-icon> {{ t('fm.cut') }}<span class="fm-ctx-key">Ctrl+X</span></div>
          <div v-if="clipboard && ctxFile?.is_dir" class="fm-ctx-item" @click="ctxAction('pasteInto')"><el-icon><DocumentAdd /></el-icon> {{ t('fm.pasteInto') }}</div>
          <div v-if="ctxFile && !ctxFile.is_dir && isArchive(ctxFile.name)" class="fm-ctx-item" @click="ctxAction('extract')"><el-icon><Files /></el-icon> {{ t('fm.extract') }}</div>
          <div class="fm-ctx-item" @click="ctxAction('compress')"><el-icon><Box /></el-icon> {{ t('fm.compress') }}</div>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item" @click="ctxAction('terminal')"><el-icon><Monitor /></el-icon> {{ t('fm.openTerminal') }}</div>
          <div class="fm-ctx-item" @click="ctxAction('chmod')"><el-icon><Lock /></el-icon> {{ t('fm.chmod') }}</div>
          <div class="fm-ctx-item" @click="ctxAction('props')"><el-icon><InfoFilled /></el-icon> {{ t('fm.properties') }}</div>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item fm-ctx-danger" @click="ctxAction('delete')"><el-icon><Delete /></el-icon> {{ t('delete') }}</div>
        </div>
      </transition>
    </teleport>
    <teleport to="body">
      <transition name="fade">
        <div ref="bgCtxMenuRef" v-if="bgCtxVisible" class="fm-ctx-menu" :style="{ left: bgCtxX + 'px', top: bgCtxY + 'px' }">
          <div class="fm-ctx-item" @click="bgCtxAction('newFile')"><el-icon><DocumentAdd /></el-icon> {{ t('fm.newFile') }}</div>
          <div class="fm-ctx-item" @click="bgCtxAction('newDir')"><el-icon><FolderAdd /></el-icon> {{ t('fm.newFolder') }}</div>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item" @click="bgCtxAction('terminal')"><el-icon><Monitor /></el-icon> {{ t('fm.openInTerminal') }}</div>
          <div class="fm-ctx-item" @click="bgCtxAction('refresh')"><el-icon><RefreshRight /></el-icon> {{ t('refresh') }}</div>
          <div v-if="clipboard" class="fm-ctx-item" @click="bgCtxAction('paste')"><el-icon><DocumentAdd /></el-icon> {{ t('fm.pasteN', { n: clipboard.paths.length }) }}<span class="fm-ctx-key">Ctrl+V</span></div>
          <template v-if="selectedSet.size > 0">
            <div class="fm-ctx-sep"></div>
            <div class="fm-ctx-item" @click="bgCtxAction('copy')"><el-icon><CopyDocument /></el-icon> {{ t('fm.copyPath') }}</div>
            <div class="fm-ctx-item" @click="bgCtxAction('download')"><el-icon><Download /></el-icon> {{ t('fm.download') }}</div>
            <div class="fm-ctx-item" @click="bgCtxAction('clipCopy')"><el-icon><DocumentCopy /></el-icon> {{ t('fm.copy') }}</div>
            <div class="fm-ctx-item" @click="bgCtxAction('clipCut')"><el-icon><Scissor /></el-icon> {{ t('fm.cut') }}</div>
            <div class="fm-ctx-item" @click="bgCtxAction('compress')"><el-icon><Box /></el-icon> {{ t('fm.compress') }}</div>
            <div class="fm-ctx-item" @click="bgCtxAction('chmod')"><el-icon><Lock /></el-icon> {{ t('fm.chmod') }}</div>
            <div class="fm-ctx-item" @click="bgCtxAction('props')"><el-icon><InfoFilled /></el-icon> {{ t('fm.properties') }}</div>
            <div class="fm-ctx-item fm-ctx-danger" @click="bgCtxAction('delete')"><el-icon><Delete /></el-icon> {{ t('delete') }}</div>
          </template>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item" @click="bgCtxAction('selectAll')"><el-icon><Select /></el-icon> {{ t('selectAll') }}</div>
        </div>
      </transition>
    </teleport>
    <teleport to="body">
      <transition name="fade">
        <div ref="termCtxMenuRef" v-if="termCtxVisible" class="fm-ctx-menu" :style="{ left: termCtxX + 'px', top: termCtxY + 'px' }">
          <div class="fm-ctx-item" :class="{ 'is-disabled': termCopyDisabled }" @click="termCtxAction('copy')">
            <el-icon><CopyDocument /></el-icon> {{ t('fm.termCopy') }}
          </div>
          <div class="fm-ctx-item" @click="termCtxAction('paste')">
            <el-icon><CopyDocument /></el-icon> {{ t('fm.termPaste') }}
          </div>
          <div class="fm-ctx-sep"></div>
          <div class="fm-ctx-item" @click="termCtxAction('clear')">
            <el-icon><Delete /></el-icon> {{ t('fm.termClear') }}
          </div>
        </div>
      </transition>
    </teleport>

    <!-- Dialogs -->
    <el-dialog v-model="deleteDialog" :title="t('fm.deleteTitle', { n: deleteItems.length })" width="480px" append-to-body>
      <ul class="fm-del-list">
        <li v-for="f in deleteItems.slice(0, 8)" :key="f.path">
          <el-icon :color="f.is_dir ? 'var(--lp-warning)' : undefined"><Folder v-if="f.is_dir" /><Document v-else /></el-icon>
          <span class="lp-truncate" :title="f.path">{{ f.name }}</span>
        </li>
        <li v-if="deleteItems.length > 8" class="fm-del-more">{{ t('fm.andMore', { n: deleteItems.length - 8 }) }}</li>
      </ul>
      <template v-if="deleteInBin"><p class="fm-del-note is-danger">{{ t('fm.deleteInBin') }}</p></template>
      <template v-else-if="recycleEnabled">
        <el-checkbox v-model="deletePermanent">{{ t('fm.deletePermanent') }}</el-checkbox>
        <p class="fm-del-note" :class="{ 'is-danger': deletePermanent }">{{ deletePermanent ? t('fm.deletePermanentHint') : t('fm.deleteToBinHint') }}</p>
      </template>
      <p v-else class="fm-del-note is-danger">{{ t('fm.recycleOff') }}</p>
      <template #footer>
        <el-button @click="deleteDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="danger" :loading="deleting" @click="confirmDelete">
          {{ deletePermanent || !recycleEnabled || deleteInBin ? t('fm.deleteForever') : t('fm.moveToBin') }}
        </el-button>
      </template>
    </el-dialog>
    <RecycleBin v-if="recycleMounted" v-model="recycleOpen" @restored="loadFiles()" />

    <!-- Loaded on first use: CodeMirror is a separate chunk. -->
    <FileEditor v-if="editorMounted" v-model="showEditor" :path="editingFile" @saved="loadFiles()" />
    <el-dialog v-model="showMkdir" :title="t('fm.newFolder')" width="400px" v-drag :modal="false" :close-on-click-modal="false">
      <el-input v-model="newDirName" :placeholder="t('fm.folderName')" @keyup.enter="doMkdir" autofocus />
      <template #footer><el-button @click="showMkdir = false">{{ t('cancel') }}</el-button><el-button type="primary" @click="doMkdir">{{ t('create') }}</el-button></template>
    </el-dialog>
    <el-dialog v-model="showNewFile" :title="t('fm.newFile')" width="400px" v-drag :modal="false" :close-on-click-modal="false">
      <el-input v-model="newFileName" :placeholder="t('fm.fileName')" @keyup.enter="doNewFile" autofocus />
      <template #footer><el-button @click="showNewFile = false">{{ t('cancel') }}</el-button><el-button type="primary" @click="doNewFile">{{ t('create') }}</el-button></template>
    </el-dialog>
    <el-dialog v-model="showRename" :title="t('rename')" width="400px" v-drag :modal="false" :close-on-click-modal="false">
      <el-input v-model="renameTo" @keyup.enter="doRename" autofocus />
      <template #footer><el-button @click="showRename = false">{{ t('cancel') }}</el-button><el-button type="primary" @click="doRename">{{ t('ok') }}</el-button></template>
    </el-dialog>
    <PermissionDialog v-model="showChmod" :targets="chmodItems" @done="loadFiles" />

    <el-dialog v-model="pasteDialog" :title="t('fm.pasteConflictTitle')" width="460px" append-to-body>
      <p class="fm-dialog-text">{{ t('fm.pasteConflictText', { n: pasteClashes.length, names: pasteClashes.slice(0, 3).join(', ') }) }}</p>
      <el-radio-group v-model="pasteConflict" class="fm-radio-col">
        <el-radio value="rename">{{ t('fm.conflictRename') }}</el-radio>
        <el-radio value="overwrite">{{ t('fm.conflictOverwrite') }}</el-radio>
        <el-radio value="skip">{{ t('fm.conflictSkip') }}</el-radio>
      </el-radio-group>
      <template #footer>
        <el-button @click="pasteDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" @click="doPaste(pasteConflict)">{{ t('ok') }}</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="compressDialog" :title="t('fm.compress')" width="480px" append-to-body>
      <p class="fm-dialog-text">{{ compressForm.names.length === 1 ? compressForm.names[0] : t('fm.batchTargetN', { n: compressForm.names.length }) }}</p>
      <el-form label-position="top" @submit.prevent="doCompress">
        <el-form-item :label="t('fm.archiveFormat')">
          <el-radio-group v-model="compressForm.format">
            <el-radio-button value="zip">zip</el-radio-button>
            <el-radio-button value="tar.gz">tar.gz</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item :label="t('fm.archiveName')">
          <el-input v-model="compressForm.name" class="lp-mono" @keyup.enter="doCompress" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="compressDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="compressing" @click="doCompress">{{ t('fm.compress') }}</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="extractDialog" :title="t('fm.extract')" width="520px" append-to-body>
      <p class="fm-dialog-text lp-mono">{{ fileNameOfPath(extractForm.path) }}</p>
      <el-form label-position="top" @submit.prevent="doExtract">
        <el-form-item :label="t('fm.extractTo')">
          <el-input v-model="extractForm.dest" class="lp-mono" />
        </el-form-item>
        <el-checkbox v-model="extractForm.intoFolder">{{ t('fm.extractIntoFolder', { name: extractBase }) }}</el-checkbox>
        <el-checkbox v-model="extractForm.overwrite">{{ t('fm.extractOverwrite') }}</el-checkbox>
        <p class="fm-dialog-hint">{{ t('fm.extractResult') }} <span class="lp-mono">{{ extractTarget }}</span></p>
      </el-form>
      <template #footer>
        <el-button @click="extractDialog = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="extracting" @click="doExtract">{{ t('fm.extract') }}</el-button>
      </template>
    </el-dialog>
    <el-dialog v-model="showProps" :title="t('fm.fileProps')" width="450px" v-drag :modal="false" :close-on-click-modal="false">
      <el-descriptions :column="1" border size="small">
        <template v-if="!propsData.is_batch">
          <el-descriptions-item :label="t('fm.name')">{{ propsData.name }}</el-descriptions-item>
          <el-descriptions-item :label="t('fm.path')">{{ propsData.path }}</el-descriptions-item>
          <el-descriptions-item :label="t('fm.type')">{{ propsData.is_dir ? t('fm.folder') : t('fm.file') }}</el-descriptions-item>
          <el-descriptions-item :label="t('fm.size')">{{ fmtSize(propsData.size || 0) }}</el-descriptions-item>
          <el-descriptions-item :label="t('fm.modified')">{{ propsData.mtime ? `${formatFullTime(propsData.mtime)} (${formatRelative(propsData.mtime)})` : propsData.modified }}</el-descriptions-item>
          <el-descriptions-item :label="t('fm.permissions')">{{ permissionDigits(propsData.permissions) }}</el-descriptions-item>
        </template>
        <template v-else>
          <el-descriptions-item :label="t('fm.selectedCount')">{{ propsData.count }}</el-descriptions-item>
          <el-descriptions-item :label="t('fm.file')">{{ propsData.file_count }}</el-descriptions-item>
          <el-descriptions-item :label="t('fm.folder')">{{ propsData.dir_count }}</el-descriptions-item>
          <el-descriptions-item :label="t('fm.totalSize')">{{ fmtSize(propsData.total_size || 0) }}</el-descriptions-item>
          <el-descriptions-item :label="t('fm.selectedPaths')">
            <div class="fm-props-path-list">
              <div v-for="path in propsData.paths || []" :key="path" class="fm-props-path">{{ path }}</div>
            </div>
          </el-descriptions-item>
        </template>
      </el-descriptions>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, nextTick, onMounted, onUnmounted, h } from 'vue'
import { usePage } from '@/utils/page'
import { useRoute } from 'vue-router'
import PermissionDialog from '@/components/PermissionDialog.vue'
import { createUploader, droppedFiles, pickedFolderFiles, type UploadItem } from '@/utils/uploader'
import { localizeApiMessage } from '@/utils/request'
import { Terminal } from 'xterm'
import { FitAddon } from '@xterm/addon-fit'
import 'xterm/css/xterm.css'
import { listFiles, getFileRoots, getDirectorySize, downloadFileApi, writeFileContent, mkdirApi, pathState, extractArchive, compressFiles, transferFiles, deletePaths, getRecycleBin, renamePathApi, fileInfo } from '@/api'
import { useI18n } from '@/i18n'
import { defineAsyncComponent } from 'vue'
const FileEditor = defineAsyncComponent(() => import('@/components/editor/FileEditor.vue'))
import RecycleBin from './RecycleBin.vue'
import { formatFullTime, formatRelative, formatShortTime } from '@/utils/format'
import { RefreshRight, FolderAdd, DocumentAdd, Upload, UploadFilled, Back, Right, Top, ArrowRight, Close, CopyDocument, Grid, List, Delete, Lock, InfoFilled, Select, View, Hide, Download, CaretTop, CaretBottom } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'

const { t } = useI18n()

const props = defineProps<{ initPath?: string; pickerMode?: boolean }>()
const emit = defineEmits<{ (e: 'select', path: string): void }>()

type FileRoot = { path: string; label: string }

const route = useRoute()
const routePath = computed(() => {
  const p = route.query.path
  return typeof p === 'string' ? p : ''
})
// The last directory browsed comes back next time (also after logging in
// again: logout only clears the session).
const LAST_PATH_KEY = 'fm_last_path'
function savedPath() {
  if (props.pickerMode) return ''
  try { return localStorage.getItem(LAST_PATH_KEY) || '' } catch { return '' }
}
const restoredPath = props.initPath || routePath.value ? '' : savedPath()
const currentPath = ref(normPath(props.initPath || routePath.value || restoredPath || '/'))
const files = ref<any[]>([])
const loading = ref(false)
const viewMode = ref<'list' | 'grid'>('list')

const selectedSet = reactive(new Set<string>())
const lastClicked = ref('')

const navHistory = ref<string[]>([currentPath.value])
const navIndex = ref(0)
const canGoBack = computed(() => navIndex.value > 0)
const canGoForward = computed(() => navIndex.value < navHistory.value.length - 1)
const isWindowsHost = ref(false)
const fileRoots = ref<FileRoot[]>([])
const windowsDriveRoots = computed(() => fileRoots.value.filter((item) => isWindowsPath(item.path)))
const WINDOWS_ROOTS_VIEW = '__LINXPANEL_WINDOWS_DRIVES__'
const SHOW_HIDDEN_STORAGE_KEY = 'fm_show_hidden_files'
const showHiddenFiles = ref(localStorage.getItem(SHOW_HIDDEN_STORAGE_KEY) === '1')

const editingPath = ref(false)
const pathInputVal = ref('')
const pathInputRef = ref<HTMLInputElement>()
const uploadInputRef = ref<HTMLInputElement>()
const folderInputRef = ref<HTMLInputElement>()
const uploadingFiles = ref(false)
const uploadDragActive = ref(false)
let uploadDragDepth = 0

const showEditor = ref(false)
const editingFile = ref('')
const saving = ref(false)
const showMkdir = ref(false)
const newDirName = ref('')
const showNewFile = ref(false)
const newFileName = ref('')
const showRename = ref(false)
const renameFrom = ref('')
const renameTo = ref('')
const showChmod = ref(false)
const chmodPath = ref('')
const chmodTargets = ref<string[]>([])
const showProps = ref(false)
const propsData = ref<any>({})
const dirSizeCache = ref<Record<string, number>>({})
const dirSizeLoadingSet = reactive(new Set<string>())

type SortKey = 'name' | 'size' | 'modified' | 'permissions'
const sortKey = ref<SortKey>('name')
const sortOrder = ref<'asc' | 'desc'>('asc')

const ctxVisible = ref(false)
const ctxX = ref(0)
const ctxY = ref(0)
const ctxFile = ref<any>(null)
const bgCtxVisible = ref(false)
const bgCtxX = ref(0)
const bgCtxY = ref(0)
const termCtxVisible = ref(false)
const termCtxX = ref(0)
const termCtxY = ref(0)
const termCopyDisabled = ref(false)
const termSelectionSnapshot = ref('')
const ctxMenuRef = ref<HTMLElement>()
const bgCtxMenuRef = ref<HTMLElement>()
const termCtxMenuRef = ref<HTMLElement>()
const CTX_MENU_EDGE_GAP = 8

const termVisible = ref(false)
const termConnected = ref(false)
const TERM_FOLLOW_STORAGE_KEY = 'fm_term_follow_dir'
const termFollowPath = ref(localStorage.getItem(TERM_FOLLOW_STORAGE_KEY) !== '0')
const termHeight = ref(240)
const termContainerRef = ref<HTMLElement>()
const TERM_MIN_HEIGHT = 140
let termResizeStartY = 0
let termResizeStartHeight = 240
let termSocket: WebSocket | null = null
let termInstance: Terminal | null = null
let termFitAddon: FitAddon | null = null
let termResizeObserver: ResizeObserver | null = null
let termThemeObserver: MutationObserver | null = null
let termReconnectTimer: number | null = null
let termDisableAutoReconnect = false
let termApplyingSyncedPath = false

const fmBodyRef = ref<HTMLElement>()
const scrollRef = ref<HTMLElement>()
const listRef = ref<HTMLElement>()
const gridRef = ref<HTMLElement>()

const dragSelecting = ref(false)
const dragStartX = ref(0)
const dragStartY = ref(0)
const dragCurX = ref(0)
const dragCurY = ref(0)
let preSelectSnapshot = new Set<string>()

const dragRectStyle = computed(() => {
  const x1 = Math.min(dragStartX.value, dragCurX.value)
  const y1 = Math.min(dragStartY.value, dragCurY.value)
  const x2 = Math.max(dragStartX.value, dragCurX.value)
  const y2 = Math.max(dragStartY.value, dragCurY.value)
  return { left: x1 + 'px', top: y1 + 'px', width: (x2 - x1) + 'px', height: (y2 - y1) + 'px' }
})

function isWindowsPath(path: string) {
  return /^\/?[A-Za-z]:(?:\/|$)/.test(path.trim())
}

function normPath(path: string) {
  const raw = (path || '').trim()
  if (raw === WINDOWS_ROOTS_VIEW || raw === `/${WINDOWS_ROOTS_VIEW}`) {
    return WINDOWS_ROOTS_VIEW
  }
  let normalized = (path || '').trim().replace(/\\/g, '/').replace(/\/+/g, '/')
  if (!normalized) return '/'

  if (normalized.startsWith('/') && isWindowsPath(normalized)) {
    normalized = normalized.slice(1)
  }
  if (/^[A-Za-z]:$/.test(normalized)) {
    normalized = `${normalized}/`
  }
  if (/^[A-Za-z]:\//.test(normalized)) {
    normalized = `${normalized[0].toUpperCase()}${normalized.slice(1)}`
    if (normalized.length > 3 && normalized.endsWith('/')) {
      normalized = normalized.slice(0, -1)
    }
    return normalized
  }

  if (!normalized.startsWith('/')) normalized = '/' + normalized
  if (normalized.length > 1 && normalized.endsWith('/')) normalized = normalized.slice(0, -1)
  return normalized
}

function getDriveRoot(path: string) {
  const normalized = normPath(path)
  const matched = normalized.match(/^([A-Za-z]):\//)
  if (matched) return `${matched[1].toUpperCase()}:/`
  const fallback = windowsDriveRoots.value[0]?.path || 'C:/'
  return normPath(fallback)
}

const useWindowsPathStyle = computed(() =>
  (isWindowsHost.value && !isVirtualDriveView.value) || isWindowsPath(currentPath.value),
)
const isVirtualDriveView = computed(() => currentPath.value === WINDOWS_ROOTS_VIEW)
const rootPath = computed(() => {
  if (isVirtualDriveView.value) return WINDOWS_ROOTS_VIEW
  return useWindowsPathStyle.value ? getDriveRoot(currentPath.value) : '/'
})
const rootLabel = computed(() => {
  if (isVirtualDriveView.value) return t('fm.allDrives')
  if (useWindowsPathStyle.value) return rootPath.value
  return ''
})
const showRootLabel = computed(() => rootLabel.value.trim().length > 0)
const currentDriveRoot = computed(() => {
  if (useWindowsPathStyle.value) return getDriveRoot(currentPath.value)
  const fallback = windowsDriveRoots.value[0]?.path || 'C:/'
  return normPath(fallback)
})
const atPathRoot = computed(() => isVirtualDriveView.value || normPath(currentPath.value) === normPath(rootPath.value))
const pathIconTooltip = computed(() => (isWindowsHost.value ? t('fm.showDrives') : t('fm.goRoot')))
const pathSegments = computed(() => {
  if (isVirtualDriveView.value) return []
  const normalized = normPath(currentPath.value)
  if (useWindowsPathStyle.value) {
    const root = getDriveRoot(normalized)
    const rest = normalized.startsWith(root) ? normalized.slice(root.length) : ''
    return rest.split('/').filter(Boolean)
  }
  return normalized.split('/').filter(Boolean)
})

function segmentPath(index: number) {
  if (isVirtualDriveView.value) return WINDOWS_ROOTS_VIEW
  const segments = pathSegments.value.slice(0, index + 1)
  if (useWindowsPathStyle.value) {
    return segments.length ? `${rootPath.value}${segments.join('/')}` : rootPath.value
  }
  return '/' + segments.join('/')
}

function parentPath(path: string) {
  if (path === WINDOWS_ROOTS_VIEW) return WINDOWS_ROOTS_VIEW
  const normalized = normPath(path)
  if (isWindowsPath(normalized)) {
    const driveRoot = getDriveRoot(normalized)
    if (normalized === driveRoot) return driveRoot
    const index = normalized.lastIndexOf('/')
    if (index <= 2) return driveRoot
    return normalized.slice(0, index)
  }
  if (normalized === '/') return '/'
  const index = normalized.lastIndexOf('/')
  if (index <= 0) return '/'
  return normalized.slice(0, index)
}

function joinPath(dir: string, name: string) {
  const base = normPath(dir)
  return base.endsWith('/') ? `${base}${name}` : `${base}/${name}`
}

function fileNameOfPath(path: string) {
  if (path === WINDOWS_ROOTS_VIEW) return t('fm.allDrives')
  const normalized = normPath(path)
  if (isWindowsPath(normalized)) {
    const driveRoot = getDriveRoot(normalized)
    if (normalized === driveRoot) return driveRoot.replace('/', '')
    const index = normalized.lastIndexOf('/')
    return index >= 0 ? normalized.slice(index + 1) : normalized
  }
  if (normalized === '/') return '/'
  const index = normalized.lastIndexOf('/')
  return index >= 0 ? normalized.slice(index + 1) : normalized
}

function onPathIconClick() {
  if (isWindowsHost.value && windowsDriveRoots.value.length > 0) {
    navigate(WINDOWS_ROOTS_VIEW)
    return
  }
  goTo('/')
}

function toggleHiddenFiles() {
  showHiddenFiles.value = !showHiddenFiles.value
  localStorage.setItem(SHOW_HIDDEN_STORAGE_KEY, showHiddenFiles.value ? '1' : '0')
}

function isHiddenFileEntry(item: any) {
  const name = String(item?.name || '')
  if (!name || item?.is_virtual_drive) return false
  return name.startsWith('.')
}

const visibleFiles = computed(() => {
  if (isVirtualDriveView.value) return files.value
  if (showHiddenFiles.value) return files.value
  return files.value.filter((item) => !isHiddenFileEntry(item))
})

const sortedVisibleFiles = computed(() => {
  const items = [...visibleFiles.value]
  const direction = sortOrder.value === 'asc' ? 1 : -1
  const currentSort = sortKey.value

  const compareText = (left: string, right: string) => left.localeCompare(right, undefined, { sensitivity: 'base' })
  const compareNumber = (left: number, right: number) => left - right
  const modifiedTime = (value: string) => {
    const timestamp = Date.parse(value || '')
    return Number.isNaN(timestamp) ? 0 : timestamp
  }

  items.sort((left, right) => {
    if (left.is_dir !== right.is_dir) return left.is_dir ? -1 : 1

    let result = 0
    if (currentSort === 'name') {
      result = compareText(String(left.name || ''), String(right.name || ''))
    } else if (currentSort === 'size') {
      const leftSize = left.is_dir ? Number(dirSizeCache.value[left.path] ?? -1) : Number(left.size || 0)
      const rightSize = right.is_dir ? Number(dirSizeCache.value[right.path] ?? -1) : Number(right.size || 0)
      result = compareNumber(leftSize, rightSize)
    } else if (currentSort === 'modified') {
      result = compareNumber(Number(left.mtime || 0) || modifiedTime(String(left.modified || '')), Number(right.mtime || 0) || modifiedTime(String(right.modified || '')))
    } else {
      result = compareNumber(
        Number(permissionDigits(String(left.permissions || '')) || '0'),
        Number(permissionDigits(String(right.permissions || '')) || '0'),
      )
    }

    if (result === 0) {
      result = compareText(String(left.name || ''), String(right.name || ''))
    }
    return result * direction
  })
  return items
})
/** Tooltip: full date with time zone, and how long ago. */
function timeTitle(f: any) {
  if (!f?.mtime) return f?.modified || ''
  return `${formatFullTime(f.mtime)} · ${formatRelative(f.mtime)}`
}
const selectableVisibleFiles = computed(() => visibleFiles.value.filter((item) => !item?.is_virtual_drive))
const allVisibleSelected = computed(
  () => selectableVisibleFiles.value.length > 0 && selectableVisibleFiles.value.every((item) => selectedSet.has(item.path)),
)
const indeterminateVisibleSelected = computed(() => {
  const total = selectableVisibleFiles.value.length
  if (total === 0) return false
  const selectedCount = selectableVisibleFiles.value.filter((item) => selectedSet.has(item.path)).length
  return selectedCount > 0 && selectedCount < total
})

function triggerUpload() {
  if (isVirtualDriveView.value) {
    ElMessage.warning(t('fm.uploadNeedDir'))
    return
  }
  uploadInputRef.value?.click()
}

// --- Uploads: resumable chunks, folders, retries (see utils/uploader) ---
const UPLOAD_EXEC_KEY = 'fm_upload_exec'
// Uploaded files are executable by default (755): uploaded programs and
// scripts run without a chmod; switch off for plain 644.
const uploadExecutable = ref(localStorage.getItem(UPLOAD_EXEC_KEY) !== '0')
watch(uploadExecutable, (on) => {
  try { localStorage.setItem(UPLOAD_EXEC_KEY, on ? '1' : '0') } catch { /* private mode */ }
})
const uploadPanelOpen = ref(true)
const uploader = createUploader((item) => {
  const top = item.name.split('/')[0]
  if (normPath(item.dir) === normPath(currentPath.value) && top) scheduleUploadRefresh()
})
const uploadQueue = uploader.queue
const uploadActive = uploader.active
watch(uploadActive, (on) => { uploadingFiles.value = on })
/** Large folder uploads: show what matters, not thousands of rows. */
const visibleUploads = computed(() => {
  const list = uploadQueue
  const urgent = list.filter((u) => u.status === 'uploading' || u.status === 'retrying' || u.status === 'failed')
  const waiting = list.filter((u) => u.status === 'waiting').slice(0, 20)
  const finished = list.filter((u) => u.status === 'done' || u.status === 'canceled').slice(-20)
  return [...urgent, ...waiting, ...finished]
})
const uploadHidden = computed(() => uploadQueue.length - visibleUploads.value.length)

function uploadPercent(item: UploadItem) {
  if (item.status === 'done') return 100
  return item.file.size ? Math.min(100, Math.floor((item.loaded / item.file.size) * 100)) : 0
}

function uploadErrorText(item: UploadItem) {
  if (item.error === 'session') return t('fm.uploadSession')
  if (item.error === 'network') return t('fm.uploadNetwork')
  return localizeApiMessage(item.error) || t('fm.uploadFailed')
}

/** Queues files (name = path relative to the current folder). */
async function queueUploads(entries: { file: File; name: string }[], into?: string) {
  if (entries.length === 0) return
  if (isVirtualDriveView.value) {
    ElMessage.warning(t('fm.uploadNeedDir'))
    return
  }
  const dir = into || currentPath.value
  let listing: any[] = files.value
  if (normPath(dir) !== normPath(currentPath.value)) {
    const res: any = await listFiles(dir).catch(() => null)
    listing = res?.success ? res.data : []
  }
  const existing = new Map(listing.map((f: any) => [f.name, f]))
  const tops = [...new Set(entries.map((e) => e.name.split('/')[0]))]
  const clashFiles = tops.filter((name) => existing.get(name) && !existing.get(name).is_dir && entries.some((e) => e.name === name))
  const clashDirs = tops.filter((name) => existing.get(name)?.is_dir && entries.some((e) => e.name.startsWith(`${name}/`)))
  if (clashFiles.length || clashDirs.length) {
    const names = [...clashDirs.map((n) => `${n}/`), ...clashFiles]
    try {
      await ElMessageBox.confirm(
        t('fm.uploadOverwrite', { n: names.length, names: names.slice(0, 3).join(', ') }),
        t('fm.uploadFiles'),
        { type: 'warning', confirmButtonText: t('fm.overwrite') },
      )
    } catch {
      return
    }
  }
  uploader.add(entries, dir, uploadExecutable.value ? '755' : '644')
  uploadPanelOpen.value = true
}

async function uploadPickedFiles(fileList: File[] | FileList) {
  await queueUploads(Array.from(fileList || []).map((file) => ({ file, name: file.name })))
}

let uploadRefreshTimer: ReturnType<typeof setTimeout> | null = null
function scheduleUploadRefresh() {
  if (uploadRefreshTimer) clearTimeout(uploadRefreshTimer)
  uploadRefreshTimer = setTimeout(() => {
    uploadRefreshTimer = null
    void loadFiles()
  }, 600)
}

function triggerFolderUpload() {
  if (isVirtualDriveView.value) {
    ElMessage.warning(t('fm.uploadNeedDir'))
    return
  }
  folderInputRef.value?.click()
}

async function onFolderInputChange(event: Event) {
  const input = event.target as HTMLInputElement
  const picked = input.files ? pickedFolderFiles(input.files) : []
  input.value = ''
  if (!picked.length) {
    ElMessage.warning(t('fm.folderEmpty'))
    return
  }
  await queueUploads(picked)
}

function onUploadCommand(command: string) {
  if (command === 'files') triggerUpload()
  else if (command === 'folder') triggerFolderUpload()
  else if (command === 'exec') uploadExecutable.value = !uploadExecutable.value
}

async function onUploadInputChange(event: Event) {
  const input = event.target as HTMLInputElement
  const filesToUpload = input.files ? Array.from(input.files) : []
  input.value = ''
  await uploadPickedFiles(filesToUpload)
}

function hasDraggedFiles(event: DragEvent) {
  return Array.from(event.dataTransfer?.types || []).includes('Files')
}

function resetUploadDragState() {
  uploadDragDepth = 0
  uploadDragActive.value = false
}

function onUploadDragEnter(event: DragEvent) {
  if (!hasDraggedFiles(event)) return
  uploadDragDepth += 1
  if (!isVirtualDriveView.value) {
    uploadDragActive.value = true
  }
}

function onUploadDragOver(event: DragEvent) {
  if (!hasDraggedFiles(event)) return
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = isVirtualDriveView.value ? 'none' : 'copy'
  }
  if (!isVirtualDriveView.value) {
    uploadDragActive.value = true
  }
}

function onUploadDragLeave(event: DragEvent) {
  if (!hasDraggedFiles(event)) return
  uploadDragDepth = Math.max(0, uploadDragDepth - 1)
  if (uploadDragDepth === 0) {
    uploadDragActive.value = false
  }
}

async function onUploadDrop(event: DragEvent) {
  if (!hasDraggedFiles(event)) return
  resetUploadDragState()
  if (isVirtualDriveView.value) {
    ElMessage.warning(t('fm.uploadNeedDir'))
    return
  }
  const transfer = event.dataTransfer
  if (!transfer) return
  await queueUploads(await droppedFiles(transfer))
}

async function loadFileRoots() {
  try {
    const res: any = await getFileRoots()
    if (!res.success) return
    isWindowsHost.value = !!res.data?.is_windows
    fileRoots.value = Array.isArray(res.data?.roots)
      ? res.data.roots.map((item: FileRoot) => ({ ...item, path: normPath(item.path) }))
      : []
  } catch {
    isWindowsHost.value = isWindowsPath(currentPath.value)
  }

  if (
    isWindowsHost.value &&
    windowsDriveRoots.value.length > 0 &&
    !isWindowsPath(currentPath.value) &&
    currentPath.value !== WINDOWS_ROOTS_VIEW
  ) {
    const firstDrive = normPath(windowsDriveRoots.value[0].path)
    currentPath.value = firstDrive
    navHistory.value = [firstDrive]
    navIndex.value = 0
  }
}

async function loadFiles() {
  loading.value = true
  selectedSet.clear()
  dirSizeLoadingSet.clear()
  try {
    if (isVirtualDriveView.value) {
      files.value = windowsDriveRoots.value.map((root) => ({
        name: root.label,
        path: normPath(root.path),
        is_dir: true,
        size: 0,
        modified: '-',
        permissions: '-',
        is_virtual_drive: true,
      }))
      return
    }
    const res: any = await listFiles(currentPath.value)
    if (res.success) {
      files.value = res.data
      if (!props.pickerMode) {
        try { localStorage.setItem(LAST_PATH_KEY, currentPath.value) } catch { /* private mode */ }
      }
    } else if (restoredPath && currentPath.value === normPath(restoredPath) && navHistory.value.length === 1) {
      // The remembered directory is gone: start at the root instead.
      currentPath.value = rootPath.value
      navHistory.value = [rootPath.value]
      navIndex.value = 0
      const retry: any = await listFiles(currentPath.value)
      if (retry.success) files.value = retry.data
    } else ElMessage.error(res.message)
  } catch { ElMessage.error(t('fm.loadFailed')) }
  finally { loading.value = false }
}

function navigate(p: string) {
  const np = p === WINDOWS_ROOTS_VIEW ? WINDOWS_ROOTS_VIEW : normPath(p || rootPath.value)
  if (np === currentPath.value) return
  currentPath.value = np
  navHistory.value = navHistory.value.slice(0, navIndex.value + 1)
  navHistory.value.push(np)
  navIndex.value = navHistory.value.length - 1
  void loadFiles()
}

function goTo(p: string) { navigate(p) }
function goUp() { navigate(parentPath(currentPath.value)) }
function goBack() { if (canGoBack.value) { navIndex.value--; currentPath.value = normPath(navHistory.value[navIndex.value]); void loadFiles() } }
function goForward() { if (canGoForward.value) { navIndex.value++; currentPath.value = normPath(navHistory.value[navIndex.value]); void loadFiles() } }
function refresh() { void loadFiles() }
function toggleView() { viewMode.value = viewMode.value === 'list' ? 'grid' : 'list' }
function setSort(key: SortKey) {
  if (sortKey.value === key) {
    sortOrder.value = sortOrder.value === 'asc' ? 'desc' : 'asc'
    return
  }
  sortKey.value = key
  sortOrder.value = 'asc'
}

function permissionDigits(rawPermission: string) {
  const text = String(rawPermission || '').trim()
  const direct = text.match(/\b([0-7]{3,4})\b/)
  if (direct) return direct[1].slice(-3)
  return text
}

function getDisplaySize(item: any) {
  if (!item?.is_dir) return fmtSize(Number(item?.size || 0))
  if (dirSizeLoadingSet.has(item.path)) return t('loading')
  const cached = dirSizeCache.value[item.path]
  if (typeof cached === 'number') return fmtSize(cached)
  return t('fm.calcSize')
}

async function calculateDirectorySize(item: any) {
  if (!item?.is_dir || dirSizeLoadingSet.has(item.path)) return
  dirSizeLoadingSet.add(item.path)
  try {
    const res: any = await getDirectorySize(item.path)
    if (res.success) {
      dirSizeCache.value = {
        ...dirSizeCache.value,
        [item.path]: Number(res.data?.size || 0),
      }
    } else {
      ElMessage.error(res.message)
    }
  } finally {
    dirSizeLoadingSet.delete(item.path)
  }
}

function resolveEntriesByPaths(paths: string[]) {
  const indexed = new Map(files.value.map((item: any) => [item.path, item]))
  return paths
    .map((path) => indexed.get(path))
    .filter((item): item is any => !!item && !item.is_virtual_drive)
}

function getSelectedEntries() {
  return resolveEntriesByPaths(Array.from(selectedSet))
}

function resolveActionTargets(primary?: any) {
  if (primary && selectedSet.size > 1 && selectedSet.has(primary.path)) {
    const selectedEntries = getSelectedEntries()
    if (selectedEntries.length > 0) return selectedEntries
  }
  if (primary && !primary.is_virtual_drive) return [primary]
  return getSelectedEntries()
}

const ctxRenameDisabled = computed(() => resolveActionTargets(ctxFile.value).length > 1)
const ctxDownloadDisabled = computed(() => {
  const targets = resolveActionTargets(ctxFile.value)
  return targets.length === 0 || targets.every((item) => item.is_virtual_drive)
})

function setSelectedByCheckbox(file: any, checked: string | number | boolean) {
  if (checked) selectedSet.add(file.path)
  else selectedSet.delete(file.path)
  lastClicked.value = file.path
}

function toggleAllVisibleSelection(checked: string | number | boolean) {
  const shouldSelect = !!checked
  selectableVisibleFiles.value.forEach((item) => {
    if (shouldSelect) selectedSet.add(item.path)
    else selectedSet.delete(item.path)
  })
}

function selectOne(f: any) { selectedSet.clear(); selectedSet.add(f.path); lastClicked.value = f.path }
function toggleSelect(f: any) {
  if (selectedSet.has(f.path)) selectedSet.delete(f.path)
  else selectedSet.add(f.path)
  lastClicked.value = f.path
}
function shiftSelect(f: any) {
  if (!lastClicked.value) { selectOne(f); return }
  const paths = sortedVisibleFiles.value.map(x => x.path)
  const a = paths.indexOf(lastClicked.value), b = paths.indexOf(f.path)
  if (a < 0 || b < 0) return
  const [lo, hi] = a < b ? [a, b] : [b, a]
  for (let i = lo; i <= hi; i++) selectedSet.add(paths[i])
}
function selectAll() { sortedVisibleFiles.value.forEach(f => selectedSet.add(f.path)) }

// --- Copy / cut / paste ---
const ARCHIVE_RE = /\.(zip|jar|war|tar|tgz|tbz2?|txz|tzst|7z|rar|gz)$/i
function isArchive(name: string) { return ARCHIVE_RE.test(name || '') }

// Kept for the browser session, so a page refresh does not lose it.
const CLIPBOARD_KEY = 'fm_clipboard'
function loadClipboard() {
  try {
    const value = JSON.parse(sessionStorage.getItem(CLIPBOARD_KEY) || 'null')
    return value && (value.mode === 'copy' || value.mode === 'cut') && Array.isArray(value.paths) ? value : null
  } catch {
    return null
  }
}
const clipboard = ref<{ mode: 'copy' | 'cut'; paths: string[] } | null>(props.pickerMode ? null : loadClipboard())
watch(clipboard, (value) => {
  if (props.pickerMode) return
  try {
    if (value) sessionStorage.setItem(CLIPBOARD_KEY, JSON.stringify(value))
    else sessionStorage.removeItem(CLIPBOARD_KEY)
  } catch { /* private mode */ }
})
function setClipboard(mode: 'copy' | 'cut', targets: any[]) {
  const paths = targets.filter((f) => f?.path && !f.is_virtual_drive).map((f) => String(f.path))
  if (!paths.length) return
  clipboard.value = { mode, paths }
  ElMessage.success(t(mode === 'cut' ? 'fm.cutN' : 'fm.clipCopiedN', { n: paths.length }))
}

const pasteDialog = ref(false)
const pasteConflict = ref<'rename' | 'overwrite' | 'skip'>('rename')
const pasteClashes = ref<string[]>([])
const pasteDest = ref('')
/** The transfer waiting on the conflict dialog (from the clipboard or a drop). */
let pendingTransfer: { paths: string[]; mode: 'copy' | 'move'; fromClipboard: boolean } | null = null
async function paste(into?: string) {
  const clip = clipboard.value
  if (!clip || isVirtualDriveView.value) return
  await transferTo(clip.paths, clip.mode === 'cut' ? 'move' : 'copy', into || currentPath.value, true)
}
/** Copies or moves `paths` into `destination`, asking first when names clash. */
async function transferTo(paths: string[], mode: 'copy' | 'move', destination: string, fromClipboard = false) {
  const dest = normPath(destination)
  const sameDir = paths.every((p) => normPath(parentPath(p)) === dest)
  if (mode === 'move' && sameDir) {
    ElMessage.info(t('fm.pasteSameDir'))
    return
  }
  // A folder cannot go into itself or one of its own subfolders.
  if (paths.some((p) => dest === normPath(p) || dest.startsWith(normPath(p).replace(/\/?$/, '/')))) {
    ElMessage.warning(t('fm.intoItself'))
    return
  }
  pendingTransfer = { paths, mode, fromClipboard }
  pasteClashes.value = []
  let existing: Set<string>
  if (dest === normPath(currentPath.value)) {
    existing = new Set(files.value.map((f: any) => f.name))
  } else {
    const res: any = await listFiles(dest).catch(() => null)
    existing = new Set(res?.success ? res.data.map((f: any) => f.name) : [])
  }
  const clashes = sameDir ? [] : paths.map(fileNameOfPath).filter((n) => existing.has(n))
  pasteDest.value = dest
  if (clashes.length) {
    pasteClashes.value = clashes
    pasteConflict.value = 'rename'
    pasteDialog.value = true
    return
  }
  await doPaste('rename')
}
async function doPaste(conflict: string) {
  pasteDialog.value = false
  const job = pendingTransfer
  pendingTransfer = null
  const hadClashes = pasteClashes.value.length > 0
  if (!job) return
  const moving = job.mode === 'move'
  const dest = pasteDest.value
  const res: any = await transferFiles(job.paths, dest, job.mode, conflict).catch(() => null)
  if (res?.success) {
    if (moving && job.fromClipboard) clipboard.value = null
    const skipped = Number(res.data?.skipped || 0)
    const text = skipped ? t('fm.pastedSkipped', { n: res.data.files, s: skipped }) : t(moving ? 'fm.movedN' : 'fm.pastedN', { n: res.data?.files ?? job.paths.length })
    // A move whose names were kept can be put back.
    // Only a move that kept every name can be put back exactly.
    if (moving && !skipped && !hadClashes) offerUndoMove(job.paths, dest, text)
    else ElMessage.success(text)
  }
  pasteClashes.value = []
  await loadFiles()
}

/** "Moved 3 items · Undo": moves them back where they came from. */
function offerUndoMove(paths: string[], dest: string, text: string) {
  const groups = new Map<string, string[]>()
  for (const p of paths) {
    const from = parentPath(p)
    const now = joinPath(dest, fileNameOfPath(p))
    groups.set(from, [...(groups.get(from) || []), now])
  }
  const message = ElMessage({
    type: 'success',
    duration: 6000,
    showClose: true,
    message: h('span', { class: 'fm-undo-msg' }, [
      text,
      h('button', {
        type: 'button',
        class: 'fm-undo-btn',
        onClick: async () => {
          message.close()
          for (const [from, moved] of groups) {
            await transferFiles(moved, from, 'move', 'skip').catch(() => null)
          }
          ElMessage.info(t('fm.undone'))
          await loadFiles()
        },
      }, t('fm.undo')),
    ]),
  })
}

// --- Drag and drop: move (Ctrl / Alt: copy) onto folders and the path bar ---
const DRAG_TYPE = 'application/x-linxpanel-paths'
const draggedPaths = ref<string[]>([])
const dropTarget = ref('')
let springTimer: ReturnType<typeof setTimeout> | null = null
const canDrag = computed(() => !props.pickerMode && !isVirtualDriveView.value)

function isInternalDrag(event: DragEvent) {
  return Array.from(event.dataTransfer?.types || []).includes(DRAG_TYPE)
}
function clearSpring() {
  if (springTimer) clearTimeout(springTimer)
  springTimer = null
}
function onItemDragStart(event: DragEvent, f: any) {
  if (!canDrag.value || !event.dataTransfer) return
  if (!selectedSet.has(f.path)) {
    selectedSet.clear()
    selectedSet.add(f.path)
  }
  const paths = sortedVisibleFiles.value.filter((x: any) => selectedSet.has(x.path)).map((x: any) => String(x.path))
  draggedPaths.value = paths
  event.dataTransfer.effectAllowed = 'copyMove'
  event.dataTransfer.setData(DRAG_TYPE, JSON.stringify(paths))
  event.dataTransfer.setData('text/plain', paths.join('\n'))
  // A compact badge instead of a ghost of the whole row.
  const badge = document.createElement('div')
  badge.className = 'fm-drag-badge'
  badge.textContent = paths.length === 1 ? fileNameOfPath(paths[0]) : t('fm.dragN', { n: paths.length })
  document.body.appendChild(badge)
  event.dataTransfer.setDragImage(badge, -12, -12)
  setTimeout(() => badge.remove(), 0)
}
function onItemDragEnd() {
  draggedPaths.value = []
  dropTarget.value = ''
  clearSpring()
}
/** Why `dest` cannot take the dragged items, or '' when it can. */
function dropRefused(dest: string) {
  const target = normPath(dest)
  const dragged = draggedPaths.value.map(normPath)
  if (!dragged.length) return ''
  if (dragged.some((p) => target === p || target.startsWith(p.replace(/\/?$/, '/')))) return 'self'
  if (dragged.every((p) => normPath(parentPath(p)) === target)) return 'same'
  return ''
}
function onFolderDragOver(event: DragEvent, dest: string, spring = true) {
  const internal = isInternalDrag(event)
  if (!internal && !hasDraggedFiles(event)) return
  event.preventDefault()
  event.stopPropagation()
  if (!event.dataTransfer) return
  if (internal && dropRefused(dest)) {
    event.dataTransfer.dropEffect = 'none'
    dropTarget.value = ''
    return
  }
  event.dataTransfer.dropEffect = !internal || event.ctrlKey || event.altKey ? 'copy' : 'move'
  if (dropTarget.value !== dest) {
    dropTarget.value = dest
    clearSpring()
    // Hovering a folder opens it, so a drop can go deeper.
    if (spring) springTimer = setTimeout(() => { springTimer = null; if (dropTarget.value === dest) void goTo(dest) }, 900)
  }
}
function onFolderDragLeave(event: DragEvent, dest: string) {
  const related = event.relatedTarget as Node | null
  if (related && (event.currentTarget as HTMLElement).contains(related)) return
  if (dropTarget.value === dest) dropTarget.value = ''
  clearSpring()
}
async function onFolderDrop(event: DragEvent, dest: string) {
  event.preventDefault()
  event.stopPropagation()
  clearSpring()
  dropTarget.value = ''
  resetUploadDragState()
  if (isInternalDrag(event)) {
    let paths: string[] = []
    try { paths = JSON.parse(event.dataTransfer?.getData(DRAG_TYPE) || '[]') } catch { paths = [] }
    draggedPaths.value = []
    if (paths.length) await transferTo(paths, event.ctrlKey || event.altKey ? 'copy' : 'move', dest)
    return
  }
  if (hasDraggedFiles(event) && event.dataTransfer) {
    await queueUploads(await droppedFiles(event.dataTransfer), dest)
  }
}
/** Dragging near the top or bottom edge of the list scrolls it. */
function onListDragOver(event: DragEvent) {
  const el = scrollRef.value as HTMLElement | undefined
  if (!el || !isInternalDrag(event)) return
  const rect = el.getBoundingClientRect()
  const edge = 40
  if (event.clientY < rect.top + edge) el.scrollTop -= 12
  else if (event.clientY > rect.bottom - edge) el.scrollTop += 12
}

// --- Compress / extract ---
const compressDialog = ref(false)
const compressForm = reactive({ names: [] as string[], name: '', format: 'zip' })
const compressing = ref(false)
function archiveExt(format: string) { return format === 'zip' ? '.zip' : '.tar.gz' }
function openCompress(targets: any[]) {
  const items = targets.filter((f) => f?.path && !f.is_virtual_drive)
  if (!items.length) return
  const names = items.map((f) => String(f.name || fileNameOfPath(f.path)))
  const folder = fileNameOfPath(currentPath.value) || 'archive'
  const base = names.length === 1 ? names[0].replace(/\.[^.]+$/, '') || names[0] : folder
  Object.assign(compressForm, { names, name: base + archiveExt(compressForm.format) })
  compressDialog.value = true
}
watch(() => compressForm.format, (format, previous) => {
  const old = archiveExt(previous)
  if (compressForm.name.endsWith(old)) compressForm.name = compressForm.name.slice(0, -old.length) + archiveExt(format)
})
async function doCompress() {
  const name = compressForm.name.trim()
  if (!name || name.includes('/')) return
  if (files.value.some((f: any) => f.name === name)) {
    ElMessage.error(t('fm.nameExists', { name }))
    return
  }
  compressing.value = true
  try {
    const res: any = await compressFiles(currentPath.value, compressForm.names, name, compressForm.format)
    if (res?.success) {
      compressDialog.value = false
      ElMessage.success(t('fm.compressed', { name }))
    }
  } finally {
    compressing.value = false
    await loadFiles()
  }
}

const extractDialog = ref(false)
const extractForm = reactive({ path: '', dest: '', intoFolder: false, overwrite: true })
const extracting = ref(false)
const extractBase = computed(() =>
  fileNameOfPath(extractForm.path).replace(/\.(tar\.(gz|bz2|xz|zst)|tgz|tbz2?|txz|tzst|zip|jar|war|tar|7z|rar|gz)$/i, '') || 'extracted',
)
const extractTarget = computed(() => {
  const dest = normPath(extractForm.dest.trim() || currentPath.value)
  return extractForm.intoFolder ? normPath(`${dest}/${extractBase.value}`) : dest
})
function openExtract(f: any) {
  Object.assign(extractForm, { path: f.path, dest: currentPath.value, intoFolder: false, overwrite: true })
  extractDialog.value = true
}
async function doExtract() {
  extracting.value = true
  try {
    const res: any = await extractArchive(extractForm.path, extractTarget.value, extractForm.overwrite)
    if (res?.success) {
      extractDialog.value = false
      const skipped = Number(res.data?.skipped || 0)
      ElMessage.success(skipped ? t('fm.extractedSkipped', { n: res.data.files, s: skipped }) : t('fm.extractedN', { n: res.data?.files ?? 0 }))
    }
  } finally {
    extracting.value = false
    await loadFiles()
  }
}

// --- Keyboard: Ctrl+C / X / V / A, Delete, F2 (not while typing anywhere) ---
function onFileKeys(event: KeyboardEvent) {
  if (props.pickerMode || !fmBodyRef.value || fmBodyRef.value.offsetParent === null) return
  const target = event.target as HTMLElement | null
  if (target?.closest('input, textarea, select, [contenteditable="true"], .xterm, .el-dialog, .el-message-box')) return
  if (document.querySelector('.el-overlay:not([style*="display: none"]) .el-dialog')) return
  const mod = event.ctrlKey || event.metaKey
  const key = event.key.toLowerCase()
  const selected = getSelectedEntries()
  if (mod && key === 'c' && selected.length) { setClipboard('copy', selected) }
  else if (mod && key === 'x' && selected.length) { setClipboard('cut', selected) }
  else if (mod && key === 'v' && clipboard.value) { void paste() }
  else if (mod && key === 'a') { selectAll() }
  else if (event.key === 'Delete' && selected.length) { void deleteTargets(selected, event.shiftKey) }
  else if (event.key === 'F2' && selected.length === 1) { startRename(selected[0]) }
  else return
  event.preventDefault()
}

async function writeClipboardText(text: string): Promise<boolean> {
  if (!text) return false
  const clipboard = typeof navigator !== 'undefined' ? navigator.clipboard : undefined
  if (clipboard && typeof clipboard.writeText === 'function') {
    try {
      await clipboard.writeText(text)
      return true
    } catch {}
  }

  if (typeof document === 'undefined') return false
  let textarea: HTMLTextAreaElement | null = null
  try {
    textarea = document.createElement('textarea')
    textarea.value = text
    textarea.setAttribute('readonly', 'true')
    textarea.style.position = 'fixed'
    textarea.style.left = '-9999px'
    textarea.style.top = '-9999px'
    textarea.style.opacity = '0'
    document.body.appendChild(textarea)
    textarea.focus()
    textarea.select()
    return document.execCommand('copy')
  } catch {
    return false
  } finally {
    if (textarea?.parentNode) textarea.parentNode.removeChild(textarea)
  }
}

async function readClipboardTextSafe(): Promise<string | null> {
  const clipboard = typeof navigator !== 'undefined' ? navigator.clipboard : undefined
  if (!clipboard || typeof clipboard.readText !== 'function') return null
  try {
    return await clipboard.readText()
  } catch {
    return null
  }
}

async function copyPathTargets(targets: any[]) {
  const paths = targets.map((item) => String(item.path || '')).filter(Boolean)
  if (paths.length === 0) return
  const copied = await writeClipboardText(paths.join('\n'))
  if (copied) {
    ElMessage.success(paths.length > 1 ? t('fm.copiedN', { n: paths.length }) : t('fm.copied'))
  } else {
    ElMessage.error(t('fm.clipboardUnavailable'))
  }
}

async function downloadTargets(targets: any[]) {
  // Files download as they are; folders as a .tar.gz built while it downloads.
  const items = targets.filter((item) => item?.path && !item.is_virtual_drive)
  if (items.length === 0) return
  for (const [index, target] of items.entries()) {
    try {
      // Browsers drop rapid back-to-back downloads; space them out a little.
      if (index > 0) await new Promise((resolve) => setTimeout(resolve, 400))
      await downloadFileApi(String(target.path || ''), String(target.name || 'download'))
    } catch (error: any) {
      ElMessage.error(String(error?.message || t('api.requestFailed')))
      return
    }
  }
  ElMessage.success(t('fm.downloadStarted', { n: items.length }))
}

const chmodItems = ref<any[]>([])
function openChmodDialog(targets: any[]) {
  const items = targets.filter((item) => item?.path && !item.is_virtual_drive)
  if (items.length === 0) return
  chmodItems.value = items
  chmodTargets.value = items.map((item) => String(item.path))
  chmodPath.value = items[0].path
  showChmod.value = true
}

async function openPropertiesDialog(targets: any[]) {
  if (targets.length === 0) return
  if (targets.length === 1) {
    const target = targets[0]
    try {
      const res: any = await fileInfo(target.path)
      if (res.success) propsData.value = { ...res.data, is_batch: false }
      else propsData.value = { ...target, is_batch: false }
    } catch {
      propsData.value = { ...target, is_batch: false }
    }
    showProps.value = true
    return
  }

  const shouldLoadDetail = targets.length <= 40
  const details = shouldLoadDetail
    ? await Promise.all(targets.map(async (item) => {
      let baseItem: any = item
      try {
        const res: any = await fileInfo(item.path)
        if (res.success) baseItem = res.data
      } catch {}
      if (baseItem?.is_dir) {
        try {
          const sizeRes: any = await getDirectorySize(baseItem.path)
          if (sizeRes.success) {
            const calculated = Number(sizeRes.data?.size || 0)
            dirSizeCache.value = { ...dirSizeCache.value, [baseItem.path]: calculated }
            return { ...baseItem, size: calculated }
          }
        } catch {}
      }
      return baseItem
    }))
    : targets

  const fileCount = details.filter((item) => !item.is_dir).length
  const dirCount = details.filter((item) => item.is_dir).length
  const totalSize = details.reduce((sum, item) => sum + Number(item.size || 0), 0)
  propsData.value = {
    is_batch: true,
    count: details.length,
    file_count: fileCount,
    dir_count: dirCount,
    total_size: totalSize,
    paths: details.map((item) => item.path),
  }
  showProps.value = true
}

// --- Delete: to the recycle bin, or permanently ---
const deleteDialog = ref(false)
const deleteItems = ref<any[]>([])
const deletePermanent = ref(false)
const deleting = ref(false)
const recycleEnabled = ref(true)
const recycleOpen = ref(false)
const recycleMounted = ref(false)
/** Whether these items would skip the bin anyway (they are in it already). */
const deleteInBin = computed(() => deleteItems.value.every((f) => /\/\.Recycle_bin(\/|$)/.test(String(f.path))))
async function deleteTargets(targets: any[], permanent = false) {
  const items = targets.filter((f) => f?.path && !f.is_virtual_drive)
  if (items.length === 0) return
  deleteItems.value = items
  deletePermanent.value = permanent
  deleteDialog.value = true
  // The bin may have been switched off since this page loaded.
  const res: any = await getRecycleBin().catch(() => null)
  if (res?.success) recycleEnabled.value = !!res.data.settings.enabled
}
async function confirmDelete() {
  const items = deleteItems.value
  const permanent = deletePermanent.value || !recycleEnabled.value || deleteInBin.value
  deleting.value = true
  try {
    const res: any = await deletePaths(items.map((f) => String(f.path)), permanent)
    if (res?.success) {
      deleteDialog.value = false
      const n = res.data?.deleted ?? items.length
      ElMessage.success(res.data?.recycled ? t('fm.recycledN', { n }) : t('fm.deletedN', { n }))
      selectedSet.clear()
    }
  } catch {
    // Interceptor reports it.
  } finally {
    deleting.value = false
    await loadFiles()
  }
}
function openRecycleBin() {
  recycleMounted.value = true
  recycleOpen.value = true
}

async function batchDelete() {
  await deleteTargets(getSelectedEntries())
}

function onBodyMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  if (editingPath.value) {
    editingPath.value = false
    pathInputRef.value?.blur()
    return
  }
  const tgt = e.target as HTMLElement
  if (
    tgt.closest('.fm-row') ||
    tgt.closest('.fm-grid-item') ||
    tgt.closest('.fm-toolbar') ||
    tgt.closest('.fm-terminal') ||
    tgt.closest('.fm-term-resize') ||
    tgt.closest('.fm-list-header')
  ) return
  e.preventDefault()
  if (!e.ctrlKey) selectedSet.clear()
  preSelectSnapshot = new Set(selectedSet)
  dragStartX.value = e.clientX; dragStartY.value = e.clientY
  dragCurX.value = e.clientX; dragCurY.value = e.clientY
  dragSelecting.value = true
  document.addEventListener('mousemove', onDragMove)
  document.addEventListener('mouseup', onDragEnd)
}

function onRowMouseDown(_e: MouseEvent, _f: any) { /* handled by click */ }

function onDragMove(e: MouseEvent) {
  dragCurX.value = e.clientX; dragCurY.value = e.clientY
  updateDragSelection()
}
function onDragEnd() {
  dragSelecting.value = false
  document.removeEventListener('mousemove', onDragMove)
  document.removeEventListener('mouseup', onDragEnd)
}
function updateDragSelection() {
  const rx1 = Math.min(dragStartX.value, dragCurX.value)
  const ry1 = Math.min(dragStartY.value, dragCurY.value)
  const rx2 = Math.max(dragStartX.value, dragCurX.value)
  const ry2 = Math.max(dragStartY.value, dragCurY.value)
  selectedSet.clear()
  preSelectSnapshot.forEach(p => selectedSet.add(p))
  const container = scrollRef.value
  if (!container) return
  container.querySelectorAll('[data-path]').forEach(el => {
    const r = el.getBoundingClientRect()
    if (r.right >= rx1 && r.left <= rx2 && r.bottom >= ry1 && r.top <= ry2) {
      const path = el.getAttribute('data-path')
      if (path) selectedSet.add(path)
    }
  })
}

function startEditPath() {
  editingPath.value = true
  if (isVirtualDriveView.value) {
    pathInputVal.value = currentDriveRoot.value
  } else {
    const normalized = normPath(currentPath.value)
    pathInputVal.value = !useWindowsPathStyle.value && normalized === '/' ? '' : normalized
  }
  nextTick(() => { pathInputRef.value?.focus(); pathInputRef.value?.select() })
}
/**
 * A directory that does not exist yet: offer to create it (like typing a
 * new folder into an address bar). Returns whether it exists afterwards.
 */
async function offerCreateDir(dir: string): Promise<boolean> {
  try {
    const state: any = await pathState(dir)
    if (!state.success) return false
    if (state.data.exists) return true
  } catch {
    return false
  }
  try {
    await ElMessageBox.confirm(t('fm.createMissing', { path: dir }), t('fm.pathMissing'), {
      type: 'info',
      confirmButtonText: t('fm.createIt'),
    })
  } catch {
    return false
  }
  const res: any = await mkdirApi(dir).catch(() => null)
  if (res?.success) ElMessage.success(t('fm.dirCreated'))
  return !!res?.success
}

async function confirmPath() {
  editingPath.value = false
  const input = pathInputVal.value.trim()
  if (!input) return
  const np = normPath(input)
  const state: any = await pathState(np).catch(() => null)
  if (state?.success && !state.data.exists) {
    if (await offerCreateDir(np)) navigate(np)
    return
  }
  try {
    const res: any = await fileInfo(np)
    if (res.success) {
      if (res.data.is_dir) { navigate(np) }
      else {
        const fileName = fileNameOfPath(np)
        const parentDir = parentPath(np)
        if (parentDir !== currentPath.value) {
          currentPath.value = parentDir
          navHistory.value = navHistory.value.slice(0, navIndex.value + 1)
          navHistory.value.push(parentDir)
          navIndex.value = navHistory.value.length - 1
          loading.value = true
          try {
            const lr: any = await listFiles(parentDir)
            if (lr.success) {
              files.value = lr.data
              selectedSet.clear()
              const match = files.value.find((f: any) => f.name === fileName)
              if (match) selectedSet.add(match.path)
            }
          } finally { loading.value = false }
        } else {
          const match = files.value.find((f: any) => f.name === fileName)
          if (match) { selectedSet.clear(); selectedSet.add(match.path) }
        }
      }
    } else {
      const fileName = fileNameOfPath(np)
      const parentDir = parentPath(np)
      try {
        const pr: any = await fileInfo(parentDir)
        if (pr.success && pr.data.is_dir) {
          navigate(parentDir)
          nextTick(() => ElMessage.warning(t('fm.fileNotExist') + ': ' + fileName))
        } else { ElMessage.error(t('fm.pathNotExist')) }
      } catch { ElMessage.error(t('fm.pathNotExist')) }
    }
  } catch { ElMessage.error(t('fm.pathNotExist')) }
}

function handleOpen(f: any) {
  if (f.is_dir) { if (props.pickerMode) { emit('select', f.path); return }; navigate(f.path) }
  else if (isArchive(f.name) && !props.pickerMode) openExtract(f)
  else openEditor(f.path)
}

function resolveCtxMenuPos(x: number, y: number, menuEl: HTMLElement) {
  const width = menuEl.offsetWidth || 180
  const height = menuEl.offsetHeight || 160
  let left = x
  let top = y

  if (left + width + CTX_MENU_EDGE_GAP > window.innerWidth) {
    left = Math.max(CTX_MENU_EDGE_GAP, window.innerWidth - width - CTX_MENU_EDGE_GAP)
  }
  if (top + height + CTX_MENU_EDGE_GAP > window.innerHeight) {
    top = y - height
  }
  if (top < CTX_MENU_EDGE_GAP) top = CTX_MENU_EDGE_GAP

  return { left, top }
}

function adjustCtxMenuPos(kind: 'file' | 'bg' | 'term', x: number, y: number) {
  nextTick(() => {
    const menuEl =
      kind === 'file' ? ctxMenuRef.value : kind === 'bg' ? bgCtxMenuRef.value : termCtxMenuRef.value
    if (!menuEl) return
    const { left, top } = resolveCtxMenuPos(x, y, menuEl)
    if (kind === 'file') {
      ctxX.value = left
      ctxY.value = top
    } else if (kind === 'bg') {
      bgCtxX.value = left
      bgCtxY.value = top
    } else {
      termCtxX.value = left
      termCtxY.value = top
    }
  })
}

function onContext(e: MouseEvent, f: any) {
  if (f?.is_virtual_drive) return
  bgCtxVisible.value = false
  termCtxVisible.value = false
  ctxX.value = e.clientX; ctxY.value = e.clientY; ctxFile.value = f; ctxVisible.value = true
  adjustCtxMenuPos('file', e.clientX, e.clientY)
  if (!selectedSet.has(f.path)) { selectedSet.clear(); selectedSet.add(f.path) }
}
function ctxAction(action: string) {
  ctxVisible.value = false
  const f = ctxFile.value; if (!f) return
  const actionTargets = resolveActionTargets(f)
  if (action === 'open') handleOpen(f)
  else if (action === 'rename') startRename(f)
  else if (action === 'delete') { void deleteTargets(actionTargets) }
  else if (action === 'copy') { void copyPathTargets(actionTargets) }
  else if (action === 'download') { void downloadTargets(actionTargets) }
  else if (action === 'terminal') {
    const targetDir = f.is_dir ? f.path : currentPath.value
    openTerminal(targetDir)
  }
  else if (action === 'chmod') { openChmodDialog(actionTargets) }
  else if (action === 'props') { void openPropertiesDialog(actionTargets) }
  else if (action === 'clipCopy') setClipboard('copy', actionTargets)
  else if (action === 'clipCut') setClipboard('cut', actionTargets)
  else if (action === 'pasteInto') void paste(f.path)
  else if (action === 'extract') openExtract(f)
  else if (action === 'compress') openCompress(actionTargets)
}

function onBodyContext(e: MouseEvent) {
  const tgt = e.target as HTMLElement
  if (tgt.closest('.fm-row') || tgt.closest('.fm-grid-item') || tgt.closest('.fm-toolbar') || tgt.closest('.fm-terminal')) return
  ctxVisible.value = false
  termCtxVisible.value = false
  bgCtxX.value = e.clientX; bgCtxY.value = e.clientY; bgCtxVisible.value = true
  adjustCtxMenuPos('bg', e.clientX, e.clientY)
}
function bgCtxAction(action: string) {
  bgCtxVisible.value = false
  if (action === 'newFile') showNewFile.value = true
  else if (action === 'newDir') showMkdir.value = true
  else if (action === 'terminal') openTerminal(currentPath.value)
  else if (action === 'refresh') loadFiles()
  else if (action === 'copy') { void copyPathTargets(getSelectedEntries()) }
  else if (action === 'download') { void downloadTargets(getSelectedEntries()) }
  else if (action === 'chmod') { openChmodDialog(getSelectedEntries()) }
  else if (action === 'props') { void openPropertiesDialog(getSelectedEntries()) }
  else if (action === 'delete') { void deleteTargets(getSelectedEntries()) }
  else if (action === 'selectAll') selectAll()
  else if (action === 'paste') void paste()
  else if (action === 'clipCopy') setClipboard('copy', getSelectedEntries())
  else if (action === 'clipCut') setClipboard('cut', getSelectedEntries())
  else if (action === 'compress') openCompress(getSelectedEntries())
}

function getTerminalSelectionText() {
  if (!termInstance) return ''
  try {
    if (termInstance.hasSelection()) return termInstance.getSelection() || ''
  } catch {}
  return ''
}
function getBrowserSelectionText() {
  try {
    return window.getSelection?.()?.toString?.() || ''
  } catch {
    return ''
  }
}
async function copyTerminalSelection() {
  const text =
    getTerminalSelectionText() ||
    termSelectionSnapshot.value ||
    getBrowserSelectionText()
  if (!text) return
  const copied = await writeClipboardText(text)
  if (copied) {
    ElMessage.success(t('fm.copied'))
  } else {
    ElMessage.error(t('fm.clipboardUnavailable'))
  }
}
async function pasteTerminalClipboard() {
  if (!termSocket || termSocket.readyState !== WebSocket.OPEN) {
    ElMessage.warning(t('fm.termDisconnected'))
    return
  }
  const text = await readClipboardTextSafe()
  if (text === null) {
    ElMessage.error(t('fm.clipboardUnavailable'))
    return
  }
  if (!text) return
  termSocket.send(text)
  termInstance?.focus()
}
function clearTerminalScreen() {
  termInstance?.clear()
  termInstance?.focus()
}
function onTerminalContext(e: MouseEvent) {
  e.preventDefault()
  ctxVisible.value = false
  bgCtxVisible.value = false
  termCtxX.value = e.clientX
  termCtxY.value = e.clientY
  termSelectionSnapshot.value = getTerminalSelectionText() || getBrowserSelectionText()
  termCopyDisabled.value = !termSelectionSnapshot.value
  termCtxVisible.value = true
  adjustCtxMenuPos('term', e.clientX, e.clientY)
}
function termCtxAction(action: 'copy' | 'paste' | 'clear') {
  termCtxVisible.value = false
  if (action === 'copy') { void copyTerminalSelection() }
  else if (action === 'paste') { void pasteTerminalClipboard() }
  else if (action === 'clear') clearTerminalScreen()
  termSelectionSnapshot.value = ''
}
function closeAllCtx() { ctxVisible.value = false; bgCtxVisible.value = false; termCtxVisible.value = false }

const editorMounted = ref(false)
function openEditor(path: string) {
  editingFile.value = path
  editorMounted.value = true
  showEditor.value = true
}
async function doMkdir() {
  if (!newDirName.value.trim()) return
  const res: any = await mkdirApi(normPath(currentPath.value + '/' + newDirName.value.trim()))
  if (res.success) { ElMessage.success(t('fm.created')); showMkdir.value = false; newDirName.value = ''; loadFiles() }
  else ElMessage.error(res.message)
}
async function doNewFile() {
  if (!newFileName.value.trim()) return
  const res: any = await writeFileContent(normPath(currentPath.value + '/' + newFileName.value.trim()), '')
  if (res.success) { ElMessage.success(t('fm.created')); showNewFile.value = false; newFileName.value = ''; loadFiles() }
  else ElMessage.error(res.message)
}
function startRename(f: any) {
  const targets = resolveActionTargets(f)
  if (targets.length > 1) {
    ElMessage.warning(t('fm.renameMultiNotSupported'))
    return
  }
  const target = targets[0] || f
  renameFrom.value = target.path
  renameTo.value = target.name
  showRename.value = true
}
async function doRename() {
  if (!renameTo.value.trim()) return
  const res: any = await renamePathApi(renameFrom.value, normPath(currentPath.value + '/' + renameTo.value.trim()))
  if (res.success) { ElMessage.success(t('fm.renamed')); showRename.value = false; loadFiles() }
  else ElMessage.error(res.message)
}
async function handleDelete(f: any) {
  await deleteTargets([f])
}
function cssVar(name: string, fallback: string) {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return value || fallback
}
function cleanupTermReconnectTimer() {
  if (termReconnectTimer !== null) {
    window.clearTimeout(termReconnectTimer)
    termReconnectTimer = null
  }
}
function scheduleTerminalReconnect() {
  if (!termVisible.value || termDisableAutoReconnect || termReconnectTimer !== null) return
  termReconnectTimer = window.setTimeout(() => {
    termReconnectTimer = null
    if (termVisible.value) connectTerminal(true)
  }, 1200)
}
function applyTerminalTheme() {
  if (!termInstance) return
  const isDark = document.documentElement.getAttribute('data-theme') === 'dark'
  termInstance.options.theme = {
    background: cssVar('--lp-input-bg', '#0f1014'),
    foreground: cssVar('--lp-text-primary', '#d4d4d4'),
    cursor: cssVar('--lp-primary', '#4f6ef7'),
    cursorAccent: cssVar('--lp-card-bg', '#ffffff'),
    selectionBackground: cssVar('--lp-primary-bg', 'rgba(79,110,247,0.24)'),
    black: cssVar('--lp-text-secondary', '#7a7a7a'),
    red: cssVar('--lp-danger', isDark ? '#ff7875' : '#cf1322'),
    green: cssVar('--lp-success', isDark ? '#6abe39' : '#389e0d'),
    yellow: isDark ? '#ffc53d' : '#8a6200',
    blue: cssVar('--lp-primary', isDark ? '#5b7bf9' : '#2f54eb'),
    magenta: isDark ? '#d3adf7' : '#722ed1',
    cyan: isDark ? '#87e8de' : '#08979c',
    white: isDark ? '#d9d9d9' : '#262626',
    brightBlack: cssVar('--lp-text-secondary', isDark ? '#737373' : '#8c8c8c'),
    brightRed: isDark ? '#ffa39e' : '#ff4d4f',
    brightGreen: isDark ? '#95de64' : '#52c41a',
    brightYellow: isDark ? '#ffd666' : '#735100',
    brightBlue: isDark ? '#91aaff' : '#4f6ef7',
    brightMagenta: isDark ? '#efdbff' : '#9254de',
    brightCyan: isDark ? '#b5f5ec' : '#13c2c2',
    brightWhite: isDark ? '#ffffff' : '#434343',
  }
  termInstance.options.drawBoldTextInBrightColors = false
}
function sendTerminalResize() {
  if (!termInstance || !termSocket || termSocket.readyState !== WebSocket.OPEN) return
  const cols = Math.max(20, termInstance.cols)
  const rows = Math.max(8, termInstance.rows)
  termSocket.send(JSON.stringify({ type: 'resize', cols, rows }))
}
function sendTerminalChdir(path: string) {
  if (!termFollowPath.value) return
  if (path === WINDOWS_ROOTS_VIEW || isVirtualDriveView.value) return
  if (!termSocket || termSocket.readyState !== WebSocket.OPEN) return
  termSocket.send(JSON.stringify({ type: 'chdir', path }))
}
function applySyncedTerminalPath(path: string) {
  if (!termFollowPath.value) return
  const targetPath = normPath(path)
  if (!targetPath || targetPath === currentPath.value) return
  termApplyingSyncedPath = true
  navigate(targetPath)
}
function fitTerminal() {
  if (!termFitAddon || !termInstance) return
  try {
    termFitAddon.fit()
    termInstance.refresh(0, Math.max(0, termInstance.rows - 1))
    sendTerminalResize()
  } catch {}
}
function getTerminalCwdPath() {
  if (isVirtualDriveView.value) {
    if (isWindowsHost.value && windowsDriveRoots.value.length > 0) {
      return currentDriveRoot.value
    }
    return '/'
  }
  return currentPath.value
}
function buildTerminalWsUrl(cwd?: string) {
  const endpoint = new URL('/api/files/terminal/ws', window.location.origin)
  endpoint.protocol = endpoint.protocol === 'https:' ? 'wss:' : 'ws:'
  endpoint.searchParams.set('cwd', cwd || getTerminalCwdPath())
  const token = localStorage.getItem('token')
  if (token) endpoint.searchParams.set('token', token)
  return endpoint.toString()
}
function disconnectTerminal(manual = true) {
  termDisableAutoReconnect = manual
  cleanupTermReconnectTimer()
  if (termSocket) {
    try { termSocket.close() } catch {}
    termSocket = null
  }
  termConnected.value = false
}
function ensureTerminal() {
  if (termInstance || !termContainerRef.value) return
  termInstance = new Terminal({
    allowTransparency: false,
    convertEol: true,
    cursorBlink: true,
    fontFamily: 'Cascadia Mono, Cascadia Code, Consolas, "Courier New", monospace',
    fontSize: 12,
    lineHeight: 1.2,
    letterSpacing: 0,
    fontWeight: '400',
    fontWeightBold: '600',
    scrollback: 5000,
  })
  termFitAddon = new FitAddon()
  termInstance.loadAddon(termFitAddon)
  termInstance.open(termContainerRef.value)
  applyTerminalTheme()
  termInstance.focus()
  termInstance.onData((data) => {
    if (!termSocket || termSocket.readyState !== WebSocket.OPEN) return
    termSocket.send(data)
  })
  if (window.ResizeObserver) {
    termResizeObserver = new ResizeObserver(() => fitTerminal())
    termResizeObserver.observe(termContainerRef.value)
  }
  if (window.MutationObserver && !termThemeObserver) {
    termThemeObserver = new MutationObserver(() => applyTerminalTheme())
    termThemeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme', 'style'],
    })
  }
  nextTick(() => fitTerminal())
}
function connectTerminal(force = false, initialPath?: string) {
  if (!termVisible.value) return
  ensureTerminal()
  if (!termInstance) return
  if (!force && termSocket && (termSocket.readyState === WebSocket.OPEN || termSocket.readyState === WebSocket.CONNECTING)) {
    if (initialPath && termSocket.readyState === WebSocket.OPEN) {
      termSocket.send(JSON.stringify({ type: 'chdir', path: initialPath }))
    }
    termInstance.focus()
    return
  }
  disconnectTerminal(false)
  termDisableAutoReconnect = false
  const startupPath = initialPath || getTerminalCwdPath()
  const ws = new WebSocket(buildTerminalWsUrl(startupPath))
  ws.binaryType = 'arraybuffer'
  termSocket = ws
  ws.onopen = () => {
    if (termSocket !== ws) return
    termConnected.value = true
    cleanupTermReconnectTimer()
    applyTerminalTheme()
    fitTerminal()
    sendTerminalChdir(startupPath)
    termInstance?.focus()
  }
  ws.onmessage = async (event) => {
    if (!termInstance) return
    if (typeof event.data === 'string') {
      try {
        const payload = JSON.parse(event.data) as { type?: string; path?: string }
        if (payload?.type === 'cwd' && typeof payload.path === 'string') {
          applySyncedTerminalPath(payload.path)
          return
        }
      } catch {}
      termInstance.write(event.data)
      return
    }
    if (event.data instanceof ArrayBuffer) {
      termInstance.write(new Uint8Array(event.data))
      return
    }
    if (event.data instanceof Blob) {
      try {
        const buffer = await event.data.arrayBuffer()
        termInstance.write(new Uint8Array(buffer))
      } catch {}
    }
  }
  ws.onerror = () => {
    if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
      ws.close()
    }
  }
  ws.onclose = () => {
    if (termSocket === ws) termSocket = null
    termConnected.value = false
    if (!termDisableAutoReconnect) {
      scheduleTerminalReconnect()
    }
  }
}
function openTerminal(targetPath?: string) {
  const startupPath = targetPath ? normPath(targetPath) : undefined
  termVisible.value = true
  nextTick(() => connectTerminal(false, startupPath))
}
function closeTerminal() {
  termVisible.value = false
  disconnectTerminal(true)
}
function toggleTerminal() {
  if (termVisible.value) {
    closeTerminal()
    return
  }
  openTerminal()
}
function reconnectTerminal() {
  if (!termVisible.value) {
    openTerminal()
    return
  }
  connectTerminal(true)
}
function stopTermResize() {
  document.removeEventListener('mousemove', onTermResizeMove)
  document.removeEventListener('mouseup', stopTermResize)
  nextTick(() => fitTerminal())
}
function onTermResizeMove(e: MouseEvent) {
  const delta = termResizeStartY - e.clientY
  const maxHeight = Math.max(280, window.innerHeight - 180)
  termHeight.value = Math.min(maxHeight, Math.max(TERM_MIN_HEIGHT, termResizeStartHeight + delta))
  nextTick(() => fitTerminal())
}
function startTermResize(e: MouseEvent) {
  e.stopPropagation()
  termResizeStartY = e.clientY
  termResizeStartHeight = termHeight.value
  document.addEventListener('mousemove', onTermResizeMove)
  document.addEventListener('mouseup', stopTermResize)
}

function fileIconColor(name: string) {
  const ext = name.split('.').pop()?.toLowerCase() || ''
  const m: Record<string, string> = {
    js:'#f7df1e',ts:'#3178c6',vue:'#42b883',py:'#3776ab',rs:'#dea584',
    html:'#e34f26',css:'#1572b6',scss:'#cc6699',json:'#8bc34a',md:'#42a5f5',
    sh:'#4eaa25',yml:'#cb171e',yaml:'#cb171e',toml:'#9c4121',
    sql:'#e38c00',xml:'#f16529',svg:'#ffb13b',
  }
  return m[ext] || 'var(--lp-info)'
}
function fmtSize(b: number) {
  if (!b) return '0 B'
  const u = ['B','KB','MB','GB','TB']
  const i = Math.floor(Math.log(b) / Math.log(1024))
  return (b / Math.pow(1024, i)).toFixed(1) + ' ' + u[i]
}

// ?select=name highlights that entry once the folder has loaded (used by
// "locate in file manager" links elsewhere in the panel).
let pendingSelect = typeof route.query.select === 'string' ? route.query.select : ''
watch(() => route.query.select, (name) => {
  pendingSelect = typeof name === 'string' ? name : ''
  applyPendingSelect()
})
function applyPendingSelect() {
  if (!pendingSelect) return
  const entry = files.value.find((f: any) => f.name === pendingSelect)
  if (!entry) return
  pendingSelect = ''
  selectedSet.clear()
  selectedSet.add(entry.path)
  nextTick(() => {
    const el = scrollRef.value?.querySelector(`[data-path="${CSS.escape(entry.path)}"]`) as HTMLElement | null
    el?.scrollIntoView({ block: 'center' })
    el?.classList.add('fm-flash')
    setTimeout(() => el?.classList.remove('fm-flash'), 1600)
  })
}
watch(files, applyPendingSelect)

watch(
  () => route.query.path,
  async (newPath) => {
    if (props.initPath) return
    if (typeof newPath !== 'string' || !newPath.trim()) return
    const targetPath = normPath(newPath)
    if (targetPath === currentPath.value) return
    if (await offerCreateDir(targetPath)) navigate(targetPath)
  },
)
watch(
  currentPath,
  (nextPath, prevPath) => {
    if (nextPath === prevPath) return
    if (termApplyingSyncedPath) {
      termApplyingSyncedPath = false
      return
    }
    sendTerminalChdir(nextPath)
  },
)
watch(
  termFollowPath,
  (enabled) => {
    localStorage.setItem(TERM_FOLLOW_STORAGE_KEY, enabled ? '1' : '0')
    if (enabled) {
      sendTerminalChdir(currentPath.value)
    }
  },
)
watch(
  showChmod,
  (visible) => {
    if (!visible) chmodTargets.value = []
  },
)

// Links from other pages: /files?path=/dir&terminal=1 also opens the terminal there.
function openRequestedTerminal() {
  if (props.initPath || props.pickerMode) return
  if (route.query.terminal === '1' && routePath.value) openTerminal(routePath.value)
}
watch(() => route.fullPath, openRequestedTerminal)

onMounted(async () => {
  document.addEventListener('click', closeAllCtx)
  document.addEventListener('keydown', onFileKeys)
  await loadFileRoots()
  // Opened from a link to a directory that is not there yet.
  if (routePath.value && !props.initPath && currentPath.value === normPath(routePath.value)) {
    if (!(await offerCreateDir(currentPath.value))) {
      currentPath.value = normPath(savedPath() || rootPath.value)
      navHistory.value = [currentPath.value]
      navIndex.value = 0
    }
  }
  await loadFiles()
  openRequestedTerminal()
})
usePage(() => loadFiles())
onUnmounted(() => {
  document.removeEventListener('click', closeAllCtx)
  document.removeEventListener('keydown', onFileKeys)
  resetUploadDragState()
  stopTermResize()
  disconnectTerminal(true)
  cleanupTermReconnectTimer()
  if (termResizeObserver) {
    termResizeObserver.disconnect()
    termResizeObserver = null
  }
  if (termThemeObserver) {
    termThemeObserver.disconnect()
    termThemeObserver = null
  }
  if (termInstance) {
    termInstance.dispose()
    termInstance = null
  }
  termFitAddon = null
})
</script>

<style lang="scss">
/* Appended to <body> / teleported, so not scoped. */
.fm-drag-badge {
  position: fixed;
  top: -1000px;
  left: -1000px;
  max-width: 260px;
  padding: 6px 12px;
  border-radius: 8px;
  background: var(--lp-primary, #4f46e5);
  color: #fff;
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  box-shadow: 0 6px 18px rgba(15, 23, 42, 0.25);
}
.fm-undo-msg { display: inline-flex; align-items: center; gap: 12px; }
@keyframes fm-flash { 0%, 60% { box-shadow: inset 0 0 0 2px var(--lp-primary, #4f46e5); } 100% { box-shadow: none; } }
.fm-flash { animation: fm-flash 1.6s ease-out; }
.fm-del-list {
  margin: 0 0 12px; padding: 8px 10px; list-style: none; max-height: 220px; overflow: auto;
  border-radius: 8px; background: var(--lp-muted-soft, rgba(127,127,127,.08));
  li { display: flex; align-items: center; gap: 6px; padding: 3px 0; font-size: 13px; min-width: 0; }
}
.fm-del-more { color: var(--lp-text-secondary); font-size: 12px; }
.fm-del-note { margin: 6px 0 0; font-size: 12px; color: var(--lp-text-secondary); line-height: 1.6; }
.fm-del-note.is-danger { color: var(--el-color-danger); }
.fm-undo-btn {
  border: 0;
  padding: 0;
  background: transparent;
  color: var(--el-color-primary);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
  &:hover { text-decoration: underline; }
}
</style>

<style scoped lang="scss">
.file-manager {
  height: calc(100vh - var(--lp-header-h));
  height: calc(100dvh - var(--lp-header-h));
  padding: var(--lp-4) var(--lp-5);
  display: flex; flex-direction: column; user-select: none;
}
@media (max-width: 768px) { .file-manager { padding: var(--lp-2); } }
.fm-toolbar {
  display: flex; align-items: center; gap: 8px;
  padding: 8px 12px; background: var(--lp-card-bg);
  border-radius: var(--lp-radius-lg); margin-bottom: 8px;
  box-shadow: var(--lp-shadow-sm); flex-shrink: 0;
  border: 1px solid var(--lp-border-light);
  /* Toolbar buttons are quiet icons until hovered; disabled ones fade out. */
  :deep(.el-button) {
    background: transparent !important;
    border-color: transparent !important;
    color: var(--lp-text-soft) !important;
    &:hover, &:focus-visible { color: var(--lp-accent) !important; background: var(--lp-accent-soft) !important; }
    &.is-disabled,
    &.is-disabled:hover {
      opacity: 0.35 !important;
      color: var(--lp-text-soft) !important;
      background: transparent !important;
    }
  }
  :deep(.el-divider) { border-color: var(--lp-border-light); }
}
.fm-nav { flex-shrink: 0; }
.fm-nav-btns {
  display: flex; align-items: center; gap: 2px;
}
.fm-nav-btn {
  width: 30px; height: 30px; padding: 0 !important;
  border-radius: 9px !important;
  box-shadow: 0 1px 0 rgba(0,0,0,0.02);
}

.fm-path-bar {
  flex: 1; min-width: 0; display: flex; align-items: center; gap: 3px;
  padding: 4px 10px; background: var(--lp-input-bg);
  border: 1px solid var(--lp-border-light); border-radius: var(--lp-radius);
  cursor: text; font-size: 13px; height: 32px; overflow: hidden;
  transition: border-color var(--lp-transition);
  &:hover { border-color: var(--lp-border); }
}
.fm-path-icon { color: var(--lp-warning); flex-shrink: 0; margin-right: 4px; }
.fm-path-icon-clickable { cursor: pointer; }
.fm-path-root {
  color: var(--lp-text-secondary);
  cursor: pointer;
  padding: 2px 8px;
  border-radius: 8px;
  transition: color var(--lp-transition), background var(--lp-transition);
  &:hover { color: var(--lp-primary); background: var(--lp-primary-bg); }
}
.fm-path-arrow { color: var(--lp-text-placeholder); flex-shrink: 0; }
.fm-path-root.drop-target,
.fm-path-seg.drop-target { color: var(--lp-primary); background: var(--lp-primary-bg); box-shadow: inset 0 0 0 1px var(--lp-primary); }
.fm-path-seg {
  color: var(--lp-text-secondary); cursor: pointer; white-space: nowrap;
  max-width: 138px; overflow: hidden; text-overflow: ellipsis;
  padding: 1px 4px; border-radius: 6px;
  transition: color var(--lp-transition), background var(--lp-transition);
  &:hover { color: var(--lp-primary); background: var(--lp-primary-bg); }
  &:last-child { color: var(--lp-text-primary); font-weight: 500; }
}
.fm-path-input {
  flex: 1; border: none; outline: none; font-size: 13px;
  background: transparent; color: var(--lp-text-primary);
  font-family: 'Cascadia Code', 'Fira Code', monospace;
}
.fm-actions { display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
.fm-upload-input { display: none; }
.fm-uploads {
  position: absolute;
  right: 16px;
  bottom: 16px;
  z-index: 20;
  width: min(420px, calc(100% - 32px));
  background: var(--lp-surface, var(--el-bg-color));
  border: 1px solid var(--lp-border-light);
  border-radius: var(--lp-radius-lg, 10px);
  box-shadow: 0 10px 30px rgba(15, 23, 42, 0.16);
  overflow: hidden;
}
.fm-uploads__head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  cursor: pointer;
  font-size: 13px;
  font-weight: 600;
}
.fm-uploads__title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.fm-uploads__pct { color: var(--lp-text-secondary); font-weight: 500; }
.fm-uploads__chevron { transition: transform 0.15s; transform: rotate(180deg); }
.fm-uploads__chevron.open { transform: rotate(0deg); }
.fm-uploads__bar { margin: 0 12px 6px; }
.fm-uploads__list { max-height: 260px; overflow-y: auto; border-top: 1px solid var(--lp-border-light); }
.fm-upload { padding: 8px 12px; border-bottom: 1px solid var(--lp-border-light); }
.fm-upload:last-child { border-bottom: none; }
.fm-upload__line { display: flex; align-items: center; gap: 8px; font-size: 12px; }
.fm-upload__name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--lp-text-primary); }
.fm-upload__meta { color: var(--lp-text-secondary); white-space: nowrap; max-width: 55%; overflow: hidden; text-overflow: ellipsis; }
.fm-upload.is-done .fm-upload__meta { color: var(--el-color-success); }
.fm-upload.is-failed .fm-upload__meta { color: var(--el-color-danger); }
.fm-upload.is-canceled .fm-upload__name { text-decoration: line-through; color: var(--lp-text-secondary); }
.fm-upload.is-retrying .fm-upload__meta { color: var(--el-color-warning); }
.fm-upload--more { font-size: 12px; color: var(--lp-text-secondary); text-align: center; }
.fm-exec-check { margin-right: 6px; pointer-events: none; }
.fm-clip {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
  padding: 2px 6px 2px 10px;
  border-radius: 999px;
  background: var(--lp-primary-bg);
  color: var(--lp-primary);
  font-size: 12px;
  white-space: nowrap;
}
.fm-clip__close { cursor: pointer; opacity: 0.7; }
.fm-clip__close:hover { opacity: 1; }
.fm-ctx-key { margin-left: auto; padding-left: 16px; font-size: 11px; color: var(--lp-text-secondary); }
.fm-dialog-text { margin: 0 0 12px; font-size: 13px; color: var(--lp-text-secondary); word-break: break-all; }
.fm-dialog-hint { margin: 10px 0 0; font-size: 12px; color: var(--lp-text-secondary); word-break: break-all; }
.fm-radio-col { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; }
.fm-body { flex: 1; display: flex; flex-direction: column; min-height: 0; position: relative; }
.fm-list {
  flex: 1; background: var(--lp-card-bg); border-radius: var(--lp-radius-lg);
  box-shadow: var(--lp-shadow-sm); display: flex; flex-direction: column; min-height: 0;
  border: 1px solid var(--lp-border-light);
}
.fm-list-header {
  display: flex; padding: 10px 18px; font-size: 11px; font-weight: 600;
  color: var(--lp-text-secondary); text-transform: uppercase; letter-spacing: 0.5px;
  border-bottom: 1px solid var(--lp-border-light); flex-shrink: 0;
  background: var(--lp-table-header-bg);
  border-radius: var(--lp-radius-lg) var(--lp-radius-lg) 0 0;
  position: relative;
}
.fm-list-body { flex: 1; overflow-y: auto; }
.fm-col-check {
  /* A real column, always visible: aligned with the header, clear of the row edge. */
  flex: 0 0 32px;
  display: flex;
  align-items: center;
  justify-content: flex-start;
  margin-right: 4px;
  :deep(.el-checkbox) { margin-right: 0; height: 18px; }
  :deep(.el-checkbox__inner) { width: 16px; height: 16px; border-width: 1.5px; }
  :deep(.el-checkbox__inner::after) { left: 5px; top: 2px; }
}
.fm-col-name { flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; }
.fm-col-name-header,
.fm-col-header-sortable {
  cursor: pointer;
  user-select: none;
  transition: color var(--lp-transition);
  &:hover { color: var(--lp-primary); }
}
.fm-col-name-header {
  align-items: center;
  display: inline-flex;
}
.fm-sort-icon {
  margin-left: 4px;
  font-size: 12px;
}
.fm-col-size { width: 96px; text-align: right; flex-shrink: 0; }
.fm-col-time { width: 172px; text-align: right; flex-shrink: 0; }
.fm-col-perm { width: 118px; text-align: right; flex-shrink: 0; font-family: monospace; }
.fm-col-owner { width: 150px; text-align: right; flex-shrink: 0; font-family: monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding-left: 8px; }
.fm-row {
  display: flex; align-items: center; padding: 9px 18px;
  font-size: 14px; cursor: default; transition: background 0.1s;
  border-bottom: 1px solid var(--lp-border-light);
  position: relative;
  &:last-child { border-bottom: none; }
  &:hover { background: var(--lp-table-row-hover); }
  &.selected { background: var(--lp-primary-bg); }
  &.drop-target { background: var(--lp-primary-bg); box-shadow: inset 0 0 0 2px var(--lp-primary); }
  &.is-dragged { opacity: 0.5; }
  .fm-col-size, .fm-col-time, .fm-col-perm, .fm-col-owner { color: var(--lp-text-secondary); font-size: 12px; }
}
.fm-fname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--lp-text-primary); font-weight: 500; }
.fm-empty { padding: 40px 0; text-align: center; }
.fm-grid {
  flex: 1; background: var(--lp-card-bg); border-radius: var(--lp-radius-lg);
  box-shadow: var(--lp-shadow-sm); border: 1px solid var(--lp-border-light);
  padding: 16px; overflow-y: auto;
  display: grid; grid-template-columns: repeat(auto-fill, minmax(100px, 1fr));
  gap: 8px; align-content: start;
}
.fm-grid-item {
  display: flex; flex-direction: column; align-items: center; gap: 6px;
  padding: 12px 8px; border-radius: var(--lp-radius);
  cursor: default; transition: background 0.1s;
  position: relative;
  &:hover { background: var(--lp-table-row-hover); }
  &.selected { background: var(--lp-primary-bg); outline: 1px solid var(--lp-primary); }
  &.drop-target { background: var(--lp-primary-bg); outline: 2px solid var(--lp-primary); }
  &.is-dragged { opacity: 0.5; }
  .fm-grid-check :deep(.el-checkbox__inner) { width: 16px; height: 16px; border-width: 1.5px; }
}
.fm-grid-check {
  position: absolute;
  top: 8px;
  left: 8px;
  z-index: 2;
}
.fm-grid-name {
  font-size: 12px; color: var(--lp-text-primary); text-align: center;
  max-width: 90px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.fm-dir-size-btn {
  display: inline-flex;
  align-items: center;
  color: var(--lp-primary);
  cursor: pointer;
  text-decoration: none;
  border-bottom: 1px dashed transparent;
  transition: color var(--lp-transition), border-color var(--lp-transition), opacity var(--lp-transition);
  &:hover { border-color: currentColor; }
  &.loading {
    color: var(--lp-text-secondary);
    cursor: wait;
    border-bottom-color: transparent;
  }
}
.fm-drag-rect {
  position: fixed; border: 1px solid var(--lp-primary);
  background: rgba(79, 110, 247, 0.08); border-radius: 2px;
  pointer-events: none; z-index: 100;
}
.fm-upload-dropzone {
  position: absolute;
  inset: 10px;
  z-index: 120;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border-radius: var(--lp-radius-lg);
  border: 1px dashed var(--lp-primary);
  background: var(--lp-primary-bg);
  color: var(--lp-primary);
  font-size: 13px;
  font-weight: 600;
  pointer-events: none;
  backdrop-filter: blur(2px);
}
.fm-terminal {
  margin-top: 0; background: var(--lp-card-bg); border-radius: var(--lp-radius-lg);
  display: flex; flex-direction: column; flex-shrink: 0; min-height: 140px;
  box-shadow: var(--lp-shadow-sm); overflow: hidden; border: 1px solid var(--lp-border-light);
}
.fm-term-resize {
  height: 12px; cursor: ns-resize; flex-shrink: 0;
  display: flex; align-items: center; justify-content: center;
  width: 100%;
  align-self: stretch;
  margin-top: 8px;
  user-select: none;
  &::before {
    content: ''; width: 46px; height: 4px; border-radius: 99px; background: var(--lp-border);
    transition: background var(--lp-transition), transform var(--lp-transition);
  }
  &:hover::before { background: color-mix(in srgb, var(--lp-primary) 40%, var(--lp-border)); transform: scaleX(1.06); }
}
.fm-term-header {
  display: flex; align-items: center; gap: 10px;
  padding: 6px 12px; background: var(--lp-table-header-bg); flex-shrink: 0;
  border-bottom: 1px solid var(--lp-border-light);
}
.fm-term-title { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--lp-text-secondary); white-space: nowrap; }
.fm-term-cwd {
  flex: 1; min-width: 0; font-size: 12px;
  color: var(--lp-text-secondary); font-family: 'Cascadia Code', 'Fira Code', monospace;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.fm-term-status {
  font-size: 11px; line-height: 20px; height: 20px;
  padding: 0 8px; border-radius: 99px;
  border: 1px solid var(--lp-danger); color: var(--lp-danger);
  background: var(--lp-danger-bg);
  white-space: nowrap;
}
.fm-term-status.online {
  border-color: var(--lp-success); color: var(--lp-success); background: var(--lp-success-bg);
}
.fm-term-follow {
  display: inline-flex;
  align-items: center;
  :deep(.el-checkbox) { --el-checkbox-font-size: 12px; }
  :deep(.el-checkbox__label) { color: var(--lp-text-secondary); padding-left: 6px; }
}
.fm-term-actions { display: flex; align-items: center; gap: 8px; }
.fm-term-action,
.fm-term-close {
  cursor: pointer; color: var(--lp-text-secondary); border-radius: var(--lp-radius-sm);
  padding: 2px; transition: color var(--lp-transition), background var(--lp-transition);
  &:hover { color: var(--lp-primary); background: var(--lp-primary-bg); }
}
.fm-term-xterm {
  flex: 1; min-height: 0; padding: 8px 10px; background: var(--lp-input-bg);
}
.fm-term-xterm :deep(.xterm) {
  width: 100%;
  height: 100%;
  padding: 0;
}
.fm-term-xterm :deep(.xterm-viewport) { overflow-y: auto !important; }
.fm-term-xterm :deep(.xterm-cursor-layer) { z-index: 2; }
.fm-term-xterm :deep(.xterm-screen canvas) { image-rendering: auto; }
.fm-term-xterm :deep(.xterm .xterm-rows) {
  font-family: 'Cascadia Code', 'Fira Code', Consolas, monospace;
  letter-spacing: 0;
}
.fm-ctx-menu {
  position: fixed; z-index: 9999; min-width: 180px;
  background: var(--lp-popover-bg); border: 1px solid var(--lp-border);
  border-radius: var(--lp-radius); box-shadow: var(--lp-shadow-lg); padding: 4px 0;
  max-height: calc(100vh - 16px); overflow-y: auto; overflow-x: hidden;
}
.fm-ctx-item {
  display: flex; align-items: center; gap: 8px;
  padding: 7px 14px; font-size: 13px; color: var(--lp-text-primary);
  cursor: pointer; transition: background 0.1s;
  &:hover { background: var(--lp-primary-bg); color: var(--lp-primary); }
}
.fm-ctx-item.is-disabled {
  opacity: 0.48;
  cursor: not-allowed;
  pointer-events: none;
}
.fm-ctx-danger { &:hover { background: var(--lp-danger-bg) !important; color: var(--lp-danger) !important; } }
.fm-ctx-sep { height: 1px; background: var(--lp-border-light); margin: 4px 0; }
.fm-props-path-list {
  max-height: 180px;
  overflow: auto;
  border: 1px solid var(--lp-border-light);
  border-radius: var(--lp-radius-sm);
  background: var(--lp-input-bg);
}
.fm-props-path {
  font-size: 12px;
  line-height: 1.5;
  padding: 6px 8px;
  color: var(--lp-text-primary);
  border-bottom: 1px solid var(--lp-border-light);
  word-break: break-all;
  &:last-child { border-bottom: none; }
}
.fm-editor-wrap { height: 65vh; }
.fm-editor {
  width: 100%; height: 100%; resize: none;
  font-family: 'Cascadia Code', 'Fira Code', monospace;
  font-size: 13px; line-height: 1.6; padding: 12px;
  background: var(--lp-input-bg); color: var(--lp-text-primary);
  border: 1px solid var(--lp-input-border); border-radius: var(--lp-radius);
  outline: none; tab-size: 2;
  &:focus { border-color: var(--lp-primary); }
}
.fm-editor-info { float: left; font-size: 12px; color: var(--lp-text-secondary); line-height: 32px; }
</style>
