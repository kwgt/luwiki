import test from 'node:test';
import assert from 'node:assert/strict';

import { nextTick } from 'vue';

import { usePageSearch } from './usePageSearch';

const windowMock = {
  setTimeout: () => 0,
  clearTimeout: () => {},
};

Object.defineProperty(globalThis, 'window', {
  value: windowMock,
  configurable: true,
});

test('usePageSearch は全対象を解除しても body 既定へ戻す', async () => {
  const {
    targetHeadings,
    targetBody,
    targetCode,
    targetFrontMatter,
  } = usePageSearch();

  assert.equal(targetBody.value, true);

  targetHeadings.value = true;
  targetBody.value = false;
  targetCode.value = false;
  targetFrontMatter.value = false;
  await nextTick();

  assert.equal(targetHeadings.value, true);
  assert.equal(targetBody.value, false);

  targetHeadings.value = false;
  await nextTick();

  assert.equal(targetBody.value, true);
  assert.equal(targetCode.value, false);
  assert.equal(targetFrontMatter.value, false);
});

test('usePageSearch は front_matter を他対象と独立して保持できる', async () => {
  const {
    targetHeadings,
    targetBody,
    targetCode,
    targetFrontMatter,
  } = usePageSearch();

  targetHeadings.value = true;
  targetBody.value = true;
  targetCode.value = true;
  targetFrontMatter.value = true;
  await nextTick();

  assert.equal(targetHeadings.value, true);
  assert.equal(targetBody.value, true);
  assert.equal(targetCode.value, true);
  assert.equal(targetFrontMatter.value, true);
});

test('path filterは表示pathのliteral部分一致で元配列を保持する', () => {
  const state = usePageSearch();
  const row = (path: string, revision = 1, deleted = false) => ({
    page_id: 'id', revision, score: 1, path, deleted, text: 'snippet',
  });
  state.results.value = [
    row('/docs'), row('/docs/a', 1), row('/docs/a', 2, true),
    row('/docs-old'), row('/Docs'), row('/日本語/a.*'),
  ];
  for (const [filter, count] of [['', 6], ['  /docs  ', 4], ['/docs/', 2],
    ['/Docs', 1], ['日本語', 1], ['.*', 1], ['削除済み', 0]] as const) {
    state.pathFilter.value = filter;
    assert.equal(state.filteredResults.value.length, count, filter);
    assert.equal(state.results.value.length, 6);
  }
  state.pathFilter.value = '';
  assert.equal(state.filteredResults.value.length, 6);
});
