<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { usePageList } from '../composables/usePageList';
import { useUiSettings } from '../composables/useUiSettings';
import { normalizeWikiPath } from '../lib/pageCommon';

const { selectedTheme } = useUiSettings();

const {
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
} = usePageList();

const pageTitle = computed(() => pagePath.value || '/');
const pageIndexLabel = computed(() => `ページ ${currentIndex.value + 1}`);

const breadcrumbItems = computed(() => {
  const currentPath = pagePath.value || '/';
  if (currentPath === '/') {
    return [{ label: '/', href: '/pages/' }];
  }

  const trimmed = currentPath.replace(/^\/+|\/+$/g, '');
  if (!trimmed) {
    return [{ label: '/', href: '/pages/' }];
  }

  const segments = trimmed.split('/');
  const items = [{ label: '/', href: '/pages/' }];
  let acc: string[] = [];
  for (const segment of segments) {
    acc.push(segment);
    const encoded = acc.map((part) => encodeURIComponent(part)).join('/');
    items.push({ label: segment, href: `/pages/${encoded}` });
  }
  return items;
});

function buildWikiUrl(path: string): string {
  const normalized = normalizeWikiPath(path);
  return normalized === '/' ? '/wiki/' : `/wiki${normalized}`;
}

function formatListTimestamp(raw: string): string {
  if (!raw) {
    return '';
  }
  const parts = raw.split('T');
  if (parts.length !== 2) {
    return raw;
  }
  const datePart = parts[0]?.replace(/-/g, '/') ?? raw;
  const timePart = parts[1] ?? '';
  return `${datePart} ${timePart}`;
}

onMounted(() => {
  void loadPageList(true);
});
</script>

<template>
  <div class="min-h-screen bg-base-200 text-base-content" :data-theme="selectedTheme">
    <div class="mx-auto flex max-w-6xl flex-col gap-1 px-4 pt-8 pb-[0.25rem] lg:px-10">
      <header class="flex flex-col gap-1">
        <div>
          <p class="text-xs font-semibold uppercase tracking-[0.32em] text-base-content/60">
            LUWIKI PAGES
          </p>
          <h1
            class="text-3xl font-bold leading-tight empty:min-h-[2.5rem] sm:text-4xl mt-3 mb-2 truncate"
            :title="pageTitle"
          >
            {{ pageTitle }} 以下のページ一覧
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
            :disabled="!canGoPrev"
            @click="goPrev"
          >
            前のページへ
          </button>
          <button
            class="btn btn-link btn-sm text-info"
            type="button"
            :disabled="!canGoNext"
            @click="goNext"
          >
            次のページへ
          </button>
          <span class="ml-auto text-xs text-base-content/60">
            {{ pageIndexLabel }}
          </span>
        </nav>
      </header>

      <div class="flex items-center gap-2">
        <label class="flex items-center gap-1 text-xs ml-auto text-base-content/70">
          <input v-model="withDeleted" class="checkbox checkbox-xs" type="checkbox" />
          <span>削除済みページも表示する</span>
        </label>
      </div>

      <main class="grid min-h-[calc(100vh-11.2em)] lg:min-h-[calc(100vh-12.8em)] gap-1">
        <section class="border border-base-300 bg-base-100 p-3 shadow-sm">
          <div v-if="isLoading" class="text-sm text-base-content/60">読み込み中...</div>
          <div v-else-if="errorMessage" class="text-sm text-error">{{ errorMessage }}</div>
          <div v-else-if="items.length === 0" class="text-sm text-base-content/60">
            ページがありません。
          </div>
          <div v-else class="flex flex-col">
            <div
              v-for="item in items"
              :key="item.page_id"
              class="flex items-center px-2 hover:bg-base-200/70 transition-colors"
            >
              <div class="flex min-w-0 items-center gap-2">
                <template v-if="item.deleted">
                  <button
                    class="link link-hover text-error truncate"
                    type="button"
                    :title="item.path"
                    @click="openRestore(item)"
                  >
                    {{ item.path }}
                  </button>
                  <span class="badge badge-error font-bold text-white badge-xs mt-1">Deleted</span>
                </template>
                <template v-else>
                  <a
                    class="link link-hover text-info truncate"
                    :href="buildWikiUrl(item.path)"
                    :title="item.path"
                  >
                    {{ item.path }}
                  </a>
                </template>
              </div>
              <div class="ml-auto flex items-center gap-4 text-sm text-base-content/60">
                <span class="w-[19ch] shrink-0 text-right ml-1 font-mono">
                  {{ formatListTimestamp(item.last_update.timestamp) }}
                </span>
                <span class="lg:w-[12ch] md:w-[6ch] sm:w-[4ch] shrink-0 text-right ml-1 truncate">
                  {{ item.last_update.username }}
                </span>
              </div>
            </div>
          </div>
        </section>
      </main>
    </div>

    <div v-if="restoreOpen" class="modal modal-open">
      <div class="modal-box space-y-4">
        <h3 class="text-lg font-bold">ページ復元</h3>
        <div class="space-y-3 text-sm">
          <label class="form-control w-full">
            <div class="label">
              <span class="label-text">復元先パス</span>
            </div>
            <input
              v-model="restoreTarget"
              class="input input-bordered w-full font-mono"
              type="text"
              placeholder="/restore/path"
            />
          </label>

          <p v-if="restoreError" class="text-sm text-error">
            {{ restoreError }}
          </p>
          <p v-else-if="restoreInputError" class="text-sm text-error">
            {{ restoreInputError }}
          </p>
          <p v-else class="text-sm min-h-[1.25rem]">
          </p>

          <div class="rounded border border-base-300 bg-base-200/60 p-2 text-xs">
            <div class="text-base-content/60">ページソースプレビュー</div>
            <div v-if="restoreSourceLoading" class="mt-2 text-base-content/70">
              読み込み中...
            </div>
            <textarea
              v-else
              class="textarea textarea-bordered mt-2 w-full font-mono text-xs h-40"
              :value="restoreSource || '（プレビューなし）'"
              readonly
            />
          </div>

          <label class="flex items-center gap-2 text-sm text-base-content/70">
            <input
              v-model="restoreRecursive"
              class="checkbox checkbox-xs"
              type="checkbox"
              :disabled="restoreInProgress || restoreSourceLoading"
            />
            <span>子ページも復元（再帰）</span>
          </label>
        </div>
        <div class="modal-action">
          <button class="btn" type="button" @click="closeRestore">
            キャンセル
          </button>
          <button
            class="btn btn-primary"
            type="button"
            :disabled="restoreInProgress || !!restoreInputError"
            @click="confirmRestore"
          >
            復元
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
