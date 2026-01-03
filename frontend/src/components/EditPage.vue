<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { usePageEdit } from '../composables/usePageEdit';
import { useUiSettings } from '../composables/useUiSettings';
import {
  createMarkdownRenderer,
  extractTitle,
  extractToc,
  normalizeWikiPath,
} from '../lib/pageCommon';

const {
  pageId,
  pagePath,
  source: sourceText,
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
  newPageToastVisible,
  newPageToastMessage,
  restoreCandidates,
  restoreCandidateId,
  restoreCandidateSource,
  restoreCandidateLoading,
  restorePromptVisible,
  restoreInProgress,
  assetDetails,
  assetMetaDetails,
  assetDetailsLoading,
  assetDeleteTarget,
  assetDeleteLoading,
  assetInteractionDisabled,
  interactionDisabled,
  errorMessage,
  isSaving,
  canSave,
  isNewPage,
  isDraftPage,
  amendChecked,
  loadPage,
  savePage,
  cancelEdit,
  setAmendChecked,
  uploadAssets,
  selectRestoreCandidate,
  confirmRestoreCandidate,
  skipRestoreCandidates,
  requestCopyName,
  dismissCopyToast,
  dismissNewPageToast,
  openAssetDetails,
  dismissAssetDetails,
  openAssetDeleteConfirm,
  dismissAssetDeleteConfirm,
  confirmAssetDelete,
  dismissError,
} = usePageEdit();

const sidePanelCollapsed = ref(false);
const sourceStatus = ref('未読込');
const autoLoadSource = true;

const renderedHtml = ref('');
const markdownRenderer = ref<ReturnType<typeof createMarkdownRenderer> | null>(null);
const markdownRendererPath = ref('');

const sourceByteLength = computed(() => new TextEncoder().encode(sourceText.value).length);
const editorTitle = ref('');
const tocItems = ref<{ level: number; text: string; anchor: string }[]>([]);
let derivedTimer: number | null = null;
const previewMode = ref(false);

const {
  selectedTheme,
  markdownThemeClass,
  prismThemeClass,
  markdownStyle,
} = useUiSettings();

const breadcrumbItems = computed(() => {
  const rawPath = window.location.pathname.replace(/^\/edit\/?/, '');
  if (!rawPath) {
    return [{ label: '/' }];
  }
  const decoded = rawPath
    .split('/')
    .map((segment) => {
      try {
        return decodeURIComponent(segment);
      } catch {
        return segment;
      }
    })
    .filter((segment) => segment.length > 0);
  return [{ label: '/' }, ...decoded.map((label) => ({ label }))];
});

function resolveEditPath(): string {
  const raw = window.location.pathname.replace(/^\/edit\/?/, '');
  if (!raw) {
    return '/';
  }
  const decoded = raw
    .split('/')
    .map((segment) => {
      try {
        return decodeURIComponent(segment);
      } catch {
        return segment;
      }
    })
    .filter((segment) => segment.length > 0)
    .join('/');
  return normalizeWikiPath(decoded);
}

const isLargeScreen = ref(false);

function updateScreenState(): void {
  isLargeScreen.value = window.innerWidth >= 1024;
}

const showEditorPanel = computed(() => isLargeScreen.value || !previewMode.value);
const showPreviewPanel = computed(() => isLargeScreen.value || previewMode.value);

function ensureMarkdownRenderer(): ReturnType<typeof createMarkdownRenderer> {
  const path = pagePath.value || resolveEditPath();
  if (!markdownRenderer.value || markdownRendererPath.value !== path) {
    markdownRenderer.value = createMarkdownRenderer(path, '/wiki');
    markdownRendererPath.value = path;
  }
  return markdownRenderer.value;
}

onMounted(async () => {
  if (autoLoadSource) {
    await loadPage();
  }
  updateScreenState();
  window.addEventListener('resize', updateScreenState);
  window.addEventListener('dragover', handleWindowDragOver);
  window.addEventListener('drop', handleWindowDrop);
  window.addEventListener('dragenter', handleWindowDragEnter);
  window.addEventListener('dragleave', handleWindowDragLeave);
  document.addEventListener('dragover', handleWindowDragOver, true);
  document.addEventListener('drop', handleWindowDrop, true);
  document.addEventListener('dragenter', handleWindowDragEnter, true);
  document.addEventListener('dragleave', handleWindowDragLeave, true);
});

