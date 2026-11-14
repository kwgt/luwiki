import { computed, getCurrentScope, onScopeDispose, ref, watch } from 'vue';
import { searchPages, type SearchResult } from '../api/pages';
import { toErrorMessage } from '../lib/pageCommon';

type SearchTarget = 'headings' | 'body' | 'code' | 'front_matter';

const SEARCH_DEBOUNCE_MS = 300;

export function usePageSearch() {
  const query = ref('');
  const targetHeadings = ref(false);
  const targetBody = ref(true);
  const targetCode = ref(false);
  const targetFrontMatter = ref(false);
  const withDeleted = ref(false);
  const latestOnly = ref(true);
  const results = ref<SearchResult[]>([]);
  const pathFilter = ref('');
  const filteredResults = computed(() => {
    const fragment = pathFilter.value.trim();
    return fragment ? results.value.filter((item) => item.path.includes(fragment))
      : results.value;
  });
  const isLoading = ref(false);
  const errorMessage = ref('');

  const targets = computed<SearchTarget[]>(() => {
    const list: SearchTarget[] = [];
    if (targetHeadings.value) {
      list.push('headings');
    }
    if (targetBody.value) {
      list.push('body');
    }
    if (targetCode.value) {
      list.push('code');
    }
    if (targetFrontMatter.value) {
      list.push('front_matter');
    }
    return list;
  });

  let debounceTimer: number | null = null;
  let requestId = 0;

  function scheduleSearch(): void {
    if (debounceTimer !== null) {
      window.clearTimeout(debounceTimer);
    }
    const current = ++requestId;
    if (!query.value.trim()) {
      results.value = [];
      errorMessage.value = '';
      isLoading.value = false;
      return;
    }

    isLoading.value = true;
    errorMessage.value = '';
    debounceTimer = window.setTimeout(() => {
      debounceTimer = null;
      void runSearch(current);
    }, SEARCH_DEBOUNCE_MS);
  }

  async function runSearch(current: number): Promise<void> {
    if (requestId !== current) {
      return;
    }
    const expr = query.value.trim();

    try {
      const data = await searchPages({
        expression: expr,
        targets: targets.value.length > 0 ? targets.value : ['body'],
        withDeleted: withDeleted.value,
        allRevision: !latestOnly.value,
      });
      if (requestId !== current) {
        return;
      }
      results.value = data;
    } catch (err: unknown) {
      if (requestId !== current) {
        return;
      }
      results.value = [];
      errorMessage.value = toErrorMessage(err);
    } finally {
      if (requestId === current) {
        isLoading.value = false;
      }
    }
  }

  // checkboxの解除描画後に戻し、既定値と実DOMのcheckedを揃える。
  watch([targetHeadings, targetBody, targetCode, targetFrontMatter], () => {
    if (targets.value.length === 0) {
      targetBody.value = true;
    }
  }, { flush: 'post' });

  watch(
    [
      query,
      targetHeadings,
      targetBody,
      targetCode,
      targetFrontMatter,
      withDeleted,
      latestOnly,
    ],
    () => {
      scheduleSearch();
    },
  );

  if (getCurrentScope()) {
    onScopeDispose(() => {
      requestId += 1;
      if (debounceTimer !== null) window.clearTimeout(debounceTimer);
    });
  }

  return {
    query,
    targetHeadings,
    targetBody,
    targetCode,
    targetFrontMatter,
    withDeleted,
    latestOnly,
    results,
    pathFilter,
    filteredResults,
    isLoading,
    errorMessage,
  };
}
