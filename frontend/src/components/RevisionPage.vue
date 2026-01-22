<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { usePageRevision } from '../composables/usePageRevision';
import { useUiSettings } from '../composables/useUiSettings';
import EditorPane from './EditorPane.vue';

const {
  themeOptions,
  fontOptions,
  editorKeymapOptions,
  diffModeOptions,
  selectedTheme,
  selectedFont,
  selectedFontSize,
  selectedCodeFontSize,
  selectedEditorKeymap,
  selectedEditorLineNumbers,
  selectedDiffMode,
  editorStyle,
  markdownStyle,
} = useUiSettings();

const {
  pageId,
  pagePath,
  pageTitle,
  revisions,
  renameRevisions,
  selectedRevisions,
  selectedRevisionInfo,
  selectedSource,
  diffHtml,
  isLoading,
  isSelectionLoading,
  isActionLoading,
  isRangeSelection,
  isPageLocked,
  errorMessage,
  loadPage,
  selectRevision,
  isSelected,
  isRenameRevision,
  preloadRenameMeta,
  getRenameTooltip,
  preloadRevisionMeta,
  hasRevisionMeta,
  getRevisionUsername,
  getRevisionTimestamp,
  rollbackRevision,
  compactRevision,
} = usePageRevision(selectedDiffMode);

const settingsOpen = ref(false);
const sidePanelCollapsed = ref(false);
const rollbackConfirmOpen = ref(false);
const compactionConfirmOpen = ref(false);
const noticeOpen = ref(false);
const noticeMessage = ref('');
const revisionElements = new Map<number, Element>();
const revisionElementMap = new WeakMap<Element, number>();
let revisionObserver: IntersectionObserver | null = null;
const selectedSingleRevision = computed(() =>
  selectedRevisions.value.length === 1 ? selectedRevisions.value[0] : null,
);
const latestRevision = computed(() => (revisions.value[0] ?? null));
const rollbackCrossesRename = computed(() => {
  const target = selectedSingleRevision.value;
  const latest = latestRevision.value;
  if (!target || !latest) {
    return false;
  }
  if (target >= latest) {
    return false;
  }
  return renameRevisions.value.some(
    (revision) => revision > target && revision <= latest,
  );
});

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

const canExecuteAction = computed(
  () =>
    selectedRevisions.value.length === 1
    && !isLoading.value
    && !isSelectionLoading.value
    && !isActionLoading.value
    && !isPageLocked.value
    && !errorMessage.value,
);
const canRollback = computed(() => {
  const target = selectedSingleRevision.value;
  const latest = latestRevision.value;
  if (!target || !latest) {
    return false;
  }
  return canExecuteAction.value && target < latest;
});
const canCompaction = computed(() => {
  const target = selectedSingleRevision.value;
  const oldest = revisions.value[revisions.value.length - 1] ?? null;
  if (!target || !oldest) {
    return false;
  }
  return canExecuteAction.value && target > oldest;
});

const selectionLabel = computed(() => {
  if (selectedRevisions.value.length === 0) {
    return '';
  }
  if (selectedRevisions.value.length === 1) {
    return `Rev ${selectedRevisions.value[0]}`;
  }
  const sorted = [...selectedRevisions.value].sort((a, b) => a - b);
  return `Rev ${sorted[0]} - ${sorted[sorted.length - 1]}`;
});

const diffHeaderLabel = computed(() => {
  if (!isRangeSelection.value) {
    return '';
  }
  const sorted = [...selectedRevisions.value].sort((a, b) => a - b);
  return `Rev ${sorted[0]} → Rev ${sorted[sorted.length - 1]}`;
});

function applySidePanelCollapsed(value: boolean): void {
  localStorage.setItem('luwiki-side-collapsed', value ? '1' : '0');
}

function toggleSidePanel(): void {
  sidePanelCollapsed.value = !sidePanelCollapsed.value;
  applySidePanelCollapsed(sidePanelCollapsed.value);
}

function handleRevisionClick(revision: number, event: MouseEvent): void {
  selectRevision(revision, event.shiftKey);
}

function setRevisionRef(revision: number, el: Element | null): void {
  const existing = revisionElements.get(revision);
  if (!el) {
    if (existing && revisionObserver) {
      revisionObserver.unobserve(existing);
    }
    revisionElements.delete(revision);
    return;
  }
  revisionElements.set(revision, el);
  revisionElementMap.set(el, revision);
  if (revisionObserver) {
    revisionObserver.observe(el);
  }
}

