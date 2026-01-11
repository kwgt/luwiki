<script setup lang="ts">
import { computed, ref } from 'vue';
import { usePageSearch } from '../composables/usePageSearch';
import { useUiSettings } from '../composables/useUiSettings';
import { normalizeWikiPath } from '../lib/pageCommon';

type SortOrder = 'page_id' | 'score' | 'path';

const {
  query,
  targetHeadings,
  targetBody,
  targetCode,
  withDeleted,
  latestOnly,
  results,
  isLoading,
  errorMessage,
} = usePageSearch();

const { selectedTheme } = useUiSettings();

const sortOrder = ref<SortOrder>('score');

const sortedResults = computed(() => {
  const items = [...results.value];
  switch (sortOrder.value) {
    case 'page_id':
      return items.sort((a, b) => a.page_id.localeCompare(b.page_id));
    case 'path':
      return items.sort((a, b) => a.path.localeCompare(b.path));
    case 'score':
    default:
      return items.sort((a, b) => b.score - a.score);
  }
});

function buildWikiUrl(path: string): string {
  const normalized = normalizeWikiPath(path);
  return normalized === '/' ? '/wiki/' : `/wiki${normalized}`;
}

function formatScore(score: number): string {
  return score.toFixed(2);
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function formatSnippet(raw: string): string {
  const escaped = escapeHtml(raw);
  return escaped
    .replace(/&lt;b&gt;/g, '<b>')
    .replace(/&lt;\/b&gt;/g, '</b>');
}
</script>

<template>
  <div class="min-h-screen bg-base-200 text-base-content" :data-theme="selectedTheme">
    <div class="mx-auto flex max-w-6xl flex-col gap-3 px-4 py-8 lg:px-10">
      <header class="flex flex-col gap-2">
        <p class="text-xs font-semibold uppercase tracking-[0.32em] text-base-content/60">
          LUWIKI SEARCH
        </p>
        <h1 class="text-3xl font-bold leading-tight sm:text-4xl">
          検索
        </h1>
      </header>

      <section class="border border-base-300 bg-base-100 p-4 shadow-sm">
        <label class="form-control w-full">
          <div class="text-sm font-semibold label">
            <span class="label-text">検索式</span>
          </div>
          <input
            v-model="query"
            class="input input-bordered w-full"
            type="text"
            placeholder="検索したい単語を入力"
          />
        </label>

        <div class="mt-3 grid gap-4 lg:grid-cols-2">
          <div class="space-y-1 text-sm">
            <div class="text-sm font-semibold">検索対象</div>
            <label class="flex items-center gap-1">
              <input v-model="targetHeadings" class="checkbox checkbox-xs" type="checkbox" />
              <span>見出しを検索する</span>
            </label>
            <label class="flex items-center gap-1">
              <input v-model="targetBody" class="checkbox checkbox-xs" type="checkbox" />
              <span>本文を検索する</span>
            </label>
            <label class="flex items-center gap-1">
              <input v-model="targetCode" class="checkbox checkbox-xs" type="checkbox" />
              <span>コードブロックを検索する</span>
            </label>
          </div>
          <div class="space-y-1 text-sm">
            <div class="text-sm font-semibold">オプション</div>
            <label class="flex items-center gap-1">
              <input v-model="withDeleted" class="checkbox checkbox-xs" type="checkbox" />
              <span>削除済みページを検索対象に含める</span>
            </label>
            <label class="flex items-center gap-1">
              <input v-model="latestOnly" class="checkbox checkbox-xs" type="checkbox" />
              <span>最新リビジョンのみを検索対象とする</span>
            </label>
          </div>
        </div>
      </section>

      <section class="border border-base-300 bg-base-100 p-4 shadow-sm">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="text-sm font-semibold">検索結果</div>
          <label class="form-control">
            <div class="label">
              <span class="label-text">ソート順</span>
            </div>
            <select v-model="sortOrder" class="select select-bordered select-sm">
              <option value="score">スコア順</option>
              <option value="page_id">ページID順</option>
              <option value="path">パス順</option>
            </select>
          </label>
        </div>

        <div class="mt-0">
          <div v-if="isLoading" class="text-sm text-base-content/70">
            検索中...
          </div>
          <div v-else-if="errorMessage" class="text-sm text-error">
            {{ errorMessage }}
          </div>
          <div v-else-if="!query.trim()" class="text-sm text-base-content/60">
            検索式を入力してください。
          </div>
          <div v-else-if="sortedResults.length === 0" class="text-sm text-base-content/60">
            検索結果がありません。
          </div>
          <div v-else class="text-sm empty:min-h-[1.25rem] text-base-content/60">
          </div>

          <div class="flex flex-col gap-4">
            <div v-for="item in sortedResults" :key="`${item.page_id}-${item.revision}`">
              <div
                class="grid gap-2 border border-base-300 bg-base-100 p-3 text-xs sm:grid-cols-[19em_60px_60px_minmax(0,1fr)]"
              >
                <div>
                  <div class="text-[0.65rem] font-semibold uppercase text-base-content/50">ID</div>
                  <div class="font-mono text-sm">{{ item.page_id }}</div>
                </div>
                <div>
                  <div class="text-[0.65rem] font-semibold uppercase text-base-content/50">REV</div>
                  <div class="text-sm">{{ item.revision }}</div>
                </div>
                <div>
                  <div class="text-[0.65rem] font-semibold uppercase text-base-content/50">SCORE</div>
                  <div class="text-sm">{{ formatScore(item.score) }}</div>
                </div>
                <div>
                  <div class="text-[0.65rem] font-semibold uppercase text-base-content/50">PATH</div>
                  <a class="link link-hover text-info text-sm break-all" :href="buildWikiUrl(item.path)">
                    {{ item.path }}
                    <span v-if="item.deleted" class="text-error"> (削除済み)</span>
                  </a>
                </div>
              </div>
              <div class="border border-base-300 border-t-0 bg-base-100 p-3 text-lg">
                <span
                  v-if="item.text"
                  class="whitespace-pre-wrap"
                  v-html="formatSnippet(item.text)"
                />
                <span v-else class="text-base-content/50">スニペットなし</span>
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>
