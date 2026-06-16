import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveTemplateImmediateMacros } from './macroEngine';

test('resolveTemplateImmediateMacros は macro_expand 無効時に専用プレースホルダをそのまま保持する', () => {
  const source = [
    '# 議事録',
    '',
    '- 作成日: {{!today:iso}}',
    '- ページ: {{!page:basename}}',
  ].join('\n');

  assert.equal(
    resolveTemplateImmediateMacros(source, {
      pagePath: '/templates/minutes',
      pageId: 'page-1',
    }, false),
    source,
  );
});

test('resolveTemplateImmediateMacros は macro_expand 有効時に即時変換型プレースホルダを展開する', () => {
  const source = [
    '# 議事録',
    '',
    '- ページ: {{!page:basename}}',
    '- 作成者: {{!user:display}}',
  ].join('\n');

  assert.equal(
    resolveTemplateImmediateMacros(source, {
      pagePath: '/projects/monthly-report',
      pageId: 'page-2',
      userId: 'user-1',
      userDisplayName: 'Alice',
    }, true),
    [
      '# 議事録',
      '',
      '- ページ: monthly-report',
      '- 作成者: Alice',
    ].join('\n'),
  );
});

test('resolveTemplateImmediateMacros はコード領域内の専用プレースホルダを展開しない', () => {
  const source = [
    '# Sample',
    '',
    '```text',
    '{{!page:basename}}',
    '```',
    '',
    '`{{!user}}`',
  ].join('\n');

  assert.equal(
    resolveTemplateImmediateMacros(source, {
      pagePath: '/projects/monthly-report',
      pageId: 'page-2',
      userId: 'user-1',
    }, true),
    source,
  );
});
