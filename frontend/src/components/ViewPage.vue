<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { usePageView } from '../composables/usePageView';
import { useUiSettings } from '../composables/useUiSettings';
import { buildLockTokenKey, ensureTabIdReady } from '../lib/lockToken';

const {
  pageId,
  pageTitle,
  pagePath,
  tocEntries,
  renderedHtml,
  assetItems,
  isLoading,
  isUploading,
  uploadProgress,
  uploadingFileName,
  uploadingIndex,
  uploadingTotal,
  assetUploadAllowed,
  assetUploadDisabled,
  copyToastVisible,
  copyToastName,
  pageMeta,
  pageMetaOpen,
  assetDetails,
  assetMetaDetails,
  assetDetailsLoading,
  assetDeleteTarget,
  assetDeleteLoading,
  assetInteractionDisabled,
  interactionDisabled,
  errorMessage,
  pageDeleteOpen,
  pageDeleteLoading,
  pageDeleteRecursive,
  pageMoveOpen,
  pageMoveLoading,
  pageMoveRecursive,
  pageMoveTarget,
  pageMoveResolvedTarget,
  pageMovePreviewPath,
  pageMoveInputError,
  pageMoveError,
  loadPage,
  uploadAssets,
  openPageMeta,
  dismissPageMeta,
  openPageDeleteConfirm,
  dismissPageDeleteConfirm,
  confirmPageDelete,
  openPageMoveConfirm,
  dismissPageMoveConfirm,
  confirmPageMove,
  requestCopyName,
  dismissCopyToast,
  openAssetDetails,
  dismissAssetDetails,
  openAssetDeleteConfirm,
  dismissAssetDeleteConfirm,
  confirmAssetDelete,
  requestEditLock,
  cleanupViewLock,
  reportError,
  dismissError,
} = usePageView();

const {
  themeOptions,
  fontOptions,
  editorKeymapOptions,
  selectedTheme,
  selectedFont,
  selectedFontSize,
  selectedCodeFontSize,
  selectedEditorKeymap,
  selectedEditorLineNumbers,
  markdownThemeClass,
  prismThemeClass,
  markdownStyle,
} = useUiSettings();

const settingsOpen = ref(false);
const sidePanelCollapsed = ref(false);
const isLocking = ref(false);
const tabIdReady = ref(false);
const cleanupLockDone = ref(false);
const isAssetDragging = ref(false);
const assetDragDepth = ref(0);
const assetInputRef = ref<HTMLInputElement | null>(null);
const isGlobalDragging = ref(false);
const globalDragDepth = ref(0);
const breadcrumbItems = computed(() => {
  const currentPath = pagePath.value || '/';
  if (currentPath === '/') {
    return [{ label: '/', href: '/wiki/' }];
  }

  const trimmed = currentPath.replace(/^\/+|\/+$/g, '');
  if (!trimmed) {
    return [{ label: '/', href: '/wiki/' }];
  }

  const segments = trimmed.split('/');
  const items = [{ label: '/', href: '/wiki/' }];
  let acc: string[] = [];
  for (const segment of segments) {
    acc.push(segment);
    const encoded = acc.map((part) => encodeURIComponent(part)).join('/');
    items.push({ label: segment, href: `/wiki/${encoded}` });
  }
  return items;
});
const editUrl = computed(() => buildEditUrl(pagePath.value));
const canDeletePage = computed(
  () => !interactionDisabled.value
    && !pageMeta.value?.page_info.deleted
    && (pagePath.value || '/') !== '/',
);
const canMovePage = computed(
  () => !interactionDisabled.value
    && !pageMeta.value?.page_info.deleted
    && (pagePath.value || '/') !== '/',
);

function applySidePanelCollapsed(value: boolean): void {
  localStorage.setItem('luwiki-side-collapsed', value ? '1' : '0');
}

function scrollToHashIfPresent(): void {
  const hash = window.location.hash;
  if (!hash || hash.length <= 1) {
    return;
  }
  const targetId = decodeURIComponent(hash.slice(1));
  const target = document.getElementById(targetId);
  if (!target) {
    return;
  }
  target.scrollIntoView({ block: 'start' });
}

function toggleSidePanel(): void {
  sidePanelCollapsed.value = !sidePanelCollapsed.value;
}

