import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveFrontMatterErrorMessage } from './frontMatterErrorMessage';

test('front matter validation で top-level 不正時は $ をそのまま表示しない', () => {
  assert.equal(
    resolveFrontMatterErrorMessage({
      type: 'validation',
      property_path: '$',
      message: 'front matter top-level must be object',
    }),
    'front matter のトップレベルは object である必要があります',
  );
});

test('front matter validation で具体的な property_path は補助情報として扱う', () => {
  assert.equal(
    resolveFrontMatterErrorMessage({
      type: 'validation',
      property_path: 'mcp.arguments',
      message: 'mcp.arguments is not allowed for resource primitive',
    }),
    'front matter の mcp.primitive が resource の場合、mcp.arguments は指定できません',
  );
});

test('front matter validation で未知メッセージでも詳細を維持する', () => {
  assert.equal(
    resolveFrontMatterErrorMessage({
      type: 'validation',
      property_path: 'wiki.tags[0]',
      message: 'custom validation failed',
    }),
    'front matter の項目が不正です: custom validation failed (対象: wiki.tags[0])',
  );
});

test('front matter validation で message がない場合は property_path を使う', () => {
  assert.equal(
    resolveFrontMatterErrorMessage({
      type: 'validation',
      property_path: 'wiki.tags[0]',
    }),
    'front matter の項目が不正です: wiki.tags[0] を確認してください',
  );
});

test('front matter validation で custom_meta 系未知メッセージでも詳細を維持する', () => {
  assert.equal(
    resolveFrontMatterErrorMessage({
      type: 'validation',
      property_path: 'custom_meta',
      message: 'custom_meta must be object',
    }),
    'front matter の custom_meta は object である必要があります',
  );
});

test('front matter syntax は既存の行列表示を維持する', () => {
  assert.equal(
    resolveFrontMatterErrorMessage({
      type: 'syntax',
      line: 3,
      column: 7,
      message: 'did not find expected node content',
    }),
    'front matter の構文エラーです: 3行目 7列目を確認してください',
  );
});