onBeforeUnmount(() => {
  if (derivedTimer !== null) {
    window.clearTimeout(derivedTimer);
    derivedTimer = null;
  }
  window.removeEventListener('resize', updateScreenState);
  window.removeEventListener('dragover', handleWindowDragOver);
  window.removeEventListener('drop', handleWindowDrop);
  window.removeEventListener('dragenter', handleWindowDragEnter);
  window.removeEventListener('dragleave', handleWindowDragLeave);
  document.removeEventListener('dragover', handleWindowDragOver, true);
  document.removeEventListener('drop', handleWindowDrop, true);
  document.removeEventListener('dragenter', handleWindowDragEnter, true);
  document.removeEventListener('dragleave', handleWindowDragLeave, true);
});

function resolveFallbackTitle(): string {
  if (pagePath.value) {
    const parts = pagePath.value.split('/').filter(Boolean);
    return parts.length > 0 ? parts[parts.length - 1] : '/';
  }
  return '編集画面';
}

function updateDerivedFromText(text: string): void {
  const path = pagePath.value || resolveEditPath();
  editorTitle.value = extractTitle(text, path) || resolveFallbackTitle();
  tocItems.value = extractToc(text);
  const renderer = ensureMarkdownRenderer();
  renderedHtml.value = renderer.render(text);
}

function scheduleDerivedUpdate(text: string): void {
  if (derivedTimer !== null) {
    window.clearTimeout(derivedTimer);
  }
  derivedTimer = window.setTimeout(() => {
    derivedTimer = null;
    updateDerivedFromText(text);
  }, 300);
}

watch(sourceText, (value) => {
  scheduleDerivedUpdate(value);
});

watch(pagePath, () => {
  updateDerivedFromText(sourceText.value);
});

watch(isLoading, (loading) => {
  if (loading) {
    sourceStatus.value = '読み込み中';
    return;
  }
  sourceStatus.value = pageId.value ? '読込完了' : '新規ページ';
});

const isAssetDragging = ref(false);
const assetDragDepth = ref(0);
const assetInputRef = ref<HTMLInputElement | null>(null);
const isGlobalDragging = ref(false);
const globalDragDepth = ref(0);

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