function setupRevisionObserver(): void {
  if (typeof IntersectionObserver === 'undefined') {
    return;
  }
  revisionObserver = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) {
        continue;
      }
      const revision = revisionElementMap.get(entry.target);
      if (!revision) {
        continue;
      }
      if (hasRevisionMeta(revision)) {
        revisionObserver?.unobserve(entry.target);
        continue;
      }
      void preloadRevisionMeta(revision);
    }
  }, { root: null, rootMargin: '0px', threshold: 0.1 });

  for (const el of revisionElements.values()) {
    revisionObserver.observe(el);
  }
}

function openRollbackConfirm(): void {
  if (!canExecuteAction.value) {
    return;
  }
  rollbackConfirmOpen.value = true;
}

function openCompactionConfirm(): void {
  if (!canExecuteAction.value) {
    return;
  }
  compactionConfirmOpen.value = true;
}

async function confirmRollback(): Promise<void> {
  const targetRevision = selectedSingleRevision.value;
  if (!targetRevision) {
    return;
  }
  rollbackConfirmOpen.value = false;
  const ok = await rollbackRevision(targetRevision);
  if (!ok) {
    return;
  }
  noticeMessage.value = `Rev ${targetRevision} へロールバックしました。`;
  noticeOpen.value = true;
}

async function confirmCompaction(): Promise<void> {
  const targetRevision = selectedSingleRevision.value;
  if (!targetRevision) {
    return;
  }
  compactionConfirmOpen.value = false;
  const ok = await compactRevision(targetRevision);
  if (!ok) {
    return;
  }
  noticeMessage.value = `Rev ${targetRevision} 以降を保持しました。`;
  noticeOpen.value = true;
}

function closeNotice(): void {
  noticeOpen.value = false;
  noticeMessage.value = '';
}

onBeforeUnmount(() => {
  revisionObserver?.disconnect();
  revisionObserver = null;
});

onMounted(() => {
  const savedCollapsed = localStorage.getItem('luwiki-side-collapsed');
  if (savedCollapsed === '1') {
    sidePanelCollapsed.value = true;
  }
  if (!window.matchMedia('(min-width: 768px)').matches) {
    sidePanelCollapsed.value = true;
  }
  setupRevisionObserver();
  void loadPage();
});
</script>