function openAssetPicker(): void {
  if (assetUploadDisabled.value) {
    return;
  }
  assetInputRef.value?.click();
}

function isFileDrag(event: DragEvent): boolean {
  const types = event.dataTransfer?.types;
  if (!types) {
    return false;
  }
  return Array.from(types).includes('Files');
}

function handleAssetInputChange(event: Event): void {
  if (assetUploadDisabled.value) {
    return;
  }
  const input = event.target as HTMLInputElement;
  const files = input.files ? Array.from(input.files) : [];
  if (files.length > 0) {
    void uploadAssets(files);
  }
  input.value = '';
}

function handleAssetDragEnter(event: DragEvent): void {
  if (assetUploadDisabled.value) {
    return;
  }
  if (!isFileDrag(event)) {
    return;
  }
  event.preventDefault();
  assetDragDepth.value += 1;
  isAssetDragging.value = true;
}

function handleAssetDragOver(event: DragEvent): void {
  if (assetUploadDisabled.value) {
    return;
  }
  if (!isFileDrag(event)) {
    return;
  }
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'copy';
  }
}

function handleAssetDragLeave(event: DragEvent): void {
  if (assetUploadDisabled.value) {
    return;
  }
  if (!isFileDrag(event)) {
    return;
  }
  event.preventDefault();
  assetDragDepth.value = Math.max(0, assetDragDepth.value - 1);
  if (assetDragDepth.value === 0) {
    isAssetDragging.value = false;
  }
}

function handleAssetDrop(event: DragEvent): void {
  if (assetUploadDisabled.value) {
    return;
  }
  if (!isFileDrag(event)) {
    return;
  }
  event.preventDefault();
  assetDragDepth.value = 0;
  isAssetDragging.value = false;
  const fileList = event.dataTransfer?.files;
  if (!fileList || fileList.length === 0) {
    return;
  }
  void uploadAssets(Array.from(fileList));
}

function buildAssetDownloadUrl(fileName: string): string {
  if (!pageId.value) {
    return '#';
  }
  const encoded = encodeURIComponent(fileName);
  return `/api/pages/${pageId.value}/assets/${encoded}`;
}

function handleWindowDragOver(event: DragEvent): void {
  if (!isFileDrag(event)) {
    return;
  }
  if (assetUploadDisabled.value) {
    return;
  }
  event.preventDefault();
  isGlobalDragging.value = true;
}

function handleWindowDrop(event: DragEvent): void {
  if (!isFileDrag(event)) {
    return;
  }
  event.preventDefault();
  globalDragDepth.value = 0;
  isGlobalDragging.value = false;
  if (assetUploadDisabled.value) {
    return;
  }
  const fileList = event.dataTransfer?.files;
  if (!fileList || fileList.length === 0) {
    return;
  }
  void uploadAssets(Array.from(fileList));
}

function handleWindowDragEnter(event: DragEvent): void {
  if (!isFileDrag(event)) {
    return;
  }
  if (assetUploadDisabled.value) {
    return;
  }
  event.preventDefault();
  globalDragDepth.value += 1;
  isGlobalDragging.value = true;
}

function handleWindowDragLeave(event: DragEvent): void {
  if (!isFileDrag(event)) {
    return;
  }
  if (assetUploadDisabled.value) {
    return;
  }
  event.preventDefault();
  globalDragDepth.value = Math.max(0, globalDragDepth.value - 1);
  if (globalDragDepth.value === 0) {
    isGlobalDragging.value = false;
  }
}

function buildEditUrl(currentPath: string): string {
  if (!currentPath || currentPath === '/') {
    return '/edit/';
  }
  const trimmed = currentPath.replace(/^\/+|\/+$/g, '');
  if (!trimmed) {
    return '/edit/';
  }
  const encoded = trimmed
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/');
  return `/edit/${encoded}`;
}

async function handleEditClick(): Promise<void> {
  if (interactionDisabled.value || isLocking.value || !tabIdReady.value) {
    return;
  }

  isLocking.value = true;
  try {
    const token = await requestEditLock();
    const key = buildLockTokenKey(pageId.value);
    sessionStorage.setItem(key, token);
    window.location.href = editUrl.value;
  } catch (err: unknown) {
    reportError(err);
  } finally {
    isLocking.value = false;
  }
}

