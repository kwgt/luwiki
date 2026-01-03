# Vue3 コンポーネントに関する規約

本プロジェクトの Vue コンポーネントは、Composition API を前提とします。  
Options API は原則として使用しません。

## コンポーネント構成

- `<script setup lang="ts">` を使用する
- `<script>` ブロックは 1 ファイルにつき 1 つとする
- `<style>` は scoped を原則とする

```vue
<script setup lang="ts">
</script>

<template>
</template>

<style scoped>
</style>
```

---

## ロジックの分離

- UI 表示とビジネスロジックを可能な限り分離する
- 状態管理やデータ取得ロジックは composable（useXxx）として切り出す
- コンポーネント内に肥大化した処理を書かないこと

---

## 命名規則（Vue 固有）

- コンポーネント名: PascalCase
- composable: useXxx
- props / emits: camelCase
- template 側の props 名は kebab-case

---

## Props / Emits 

- props は必ず型定義を行う
- emits は明示的に定義する
- props を直接書き換えないこと

```ts
const props = defineProps<{
  id: string;
}>();

const emit = defineEmits<{
  (e: 'update'): void;
}>();
```

---

## リアクティブデータ

- プリミティブ値は ref を使用する
- オブジェクト全体を扱う場合は reactive を使用する
- 不要に両者を混在させないこと

--- 

## computed / watch

- 派生値は computed を使用する
- 副作用を伴う処理は watch を使用する
- watch の多用は避け、設計を見直すこと