<template>
  <div class="min-h-screen bg-base-200 text-base-content" :data-theme="selectedTheme">
    <div class="mx-auto flex max-w-6xl flex-col gap-1 px-4 pt-8 pb-[0.25rem] lg:px-10">
      <header class="flex flex-col gap-1">
        <div>
          <p class="text-xs font-semibold uppercase tracking-[0.32em] text-base-content/60">
            LuWiki REVISION
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

        <nav class="flex flex-wrap items-center gap-1">
          <button
            class="btn btn-link btn-sm pl-1 text-info"
            type="button"
            :disabled="!canRollback"
            @click="openRollbackConfirm"
          >
            ロールバック
          </button>
          <button
            class="btn btn-link btn-sm text-info"
            type="button"
            :disabled="!canCompaction"
            @click="openCompactionConfirm"
          >
            コンパクション
          </button>
          <button
            class="btn btn-link btn-sm pr-1 text-info ml-auto"
            type="button"
            @click="settingsOpen = true"
          >
            設定
          </button>
        </nav>
      </header>

      <div class="flex items-center justify-start">
        <button class="btn btn-ghost btn-xs" type="button" @click="toggleSidePanel">
          {{ sidePanelCollapsed ? 'サイドパネルを開く' : 'サイドパネルを閉じる' }}
        </button>
      </div>

      <main
        class="relative grid items-stretch gap-1 min-h-[calc(100vh-12.8em)]"
        :class="
          sidePanelCollapsed
            ? 'md:grid-cols-[minmax(0,1fr)]'
            : 'md:grid-cols-[220px_minmax(0,1fr)]'
        "
      >
        <aside
          v-if="!sidePanelCollapsed"
          class="order-2 flex flex-col gap-2 absolute inset-y-0 left-0 z-20 w-[220px] max-w-[85vw] md:static md:inset-auto md:z-auto md:w-auto md:max-w-none md:order-1"
        >
          <section class="h-full border border-base-300 bg-base-100 p-2 shadow-sm">
            <div class="mb-2 text-lg font-semibold">リビジョン一覧</div>
            <div v-if="isLoading" class="text-sm text-base-content/60">読み込み中...</div>
            <div v-else class="flex flex-col gap-1">
              <button
                v-for="rev in revisions"
                :key="rev"
                class="flex items-center justify-between gap-2 rounded px-2 py-1 text-left text-xs font-semibold"
                :class="isSelected(rev) ? 'bg-primary text-primary-content' : 'bg-base-200'"
                type="button"
                :ref="(el) => setRevisionRef(rev, el)"
                @click="handleRevisionClick(rev, $event)"
              >
                <div class="flex min-w-0 flex-1 flex-col gap-0.5">
                  <div class="flex items-center justify-between gap-2">
                    <span class="font-mono">Rev {{ rev }}</span>
                    <span
                      v-if="isRenameRevision(rev)"
                      class="tooltip tooltip-right font-normal"
                      :data-tip="getRenameTooltip(rev)"
                      @mouseenter="preloadRenameMeta(rev)"
                    >
                      <span class="badge badge-sm font-bold">Rename</span>
                    </span>
                  </div>
                  <span
                    class="block min-h-[0.9rem] truncate text-[10px]"
                    :class="isSelected(rev) ? 'text-primary-content/80' : 'text-base-content/70'"
                  >
                    {{ getRevisionUsername(rev) }}
                  </span>
                  <span
                    class="block min-h-[0.9rem] text-[10px]"
                    :class="isSelected(rev) ? 'text-primary-content/70' : 'text-base-content/60'"
                  >
                    {{ getRevisionTimestamp(rev) }}
                  </span>
                </div>
              </button>
            </div>
          </section>
        </aside>

        <section class="order-1 flex min-h-[240px] flex-col gap-2 md:order-2">
          <div class="border border-base-300 bg-base-100 p-2 shadow-sm">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div class="text-lg font-semibold">
                {{ isRangeSelection ? '差分表示' : 'ソース表示' }}
              </div>
              <div class="text-xs text-base-content/60">
                {{ isRangeSelection ? diffHeaderLabel : selectionLabel }}
              </div>
            </div>
            <div class="mt-2 text-xs text-base-content/60">
              <div>page_id: {{ pageId }}</div>
            </div>
          </div>

          <div class="flex flex-1 flex-col border border-base-300 bg-base-100 shadow-sm">
            <div v-if="isLoading || isSelectionLoading" class="p-4 text-sm text-base-content/60">
              読み込み中...
            </div>
            <div v-else-if="isRangeSelection" class="overflow-x-auto p-4" :style="editorStyle">
              <pre
                class="whitespace-pre-wrap break-words font-mono text-sm"
                style="font-family: var(--cm-font-family); font-size: var(--cm-font-size);"
                v-html="diffHtml"
              />
            </div>
            <div v-else class="revision-editor min-h-[260px] min-h-0 flex-1" :style="markdownStyle">
              <EditorPane
                :model-value="selectedSource"
                placeholder="ソースがありません。"
                :theme="selectedTheme"
                :keymap="selectedEditorKeymap"
                :line-numbers="selectedEditorLineNumbers"
                :editor-style="editorStyle"
                :read-only="true"
                class="h-full w-full"
              />
            </div>
          </div>
        </section>
      </main>
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
              <span class="label-text">差分表示モード</span>
            </div>
            <select v-model="selectedDiffMode" class="select select-bordered">
              <option v-for="option in diffModeOptions" :key="option.value" :value="option.value">
                {{ option.label }}
              </option>
            </select>
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

    <div v-if="rollbackConfirmOpen" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">ロールバック確認</h3>
        <p class="text-sm text-base-content/70">
          選択中のリビジョン（Rev {{ selectedSingleRevision ?? '-' }}）まで巻き戻します。よろしいですか？
        </p>
        <p
          v-if="rollbackCrossesRename"
          class="text-xs text-warning"
        >
          このロールバックではリネームは巻き戻りません（ソースのみが対象です）。
        </p>
        <div class="modal-action">
          <button class="btn" type="button" @click="rollbackConfirmOpen = false">
            キャンセル
          </button>
          <button class="btn btn-primary" type="button" :disabled="isActionLoading" @click="confirmRollback">
            実行
          </button>
        </div>
      </div>
    </div>

    <div v-if="compactionConfirmOpen" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">コンパクション確認</h3>
        <p class="text-sm text-base-content/70">
          選択中のリビジョン（Rev {{ selectedSingleRevision ?? '-' }}）より前を削除します。よろしいですか？
        </p>
        <div class="modal-action">
          <button class="btn" type="button" @click="compactionConfirmOpen = false">
            キャンセル
          </button>
          <button class="btn btn-primary" type="button" :disabled="isActionLoading" @click="confirmCompaction">
            実行
          </button>
        </div>
      </div>
    </div>

    <div v-if="noticeOpen" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">お知らせ</h3>
        <p class="text-sm text-base-content/70">{{ noticeMessage }}</p>
        <div class="modal-action">
          <button class="btn btn-primary" type="button" @click="closeNotice">
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
          <button class="btn btn-primary" type="button" @click="errorMessage = ''">
            閉じる
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.revision-editor :deep(.cm-editor),
.revision-editor :deep(.cm-scroller),
.revision-editor :deep(.cm-gutters) {
  height: 100%;
}

.revision-editor :deep(.cm-content),
.revision-editor :deep(.cm-gutters) {
  min-height: 100%;
}
</style>
