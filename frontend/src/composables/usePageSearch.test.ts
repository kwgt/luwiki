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
