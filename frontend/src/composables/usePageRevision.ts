import { computed, ref, watch, type Ref } from 'vue';
import {
  compactPageRevision,
  fetchPageMeta,
  fetchPageSource,
  rollbackPageRevision,
  type PageMetaResponse,
} from '../api/pages';
import {
  extractTitle,
  getMetaContent,
  normalizeWikiPath,
  toErrorMessage,
} from '../lib/pageCommon';
import {
  createPatch,
  diffChars,
  diffLines,
  diffWords,
  type Change,
} from 'diff';

export type DiffMode = 'lines' | 'words' | 'chars' | 'patch';

type RevisionScope = {
  latest: number;
  oldest: number;
};

type RenameInfo = {
  from?: string;
  to: string;
};

type RevisionMeta = {
  revision: number;
  timestamp: string;
  username: string;
  rename_info?: RenameInfo;
};

type SourceCache = Record<number, string>;
type MetaCache = Record<number, PageMetaResponse>;
type RenameTooltipState = {
  text: string;
  loading: boolean;
};

type MetaLoadingMap = Record<number, boolean>;

function formatTimestampLocal(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  const pad = (num: number) => String(num).padStart(2, '0');
  return [
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`,
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`,
  ].join(' ');
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function renderDiffParts(parts: Change[]): string {
  return parts.map((part) => {
    const className = part.added
      ? 'bg-success/15 text-success'
      : part.removed
        ? 'bg-error/15 text-error'
        : 'text-base-content/70';
    return `<span class="${className}">${escapeHtml(part.value)}</span>`;
  }).join('');
}

function renderPatch(patch: string): string {
  return patch
    .split('\n')
    .map((line) => {
      let className = 'text-base-content/70';
      if (line.startsWith('@@')) {
        className = 'text-info';
      } else if (line.startsWith('+++') || line.startsWith('---')) {
        className = 'text-info';
      } else if (line.startsWith('+')) {
        className = 'text-success';
      } else if (line.startsWith('-')) {
        className = 'text-error';
      }
      return `<span class="${className}">${escapeHtml(line)}</span>`;
    })
    .join('\n');
}

export function usePageRevision(diffMode: Ref<DiffMode>) {
  const pageId = ref('');
  const pagePath = ref('');
  const revisionScope = ref<RevisionScope | null>(null);
  const renameRevisions = ref<number[]>([]);
  const pageMeta = ref<PageMetaResponse | null>(null);
  const sourceCache = ref<SourceCache>({});
  const metaCache = ref<MetaCache>({});
  const metaLoadingMap = ref<MetaLoadingMap>({});
  const renameTooltipMap = ref<Record<number, RenameTooltipState>>({});
  const selectedRevisions = ref<number[]>([]);
  const selectionAnchor = ref<number | null>(null);
  const isLoading = ref(false);
  const isSelectionLoading = ref(false);
  const isActionLoading = ref(false);
  const errorMessage = ref('');

  const revisions = computed(() => {
    if (!revisionScope.value) {
      return [];
    }
    const { latest, oldest } = revisionScope.value;
    const items: number[] = [];
    for (let rev = latest; rev >= oldest; rev -= 1) {
      items.push(rev);
    }
    return items;
  });

  const selectedRevisionSet = computed(() => new Set(selectedRevisions.value));
  const isRangeSelection = computed(() => selectedRevisions.value.length >= 2);
  const selectedRange = computed(() => {
    if (selectedRevisions.value.length < 2) {
      return null;
    }
    const sorted = [...selectedRevisions.value].sort((a, b) => a - b);
    return { left: sorted[0], right: sorted[sorted.length - 1] };
  });
  const selectedSource = computed(() => {
    if (selectedRevisions.value.length !== 1) {
      return '';
    }
    return sourceCache.value[selectedRevisions.value[0]] ?? '';
  });
  const fallbackTitleSource = computed(() => {
    if (revisionScope.value) {
      const latest = revisionScope.value.latest;
      return sourceCache.value[latest] ?? '';
    }
    return '';
  });
  const pageTitle = computed(() => {
    const source = selectedSource.value || fallbackTitleSource.value;
    return extractTitle(source, pagePath.value || '/');
  });
  const selectedRevisionInfo = computed<RevisionMeta | null>(() => {
    if (!selectedRange.value) {
      const rev = selectedRevisions.value[0];
      if (!rev) {
        return null;
      }
      const meta = metaCache.value[rev];
      return meta?.revision_info ?? null;
    }
    const right = selectedRange.value.right;
    const meta = metaCache.value[right];
    return meta?.revision_info ?? null;
  });
  const isPageLocked = computed(() => pageMeta.value?.page_info.locked ?? false);
  const diffHtml = computed(() => {
    if (!selectedRange.value) {
      return '';
    }
    const leftSource = sourceCache.value[selectedRange.value.left];
    const rightSource = sourceCache.value[selectedRange.value.right];
    if (leftSource === undefined || rightSource === undefined) {
      return '';
    }
    if (diffMode.value === 'patch') {
      const patch = createPatch(
        `rev-${selectedRange.value.left}-to-${selectedRange.value.right}`,
        leftSource,
        rightSource,
      );
      return renderPatch(patch);
    }
    const parts = diffMode.value === 'chars'
      ? diffChars(leftSource, rightSource)
      : diffMode.value === 'words'
        ? diffWords(leftSource, rightSource)
        : diffLines(leftSource, rightSource);
    return renderDiffParts(parts);
  });

  function isSelected(revision: number): boolean {
    return selectedRevisionSet.value.has(revision);
  }

  function isRenameRevision(revision: number): boolean {
    // リビジョン1は必ずページの新規作成なので特例として
    // リネームではないものとして扱っている
    return revision != 1 && renameRevisions.value.includes(revision);
  }

  async function ensureSource(revision: number): Promise<void> {
    if (sourceCache.value[revision] !== undefined) {
      return;
    }
    const source = await fetchPageSource(pageId.value, revision);
    sourceCache.value = {
      ...sourceCache.value,
      [revision]: source,
    };
  }

  async function ensureMeta(revision: number): Promise<void> {
    if (metaCache.value[revision]) {
      return;
    }
    const meta = await fetchPageMeta(pageId.value, revision);
    metaCache.value = {
      ...metaCache.value,
      [revision]: meta,
    };
  }

  async function preloadRevisionMeta(revision: number): Promise<void> {
    if (metaCache.value[revision] || metaLoadingMap.value[revision]) {
      return;
    }
    metaLoadingMap.value = {
      ...metaLoadingMap.value,
      [revision]: true,
    };
    try {
      await ensureMeta(revision);
    } finally {
      const next = { ...metaLoadingMap.value };
      delete next[revision];
      metaLoadingMap.value = next;
    }
  }

  function hasRevisionMeta(revision: number): boolean {
    return Boolean(metaCache.value[revision]?.revision_info);
  }

  function getRevisionUsername(revision: number): string {
    return metaCache.value[revision]?.revision_info?.username ?? '';
  }

  function getRevisionTimestamp(revision: number): string {
    const timestamp = metaCache.value[revision]?.revision_info?.timestamp;
    return timestamp ? formatTimestampLocal(timestamp) : '';
  }

  async function preloadRenameMeta(revision: number): Promise<void> {
    if (renameTooltipMap.value[revision]?.loading) {
      return;
    }
    const cached = metaCache.value[revision];
    if (cached) {
      updateRenameTooltip(revision, cached);
      return;
    }
    renameTooltipMap.value = {
      ...renameTooltipMap.value,
      [revision]: {
        text: '読み込み中...',
        loading: true,
      },
    };
    try {
      const meta = await fetchPageMeta(pageId.value, revision);
      metaCache.value = {
        ...metaCache.value,
        [revision]: meta,
      };
      updateRenameTooltip(revision, meta);
    } catch {
      renameTooltipMap.value = {
        ...renameTooltipMap.value,
        [revision]: {
          text: '情報なし',
          loading: false,
        },
      };
    }
  }

  function updateRenameTooltip(revision: number, meta: PageMetaResponse): void {
    const renameInfo = meta.revision_info?.rename_info;
    const text = renameInfo
      ? `${renameInfo.from ?? '-'} \u27a1 ${renameInfo.to}`
      : '情報なし';
    renameTooltipMap.value = {
      ...renameTooltipMap.value,
      [revision]: {
        text,
        loading: false,
      },
    };
  }

  function getRenameTooltip(revision: number): string {
    return renameTooltipMap.value[revision]?.text ?? '読み込み中...';
  }

  async function loadPage(): Promise<void> {
    const rawPageId = getMetaContent('wiki-page-id');
    const rawRevision = getMetaContent('wiki-page-revision');

    if (!rawPageId) {
      errorMessage.value = 'page id not found';
      return;
    }

    pageId.value = rawPageId;
    const currentRevision = rawRevision ? Number(rawRevision) : null;
    if (!currentRevision) {
      errorMessage.value = 'revision not found';
      return;
    }

    isLoading.value = true;
    errorMessage.value = '';

    try {
      const meta = await fetchPageMeta(pageId.value, currentRevision);
      pageMeta.value = meta;
      pagePath.value = normalizeWikiPath(meta.page_info.path.value);
      revisionScope.value = meta.page_info.revision_scope;
      renameRevisions.value = meta.page_info.rename_revisions;
      metaCache.value = {
        ...metaCache.value,
        [currentRevision]: meta,
      };
      selectedRevisions.value = [meta.page_info.revision_scope.latest];
      selectionAnchor.value = meta.page_info.revision_scope.latest;
      await ensureSource(meta.page_info.revision_scope.latest);
    } catch (err: unknown) {
      errorMessage.value = toErrorMessage(err);
    } finally {
      isLoading.value = false;
    }
  }

  function pruneCaches(scope: RevisionScope): void {
    const { latest, oldest } = scope;
    const nextSourceCache: SourceCache = {};
    for (const [key, value] of Object.entries(sourceCache.value)) {
      const revision = Number(key);
      if (!Number.isNaN(revision) && revision >= oldest && revision <= latest) {
        nextSourceCache[revision] = value;
      }
    }
    sourceCache.value = nextSourceCache;

    const nextMetaCache: MetaCache = {};
    for (const [key, value] of Object.entries(metaCache.value)) {
      const revision = Number(key);
      if (!Number.isNaN(revision) && revision >= oldest && revision <= latest) {
        nextMetaCache[revision] = value;
      }
    }
    metaCache.value = nextMetaCache;
  }

  async function refreshPageMeta(): Promise<void> {
    const meta = await fetchPageMeta(pageId.value);
    pageMeta.value = meta;
    pagePath.value = normalizeWikiPath(meta.page_info.path.value);
    revisionScope.value = meta.page_info.revision_scope;
    renameRevisions.value = meta.page_info.rename_revisions;
    metaCache.value = {
      ...metaCache.value,
      [meta.page_info.revision_scope.latest]: meta,
    };
    pruneCaches(meta.page_info.revision_scope);
    selectedRevisions.value = [meta.page_info.revision_scope.latest];
    selectionAnchor.value = meta.page_info.revision_scope.latest;
    await ensureSource(meta.page_info.revision_scope.latest);
  }

  async function rollbackRevision(targetRevision: number): Promise<boolean> {
    if (!pageId.value) {
      errorMessage.value = 'page id not found';
      return false;
    }
    isActionLoading.value = true;
    errorMessage.value = '';
    try {
      await rollbackPageRevision(pageId.value, targetRevision);
      await refreshPageMeta();
      return true;
    } catch (err: unknown) {
      errorMessage.value = toErrorMessage(err);
      return false;
    } finally {
      isActionLoading.value = false;
    }
  }

  async function compactRevision(targetRevision: number): Promise<boolean> {
    if (!pageId.value) {
      errorMessage.value = 'page id not found';
      return false;
    }
    isActionLoading.value = true;
    errorMessage.value = '';
    try {
      await compactPageRevision(pageId.value, targetRevision);
      await refreshPageMeta();
      return true;
    } catch (err: unknown) {
      errorMessage.value = toErrorMessage(err);
      return false;
    } finally {
      isActionLoading.value = false;
    }
  }

  function selectRevision(revision: number, withRange: boolean): void {
    if (!revisionScope.value) {
      return;
    }
    if (withRange && selectionAnchor.value !== null) {
      const start = Math.min(selectionAnchor.value, revision);
      const end = Math.max(selectionAnchor.value, revision);
      const range: number[] = [];
      for (let rev = end; rev >= start; rev -= 1) {
        range.push(rev);
      }
      selectedRevisions.value = range;
      return;
    }
    selectedRevisions.value = [revision];
    selectionAnchor.value = revision;
  }

  watch(selectedRevisions, async (value) => {
    if (value.length === 0) {
      return;
    }
    const needed = selectedRange.value
      ? [selectedRange.value.left, selectedRange.value.right]
      : [value[0]];
    isSelectionLoading.value = true;
    try {
      await Promise.all(needed.map((revision) => ensureSource(revision)));
      if (selectedRange.value) {
        await ensureMeta(selectedRange.value.right);
      } else {
        await ensureMeta(value[0]);
      }
    } catch (err: unknown) {
      errorMessage.value = toErrorMessage(err);
    } finally {
      isSelectionLoading.value = false;
    }
  });

  return {
    pageId,
    pagePath,
    pageTitle,
    pageMeta,
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
  };
}
