<script setup lang="ts">
import { computed } from 'vue';
import ViewPage from './components/ViewPage.vue';
import EditPage from './components/EditPage.vue';
import SearchPage from './components/SearchPage.vue';
import RevisionPage from './components/RevisionPage.vue';
import PageListPage from './components/PageListPage.vue';

const isEditPage = computed(() => {
  const path = window.location.pathname;
  return path === '/edit' || path.startsWith('/edit/');
});
const isSearchPage = computed(() => {
  const path = window.location.pathname;
  return path === '/search' || path.startsWith('/search/');
});
const isRevisionPage = computed(() => {
  const path = window.location.pathname;
  return path === '/rev' || path.startsWith('/rev/');
});
const isPageListPage = computed(() => {
  const path = window.location.pathname;
  return path === '/pages' || path.startsWith('/pages/');
});
</script>

<template>
  <SearchPage v-if="isSearchPage" />
  <EditPage v-else-if="isEditPage" />
  <RevisionPage v-else-if="isRevisionPage" />
  <PageListPage v-else-if="isPageListPage" />
  <ViewPage v-else />
</template>