async function initTabId(): Promise<void> {
  try {
    await ensureTabIdReady();
    tabIdReady.value = true;
  } catch (err: unknown) {
    reportError(err);
  }
}

onMounted(() => {
  const savedCollapsed = localStorage.getItem('luwiki-side-collapsed');
  if (savedCollapsed === '1') {
    sidePanelCollapsed.value = true;
  }
  void initTabId();
  void loadPage();

  window.addEventListener('dragover', handleWindowDragOver);
  window.addEventListener('drop', handleWindowDrop);
  window.addEventListener('dragenter', handleWindowDragEnter);
  window.addEventListener('dragleave', handleWindowDragLeave);
  document.addEventListener('dragover', handleWindowDragOver, true);
  document.addEventListener('drop', handleWindowDrop, true);
  document.addEventListener('dragenter', handleWindowDragEnter, true);
  document.addEventListener('dragleave', handleWindowDragLeave, true);
});

watch([pageId, tabIdReady], async ([nextPageId, nextTabReady]) => {
  if (cleanupLockDone.value) {
    return;
  }
  if (!nextPageId || !nextTabReady) {
    return;
  }
  cleanupLockDone.value = true;
  await cleanupViewLock();
});

onBeforeUnmount(() => {
  window.removeEventListener('dragover', handleWindowDragOver);
  window.removeEventListener('drop', handleWindowDrop);
  window.removeEventListener('dragenter', handleWindowDragEnter);
  window.removeEventListener('dragleave', handleWindowDragLeave);
  document.removeEventListener('dragover', handleWindowDragOver, true);
  document.removeEventListener('drop', handleWindowDrop, true);
  document.removeEventListener('dragenter', handleWindowDragEnter, true);
  document.removeEventListener('dragleave', handleWindowDragLeave, true);
});

watch(sidePanelCollapsed, (value) => {
  applySidePanelCollapsed(value);
});

watch(renderedHtml, async (value) => {
  if (!value) {
    return;
  }
  await nextTick();
  scrollToHashIfPresent();
});
</script>

