import { computed, ref, watch } from 'vue';
import { searchPages, type SearchResult } from '../api/pages';
import { toErrorMessage } from '../lib/pageCommon';

type SearchTarget = 'headings' | 'body' | 'code';

const SEARCH_DEBOUNCE_MS = 300;

export function usePageSearch() {
  const query = ref('');
  const targetHeadings = ref(false);
  const targetBody = ref(true);
  const targetCode = ref(false);
  const withDeleted = ref(false);
  const latestOnly = ref(true);
  const results = ref<SearchResult[]>([]);
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
    return list;
  });

  let debounceTimer: number | null = null;
  let requestId = 0;

  function scheduleSearch(): void {
    if (debounceTimer !== null) {
      window.clearTimeout(debounceTimer);
    }
    debounceTimer = window.setTimeout(() => {
      void runSearch();
    }, SEARCH_DEBOUNCE_MS);
  }

  async function runSearch(): Promise<void> {
    const expr = query.value.trim();
    if (!expr) {
      requestId += 1;
      results.value = [];
      errorMessage.value = '';
      isLoading.value = false;
      return;
    }

    const current = requestId + 1;
    requestId = current;
    isLoading.value = true;
    errorMessage.value = '';

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

  watch([targetHeadings, targetBody, targetCode], () => {
    if (targets.value.length === 0) {
      targetBody.value = true;
    }
  });

  watch(
    [query, targetHeadings, targetBody, targetCode, withDeleted, latestOnly],
    () => {
      scheduleSearch();
    },
  );

  return {
    query,
    targetHeadings,
    targetBody,
    targetCode,
    withDeleted,
    latestOnly,
    results,
    isLoading,
    errorMessage,
  };
}
