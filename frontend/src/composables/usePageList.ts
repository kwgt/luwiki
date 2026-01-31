import { computed, ref, watch } from 'vue';
import {
  fetchPageList,
  fetchPageSource,
  restorePagePath,
  type PageListItem,
} from '../api/pages';
import { normalizeWikiPath, resolvePagePath, toErrorMessage } from '../lib/pageCommon';

const PAGE_LIMIT = 100;

function resolveListPath(): string {
  const raw = window.location.pathname;
  const trimmed = raw.replace(/^\/pages\/?/, '');
  if (!trimmed) {
    return '/';
  }
  const decoded = trimmed
    .split('/')
    .map((segment) => {
      try {
        return decodeURIComponent(segment);
      } catch {
        return segment;
      }
    })
    .join('/');
  return normalizeWikiPath(decoded);
}

type PageSnapshot = {
  items: PageListItem[];
  hasMore: boolean;
  anchor?: string;
};

export function usePageList() {
  const pagePath = ref(resolveListPath());
  const withDeleted = ref(false);
  const isLoading = ref(false);
  const errorMessage = ref('');
  const pageHistory = ref<PageSnapshot[]>([]);
  const currentIndex = ref(0);

  const currentPage = computed(
    () => pageHistory.value[currentIndex.value] ?? null,
  );
  const items = computed(
    () => currentPage.value?.items ?? [],
  );
  const canGoPrev = computed(
    () => currentIndex.value > 0 && !isLoading.value,
  );
  const canGoNext = computed(
    () =>
      !isLoading.value
      && !!currentPage.value?.hasMore
      && !!currentPage.value?.anchor,
  );

  const restoreOpen = ref(false);
  const restoreTarget = ref('');
  const restoreSource = ref('');
  const restoreSourceLoading = ref(false);
  const restoreRecursive = ref(false);
  const restoreInProgress = ref(false);
  const restoreError = ref('');
  const restoreItem = ref<PageListItem | null>(null);

  const restoreResolvedPath = computed(() => {
    const raw = restoreTarget.value.trim();
    if (!raw) {
      return null;
    }
    const base = pagePath.value || '/';
    return resolvePagePath(base, raw);
  });

  const existingPaths = computed(() => {
    const set = new Set<string>();
    for (const snapshot of pageHistory.value) {
      for (const item of snapshot.items) {
        if (!item.deleted) {
          set.add(normalizeWikiPath(item.path));
        }
      }
    }
    return set;
  });

  const restoreInputError = computed(() => {
    const raw = restoreTarget.value.trim();
    if (!raw) {
      return '';
    }
    if (raw.endsWith('/')) {
      return '末尾に"/"は指定できません';
    }
    const resolved = restoreResolvedPath.value;
    if (!resolved) {
      return '復元先パスが不正です';
    }
    if (resolved === '/') {
      return '復元先がルートページです';
    }
    if (existingPaths.value.has(resolved)) {
      return '同名のページが既に存在します';
    }
    return '';
  });

  async function fetchPage(index: number, cursor: string): Promise<void> {
    isLoading.value = true;
    errorMessage.value = '';
    try {
      const response = await fetchPageList({
        prefix: pagePath.value,
        forward: cursor,
        limit: PAGE_LIMIT,
        withDeleted: withDeleted.value,
      });
      pageHistory.value[index] = {
        items: response.items,
        hasMore: response.has_more,
        anchor: response.anchor,
      };
      currentIndex.value = index;
    } catch (err: unknown) {
      errorMessage.value = toErrorMessage(err);
    } finally {
      isLoading.value = false;
    }
  }

  async function loadPageList(reset = false): Promise<void> {
    if (reset) {
      pageHistory.value = [];
      currentIndex.value = 0;
    }
    await fetchPage(0, pagePath.value);
  }

  async function goNext(): Promise<void> {
    if (!canGoNext.value || !currentPage.value?.anchor) {
      return;
    }
    const nextIndex = currentIndex.value + 1;
    const cached = pageHistory.value[nextIndex];
    if (cached) {
      currentIndex.value = nextIndex;
      return;
    }
    await fetchPage(nextIndex, currentPage.value.anchor);
  }

  function goPrev(): void {
    if (!canGoPrev.value) {
      return;
    }
    currentIndex.value -= 1;
  }

  async function openRestore(item: PageListItem): Promise<void> {
    restoreItem.value = item;
    restoreTarget.value = item.path;
    restoreSource.value = '';
    restoreError.value = '';
    restoreRecursive.value = false;
    restoreOpen.value = true;
    restoreSourceLoading.value = true;
    try {
      const source = await fetchPageSource(item.page_id);
      restoreSource.value = source;
    } catch (err: unknown) {
      restoreError.value = toErrorMessage(err);
    } finally {
      restoreSourceLoading.value = false;
    }
  }

  function closeRestore(): void {
    restoreOpen.value = false;
    restoreItem.value = null;
    restoreTarget.value = '';
    restoreSource.value = '';
    restoreRecursive.value = false;
    restoreError.value = '';
    restoreInProgress.value = false;
  }

  async function confirmRestore(): Promise<void> {
    if (!restoreItem.value || restoreInProgress.value) {
      return;
    }
    if (restoreInputError.value) {
      restoreError.value = restoreInputError.value;
      return;
    }
    const resolved = restoreResolvedPath.value;
    if (!resolved) {
      restoreError.value = '復元先パスが不正です';
      return;
    }
    restoreInProgress.value = true;
    restoreError.value = '';
    try {
      await restorePagePath(
        restoreItem.value.page_id,
        resolved,
        restoreRecursive.value,
      );
      const nextUrl = resolved === '/' ? '/wiki/' : `/wiki${resolved}`;
      window.location.replace(nextUrl);
    } catch (err: unknown) {
      restoreError.value = toErrorMessage(err);
      restoreInProgress.value = false;
    }
  }

  watch(withDeleted, () => {
    void loadPageList(true);
  });

  watch(restoreTarget, () => {
    if (restoreError.value) {
      restoreError.value = '';
    }
  });

  return {
    pagePath,
    withDeleted,
    isLoading,
    errorMessage,
    items,
    currentIndex,
    canGoPrev,
    canGoNext,
    loadPageList,
    goNext,
    goPrev,
    restoreOpen,
    restoreTarget,
    restoreSource,
    restoreSourceLoading,
    restoreRecursive,
    restoreInProgress,
    restoreError,
    restoreInputError,
    openRestore,
    closeRestore,
    confirmRestore,
  };
}