<template>
  <div class="min-h-screen bg-base-200 text-base-content" :data-theme="selectedTheme">
    <div
      v-if="isGlobalDragging && assetUploadAllowed"
      class="fixed inset-0 z-50 flex items-center justify-center bg-base-300/80"
    >
      <div class="border-2 border-dashed border-info/70 bg-base-100/95 px-8 py-6">
        <p class="text-sm font-semibold text-info">
          ここにドロップしてアセットを追加
        </p>
      </div>
    </div>
    <div class="mx-auto flex max-w-6xl flex-col gap-1 px-4 py-8 lg:px-10">
      <header class="flex flex-col gap-1">
        <div>
          <p class="text-xs font-semibold uppercase tracking-[0.32em] text-base-content/60">
            LUWIKI VIEW
          </p>
          <h1
            class="text-3xl font-bold leading-tight empty:min-h-[2.5rem] sm:text-4xl mt-3 mb-2 truncate"
            :title="pageTitle"
          >
            {{ pageTitle }}
          </h1>
          <nav
            class="flex flex-nowrap items-center gap-1 text-sm text-info mx-4 mt-3"
            aria-label="breadcrumb"
          >
            <template v-for="(item, index) in breadcrumbItems" :key="item.href">
              <a class="link link-hover inline-flex items-center" :href="item.href">
                <span class="inline-block max-w-full truncate">{{ item.label }}</span>
              </a>
              <span
                v-if="index < breadcrumbItems.length - 1"
                class="inline-flex h-4 items-center text-base-content/50 leading-none"
                aria-hidden="true"
              >
                ⏵
              </span>
            </template>
          </nav>
        </div>

        <nav
          class="flex flex-wrap items-center gap-1"
          :class="{ 'pointer-events-none opacity-50': interactionDisabled }"
        >
          <button
            class="btn btn-link btn-sm pl-1 text-info"
            type="button"
            :disabled="interactionDisabled || isLocking || !tabIdReady"
            @click="handleEditClick"
          >
            編集
          </button>
          <button
            class="btn btn-link btn-sm text-info"
            type="button"
            :disabled="assetUploadDisabled"
            @click="openAssetPicker"
          >
            アセット追加
          </button>
          <button
            class="btn btn-link btn-sm text-info"
            type="button"
            :disabled="!canMovePage"
            @click="openPageMoveConfirm"
          >
            移動
          </button>
          <button
            class="btn btn-link btn-sm text-info"
            type="button"
            :disabled="!canDeletePage"
            @click="openPageDeleteConfirm"
          >
            削除
          </button>
          <button class="btn btn-link btn-sm text-info hidden md:block" type="button" @click="openPageMeta">
            情報表示
          </button>
          <a class="btn btn-link btn-sm text-neutral-content hidden md:inline-flex" href="#">履歴</a>
          <a class="btn btn-link btn-sm text-neutral-content hidden md:inline-flex" href="#">差分</a>

          <a
            class="btn btn-link btn-sm text-info text-neutral-content ml-auto"
            href="#"
            :class="{ 'pointer-events-none opacity-50': interactionDisabled || !tabIdReady }"
            :aria-disabled="interactionDisabled || !tabIdReady"
          >
            新規作成
          </a>
          <a class="btn btn-link btn-sm text-info" href="/search">検索</a>
          <button
            class="btn btn-link btn-sm pr-1 text-info"
            type="button"
            @click="settingsOpen = true"
          >
            設定
          </button>
        </nav>
        <input
          ref="assetInputRef"
          type="file"
          class="hidden"
          multiple
          @change="handleAssetInputChange"
        />
      </header>

      <div class="hidden items-center justify-start lg:flex">
        <button class="btn btn-ghost btn-xs" type="button" @click="toggleSidePanel">
          {{ sidePanelCollapsed ? 'サイドパネルを開く' : 'サイドパネルを閉じる' }}
        </button>
      </div>

      <main
        class="grid min-h-[calc(100vh-11.2em)] lg:min-h-[calc(100vh-12.8em)] items-stretch gap-1"
        :class="
          sidePanelCollapsed
            ? 'lg:grid-cols-[minmax(0,1fr)]'
            : 'lg:grid-cols-[220px_minmax(0,1fr)]'
        "
      >
        <aside
          v-if="!sidePanelCollapsed"
          class="order-2 hidden flex-col gap-1 lg:flex lg:order-1"
        >
          <section
            class="h-full border border-base-300 bg-base-100 p-2 shadow-sm"
            :class="{ 'pointer-events-none opacity-50': interactionDisabled }"
          >
            <h2 class="mb-2 text-lg font-semibold">TOC</h2>
            <ul class="flex flex-col gap-1 text-sm">
              <li
                v-for="entry in tocEntries"
                :key="entry.anchor"
                :class="{
                  'ml-3': entry.level === 3,
                  'ml-6': entry.level === 4,
                }"
              >
                <a
                  class="link link-hover block truncate"
                  :title="entry.text"
                  :href="`#${entry.anchor}`"
                >
                  {{ entry.text }}
                </a>
              </li>
            </ul>
          </section>
        </aside>

        <div
          class="order-1 flex h-full min-h-full flex-col gap-1 lg:order-2"
          :class="{ 'pointer-events-none opacity-50': interactionDisabled }"
        >
          <section class="flex min-h-0 flex-1 border border-base-300 bg-transparent shadow-sm">
            <article
              class="markdown-body h-full flex-1 p-4"
              :class="[markdownThemeClass, prismThemeClass]"
              :style="markdownStyle"
              v-html="renderedHtml"
            />
          </section>
        </div>
      </main>

      <footer>
        <div
          class="grid items-stretch gap-1"
          :class="
            sidePanelCollapsed
              ? 'lg:grid-cols-[minmax(0,1fr)]'
              : 'lg:grid-cols-[220px_minmax(0,1fr)]'
          "
        >
          <section
            class="border p-4 shadow-sm transition-colors"
            :class="[
              sidePanelCollapsed ? 'lg:col-start-1' : 'lg:col-start-2',
              assetInteractionDisabled ? 'pointer-events-none opacity-50' : '',
              isAssetDragging
                ? 'border-info/70 bg-info/10'
                : 'border-base-300 bg-base-100',
            ]"
            @dragenter.prevent="handleAssetDragEnter"
            @dragover.prevent="handleAssetDragOver"
            @dragleave.prevent="handleAssetDragLeave"
            @drop.prevent="handleAssetDrop"
          >
            <div class="mb-3 flex items-center justify-between">
              <div class="flex flex-col gap-2">
                <div class="flex items-center gap-2">
                  <h2 class="text-lg font-semibold">アセット</h2>
                  <span v-if="isUploading" class="badge badge-outline badge-sm">
                    アップロード中
                  </span>
                </div>
                <div v-if="!assetUploadAllowed" class="text-xs text-error">
                  アップロード不可（最大サイズ未設定）
                </div>
                <div v-if="isUploading" class="text-xs text-base-content/70">
                  {{ uploadingIndex }}/{{ uploadingTotal }}
                  <span v-if="uploadingFileName"> - {{ uploadingFileName }}</span>
                  <span v-if="uploadProgress !== null"> ({{ uploadProgress }}%)</span>
                </div>
              </div>
              <span class="badge badge-warning badge-outline">
                {{ assetItems.length }}
              </span>
            </div>
            <div v-if="isUploading" class="mb-3">
              <progress
                class="progress progress-info w-full"
                :value="uploadProgress ?? 0"
                max="100"
              />
            </div>
            <div class="flex flex-wrap gap-2">
              <div
                v-for="asset in assetItems"
                :key="asset.file_name"
                class="border border-base-300 bg-base-200/70 p-2"
              >
                <div class="max-w-[16em] text-sm font-semibold truncate" :title="asset.file_name">
                  {{ asset.file_name }}
                </div>
                <div class="text-xs text-base-content/70">
                  {{ asset.formattedSize }}
                </div>
                <div class="flex flex-wrap gap-2 text-xs">
                  <button
                    class="link link-hover"
                    type="button"
                    @click="openAssetDetails(asset)"
                  >
                    詳細
                  </button>
                  <a class="link link-hover" :href="buildAssetDownloadUrl(asset.file_name)">
                    DL
                  </a>
                  <button
                    class="link link-hover"
                    type="button"
                    @click="requestCopyName(asset.file_name)"
                  >
                    コピー
                  </button>
                  <button
                    class="link link-hover text-error"
                    type="button"
                    @click="openAssetDeleteConfirm(asset)"
                  >
                    削除
                  </button>
                </div>
              </div>
            </div>
          </section>
        </div>
      </footer>
    </div>

    <div v-if="settingsOpen" class="modal modal-open">
      <div class="modal-box space-y-4 transform-none">
        <h3 class="text-lg font-bold">表示設定</h3>
        <div class="space-y-3">
          <label class="form-control w-full">
            <div class="label">
              <span class="label-text">画面テーマ</span>
            </div>
            <select v-model="selectedTheme" class="select select-bordered">
              <option v-for="theme in themeOptions" :key="theme" :value="theme">
                {{ theme }}
              </option>
            </select>
          </label>

          <label class="form-control w-full">
            <div class="label">
              <span class="label-text">Markdownフォント</span>
            </div>
            <select v-model="selectedFont" class="select select-bordered">
              <option v-for="font in fontOptions" :key="font.value" :value="font.value">
                {{ font.label }}
              </option>
            </select>
          </label>

          <label class="form-control w-full">
            <div class="label">
              <span class="label-text">エディタキーバインド</span>
            </div>
            <select v-model="selectedEditorKeymap" class="select select-bordered">
              <option v-for="option in editorKeymapOptions" :key="option.value" :value="option.value">
                {{ option.label }}
              </option>
            </select>
          </label>

          <label class="form-control w-full">
            <div class="label">
              <span class="label-text">行番号表示</span>
            </div>
            <label class="label cursor-pointer justify-start gap-3">
              <input
                v-model="selectedEditorLineNumbers"
                class="toggle toggle-sm"
                type="checkbox"
              />
              <span class="label-text text-sm">表示する</span>
            </label>
          </label>

          <label class="form-control w-full">
            <div class="label">
              <span class="label-text">Markdownフォントサイズ</span>
              <span class="label-text-alt">{{ selectedFontSize }}px</span>
            </div>
            <input
              v-model.number="selectedFontSize"
              type="range"
              min="12"
              max="22"
              step="1"
              class="range range-sm"
            />
          </label>

          <label class="form-control w-full">
            <div class="label">
              <span class="label-text">コードブロック文字サイズ</span>
              <span class="label-text-alt">{{ selectedCodeFontSize }}px</span>
            </div>
            <input
              v-model.number="selectedCodeFontSize"
              type="range"
              min="12"
              max="22"
              step="1"
              class="range range-sm"
            />
          </label>
        </div>
        <div class="modal-action">
          <button class="btn" type="button" @click="settingsOpen = false">
            閉じる
          </button>
        </div>
      </div>
    </div>

    <div v-if="errorMessage" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">読み込みエラー</h3>
        <p class="text-sm text-base-content/70">{{ errorMessage }}</p>
        <div class="modal-action">
          <button class="btn btn-primary" type="button" @click="dismissError">
            閉じる
          </button>
        </div>
      </div>
    </div>

    <div v-if="assetDetails" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">アセット詳細</h3>
        <div v-if="assetDetailsLoading" class="text-sm text-base-content/70">
          読み込み中...
        </div>
        <div v-else class="space-y-2 text-sm">
          <div>
            <span class="text-base-content/60">ファイル名</span>
            <div class="font-semibold">{{ assetDetails.file_name }}</div>
          </div>
          <div>
            <span class="text-base-content/60">MIME種別</span>
            <div class="font-semibold">
              {{ assetMetaDetails?.mime_type ?? assetDetails.mime_type }}
            </div>
          </div>
          <div>
            <span class="text-base-content/60">サイズ</span>
            <div class="font-semibold">
              {{ assetMetaDetails?.size ?? assetDetails.size }}
            </div>
          </div>
          <div>
            <span class="text-base-content/60">登録日時</span>
            <div class="font-semibold">
              {{ assetMetaDetails?.timestamp ?? assetDetails.timestamp }}
            </div>
          </div>
          <div>
            <span class="text-base-content/60">登録ユーザ</span>
            <div class="font-semibold">
              {{ assetMetaDetails?.username ?? assetDetails.username }}
            </div>
          </div>
        </div>
        <div class="modal-action">
          <button class="btn" type="button" @click="dismissAssetDetails">
            閉じる
          </button>
        </div>
      </div>
    </div>

    <div v-if="assetDeleteTarget" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">アセット削除</h3>
        <p class="text-sm text-base-content/70">
          "{{ assetDeleteTarget.file_name }}" を削除しますか？
        </p>
        <div class="modal-action">
          <button class="btn" type="button" @click="dismissAssetDeleteConfirm">
            キャンセル
          </button>
          <button
            class="btn btn-error"
            type="button"
            :disabled="assetDeleteLoading"
            @click="confirmAssetDelete"
          >
            削除
          </button>
        </div>
      </div>
    </div>

    <div v-if="pageDeleteOpen" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">ページ削除</h3>
        <div class="flex justify-between">
          <p class="text-sm text-base-content/70">
            "{{ pageTitle }}" を削除しますか？
          </p>
          <label class="flex items-center gap-2 text-sm text-base-content/70">
            <input
              v-model="pageDeleteRecursive"
              class="checkbox checkbox-ghost checkbox-xs"
              type="checkbox"
            />
            <span>子ページも削除（再帰）</span>
          </label>
        </div>
        <div class="modal-action">
          <button class="btn" type="button" @click="dismissPageDeleteConfirm">
            キャンセル
          </button>
          <button
            class="btn btn-error"
            type="button"
            :disabled="pageDeleteLoading"
            @click="confirmPageDelete"
          >
            削除
          </button>
        </div>
      </div>
    </div>

    <div v-if="pageMoveOpen" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">ページ移動(リネーム)</h3>
        <div class="space-y-3 text-sm">
          <p class="text-base-content/70">
            現在のパス: "{{ pagePath || '/' }}"
          </p>
          <label class="form-control w-full">
            <div class="label">
              <span class="label-text">移動先パス</span>
            </div>
            <input
              v-model="pageMoveTarget"
              class="input input-bordered w-full"
              type="text"
              placeholder="/new/path"
            />
          </label>

          <div class="rounded border border-base-300 bg-base-200/60 p-2 text-xs">
            <div class="text-base-content/60">移動プレビュー</div>
            <div class="p-2">
              <div class="break-all font-mono">{{ pagePath || '/' }}</div>
              <div class="text-base-content/60 ml-4 my-2">&#x2b07;</div>
              <div class="break-all font-mono">
                {{ pageMovePreviewPath || '(未入力)' }}
              </div>
            </div>
          </div>

          <p v-if="pageMoveError" class="text-sm text-error">
            {{ pageMoveError }}
          </p>
          <p v-else-if="pageMoveInputError" class="text-sm text-error">
            {{ pageMoveInputError }}
          </p>
          <p v-else class="text-sm min-h-[1.25rem]">
          </p>

          <label class="flex items-center gap-2 text-sm text-base-content/70">
            <input
              v-model="pageMoveRecursive"
              class="checkbox checkbox-ghost checkbox-xs ml-auto"
              type="checkbox"
            />
            <span>子ページも移動（再帰）</span>
          </label>
        </div>
        <div class="modal-action">
          <button class="btn" type="button" @click="dismissPageMoveConfirm">
            キャンセル
          </button>
          <button
            class="btn btn-primary"
            type="button"
            :disabled="pageMoveLoading || !!pageMoveInputError"
            @click="confirmPageMove"
          >
            移動
          </button>
        </div>
      </div>
    </div>

    <div v-if="pageMetaOpen && pageMeta" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">ページ情報</h3>
        <div class="space-y-2 text-sm">
          <div>
            <span class="text-base-content/60">ページID</span>
            <div class="font-semibold">{{ pageId }}</div>
          </div>
          <div>
            <span class="text-base-content/60">パス</span>
            <div class="font-semibold">{{ pageMeta.page_info.path.value }}</div>
          </div>
          <div>
            <span class="text-base-content/60">リビジョン</span>
              <div class="font-semibold">{{ pageMeta.revision_info?.revision ?? '-' }}</div>
          </div>
          <div>
            <span class="text-base-content/60">最新/最古</span>
            <div class="font-semibold">
              {{ pageMeta.page_info.revision_scope.latest }}
              /
              {{ pageMeta.page_info.revision_scope.oldest }}
            </div>
          </div>
          <div>
            <span class="text-base-content/60">更新者</span>
              <div class="font-semibold">{{ pageMeta.revision_info?.username ?? '-' }}</div>
          </div>
          <div>
            <span class="text-base-content/60">更新日時</span>
              <div class="font-semibold">{{ pageMeta.revision_info?.timestamp ?? '-' }}</div>
          </div>
          <div>
            <span class="text-base-content/60">リネーム履歴</span>
            <div class="font-semibold">
              {{
                pageMeta.page_info.rename_revisions.length > 0
                  ? pageMeta.page_info.rename_revisions.join(', ')
                  : 'なし'
              }}
            </div>
          </div>
          <div>
            <span class="text-base-content/60">削除済み</span>
            <div class="font-semibold">{{ pageMeta.page_info.deleted ? 'はい' : 'いいえ' }}</div>
          </div>
          <div>
            <span class="text-base-content/60">ロック中</span>
            <div class="font-semibold">{{ pageMeta.page_info.locked ? 'はい' : 'いいえ' }}</div>
          </div>
          <div v-if="pageMeta.revision_info?.rename_info">
            <span class="text-base-content/60">リネーム</span>
            <div class="font-semibold">
              {{ pageMeta.revision_info?.rename_info?.from ?? '-' }}
              →
              {{ pageMeta.revision_info?.rename_info?.to }}
            </div>
          </div>
        </div>
        <div class="modal-action">
          <button class="btn" type="button" @click="dismissPageMeta">
            閉じる
          </button>
        </div>
      </div>
    </div>

    <div v-if="copyToastVisible" class="toast toast-end toast-bottom z-50">
      <div class="alert border border-base-300 bg-base-100 text-base-content shadow-lg">
        <div class="flex flex-col gap-2">
          <span class="text-sm font-semibold">アップロード完了</span>
          <span class="text-xs text-base-content/70">{{ copyToastName }}</span>
          <div class="flex gap-2">
            <button class="btn btn-xs" type="button" @click="requestCopyName(copyToastName)">
              コピー
            </button>
            <button class="btn btn-ghost btn-xs" type="button" @click="dismissCopyToast">
              閉じる
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