function buildAssetDownloadUrl(fileName: string): string {
  if (!pageId.value) {
    return '#';
  }
  const encoded = encodeURIComponent(fileName);
  return `/api/pages/${pageId.value}/assets/${encoded}`;
}
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
    <div class="mx-auto flex min-h-screen max-w-none flex-col gap-1 px-4 py-8 lg:px-8">
      <header class="flex flex-col gap-1">
        <div>
          <p class="text-xs font-semibold uppercase tracking-[0.32em] text-base-content/60">
            LUWIKI EDIT
          </p>
          <h1 class="text-3xl font-bold leading-tight sm:text-4xl mt-3 mb-2">
            {{ editorTitle || '編集画面' }}
          </h1>
          <nav
            class="flex flex-wrap items-center gap-1 text-sm text-info mx-4 mt-3"
            aria-label="breadcrumb"
          >
            <template v-for="(item, index) in breadcrumbItems" :key="`${item.label}-${index}`">
              <span class="inline-flex items-center text-base-content/70">
                <span class="inline-block max-w-full truncate">{{ item.label }}</span>
              </span>
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
            :disabled="assetUploadDisabled"
            @click="openAssetPicker"
          >
            アセット追加
          </button>

          <button
            class="btn btn-link btn-sm text-info"
            type="button"
            :disabled="!canSave || isSaving"
            @click="savePage(sourceText)"
          >
            保存
          </button>
          <button
            class="btn btn-link btn-sm text-info"
            type="button"
            :disabled="interactionDisabled"
            @click="cancelEdit"
          >
            キャンセル
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

      <div class="flex items-center gap-2">
        <button
          class="btn btn-ghost btn-xs hidden md:inline-flex"
          type="button"
          @click="sidePanelCollapsed = !sidePanelCollapsed"
        >
          {{ sidePanelCollapsed ? 'サイドパネルを開く' : 'サイドパネルを閉じる' }}
        </button>
        <button
          class="btn btn-ghost btn-xs lg:hidden"
          type="button"
          @click="previewMode = !previewMode"
        >
          {{ previewMode ? '編集' : 'プレビュー' }}
        </button>

          <label
            v-if="!isNewPage && !isDraftPage"
            class="flex items-center gap-1 text-xs ml-auto text-base-content/70"
          >
            <input
              class="checkbox checkbox-ghost checkbox-xs"
              type="checkbox"
              :checked="amendChecked"
              @change="setAmendChecked(($event.target as HTMLInputElement).checked)"
            />
            <span>リビジョンを維持</span>
          </label>
      </div>


      <main
        class="grid min-h-0 flex-1 items-stretch gap-1"
        :class="
          sidePanelCollapsed
            ? 'md:grid-cols-[minmax(0,1fr)] lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]'
            : 'md:grid-cols-[220px_minmax(0,1fr)] lg:grid-cols-[220px_minmax(0,1fr)_minmax(0,1fr)]'
        "
      >
        <aside
          v-if="!sidePanelCollapsed"
          class="order-2 hidden flex-col gap-1 md:flex md:order-1"
        >
          <section class="h-full overflow-auto border border-base-300 bg-base-100 p-3 shadow-sm">
            <h2 class="mb-3 text-lg font-semibold">TOC</h2>
            <ul v-if="tocItems.length > 0" class="flex flex-col gap-1 text-sm">
              <li v-for="entry in tocItems" :key="entry.anchor">
                <a
                  class="link link-hover"
                  :class="{
                    'pl-3': entry.level === 3,
                    'pl-6': entry.level === 4,
                  }"
                  :href="`#${entry.anchor}`"
                >
                  {{ entry.text }}
                </a>
              </li>
            </ul>
            <p v-else class="text-xs text-base-content/50">（未読み込み）</p>
          </section>
        </aside>

        <div
          v-if="showEditorPanel"
          class="order-1 flex min-h-0 flex-col gap-1 md:order-2"
        >
          <section class="flex min-h-0 flex-1 border border-base-300 bg-base-100 shadow-sm">
            <textarea
              v-model="sourceText"
              placeholder="Markdownを入力してください"
              class="h-full w-full resize-none bg-base-100 p-3 text-sm"
            />
          </section>

          <section
            class="border p-4 shadow-sm transition-colors"
            :class="[
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

        <div
          v-if="showPreviewPanel"
          class="order-3 flex min-h-0 flex-col gap-1 md:order-3"
          :class="{ 'md:col-span-1 lg:col-span-1': !sidePanelCollapsed }"
        >
          <section class="flex min-h-0 flex-1 overflow-hidden border border-base-300 bg-transparent shadow-sm">
            <article
              class="markdown-body flex-1 w-full overflow-auto p-4"
              :class="[markdownThemeClass, prismThemeClass]"
              :style="markdownStyle"
              v-html="renderedHtml"
            />
          </section>
        </div>
      </main>

      <div v-if="errorMessage" class="alert alert-error">
        {{ errorMessage }}
      </div>
    </div>

    <div v-if="restorePromptVisible" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">削除済みページの候補</h3>
        <p class="text-sm text-base-content/70">
          このパスには削除済みページが存在します。復活するページを選択するか、
          新規作成に進んでください。
        </p>
        <div class="grid gap-3 md:grid-cols-[220px_minmax(0,1fr)]">
          <div class="space-y-2">
            <div
              v-for="candidate in restoreCandidates"
              :key="candidate"
              class="flex items-center gap-2"
            >
              <input
                class="radio radio-sm"
                type="radio"
                name="restoreCandidate"
                :value="candidate"
                v-model="restoreCandidateId"
                @change="selectRestoreCandidate(candidate)"
              />
              <span class="text-xs font-semibold">{{ candidate }}</span>
            </div>
          </div>
          <div class="border border-base-300 bg-base-100 p-3">
            <p v-if="restoreCandidateLoading" class="text-xs text-base-content/70">
              読み込み中...
            </p>
            <pre
              v-else
              class="max-h-60 overflow-auto whitespace-pre-wrap text-xs text-base-content/80"
            >{{ restoreCandidateSource || '（プレビューなし）' }}</pre>
          </div>
        </div>
        <p class="text-xs text-base-content/60">
          ※他のパスで復活が必要な場合は、該当ページIDを管理者に通知してください。
        </p>
        <div class="modal-action">
          <button
            class="btn"
            type="button"
            :disabled="restoreInProgress"
            @click="skipRestoreCandidates"
          >
            新規作成
          </button>
          <button
            class="btn btn-primary"
            type="button"
            :disabled="!restoreCandidateId || restoreInProgress || restoreCandidateLoading"
            @click="confirmRestoreCandidate"
          >
            復活して編集
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

    <div v-if="newPageToastVisible" class="toast toast-end toast-bottom z-50">
      <div class="alert border border-base-300 bg-base-100 text-base-content shadow-lg">
        <div class="flex flex-col gap-2">
          <span class="text-sm font-semibold">新規ページ</span>
          <span class="text-xs text-base-content/70">{{ newPageToastMessage }}</span>
          <div class="flex gap-2">
            <button class="btn btn-xs" type="button" @click="dismissNewPageToast">
              閉じる
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
