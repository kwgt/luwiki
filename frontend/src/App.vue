<script setup lang="ts">
import { computed } from 'vue';
import ViewPage from './components/ViewPage.vue';
import EditPage from './components/EditPage.vue';

const isEditPage = computed(() => {
  const path = window.location.pathname;
  return path === '/edit' || path.startsWith('/edit/');
});
</script>

<template>
  <EditPage v-if="isEditPage" />
  <ViewPage v-else />
</template>
