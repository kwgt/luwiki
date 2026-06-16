import test from 'node:test';
import assert from 'node:assert/strict';

import type { TemplatePageItem } from '../api/pages';
import {
  resolveSelectedTemplateId,
  sortTemplateItems,
} from './templateSelection';

function createItem(
  page_id: string,
  name: string,
): TemplatePageItem {
  return {
    page_id,
    name,
    description: null,
    macro_expand: null,
  };
}

test('sortTemplateItems は name 昇順かつ page_id 補助キーで安定化する', () => {
  const items = [
    createItem('page-3', '議事録10'),
    createItem('page-2', '議事録2'),
    createItem('page-1', '議事録2'),
  ];

  assert.deepEqual(
    sortTemplateItems(items).map((item) => item.page_id),
    ['page-1', 'page-2', 'page-3'],
  );
});

test('resolveSelectedTemplateId は既存選択が残っていれば維持する', () => {
  const items = [
    createItem('page-1', 'A'),
    createItem('page-2', 'B'),
  ];

  assert.equal(resolveSelectedTemplateId('page-2', items), 'page-2');
});

test('resolveSelectedTemplateId は選択が無効なとき表示順先頭へフォールバックする', () => {
  const items = [
    createItem('page-3', '議事録B'),
    createItem('page-1', '議事録A'),
  ];

  assert.equal(resolveSelectedTemplateId('missing', items), 'page-1');
});

test('resolveSelectedTemplateId は候補が空なら空文字を返す', () => {
  assert.equal(resolveSelectedTemplateId('page-1', []), '');
});
