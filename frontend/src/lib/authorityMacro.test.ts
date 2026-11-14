import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { expandAuthorityMacro } from './authorityMacro';
import { getMcpAuthority } from './pageMeta';
import { createMarkdownRenderer } from './pageCommon';
import {
  collectImmediateMacroChanges,
  expandRenderMacros,
  resolveTemplateImmediateMacros,
} from './macroEngine';

interface Fixture {
  name: string;
  source: string;
  authority: string | null;
  expected: string;
}

test('authority macroはRustと同じfixture結果を返す', () => {
  const fixtures: Fixture[] = JSON.parse(readFileSync(path.resolve(
    process.cwd(), '../tests/fixtures/authority_macro_cases.json',
  ), 'utf8'));
  for (const fixture of fixtures) {
    assert.equal(expandAuthorityMacro(fixture.source, fixture.authority),
      fixture.expected, fixture.name);
  }
});

test('authorityは描画時だけ展開しtemplate専用形とソースを保持する', async () => {
  const source = '`{{mcp:authority}}`\n{{!mcp:authority}}';
  const context = { pagePath: '/test', mcpAuthority: 'docs.example.test' };
  assert.deepEqual(collectImmediateMacroChanges(source, context), []);
  for (const flag of [true, false]) {
    assert.equal(resolveTemplateImmediateMacros(source, context, flag), source);
  }
  assert.equal(await expandRenderMacros(source, context),
    '`docs.example.test`\n{{!mcp:authority}}');
  assert.equal(source, '`{{mcp:authority}}`\n{{!mcp:authority}}');
  assert.equal(await expandRenderMacros(source, { pagePath: '/test' }), source);
});

test('表示とpreviewの描画HTMLはリンクとcodeで同じauthorityを使う', async () => {
  const source = '[manual](luwiki://{{mcp:authority}}/pages/manual)\n\n'
    + '`{{mcp:authority}}`\n\n```text\n{{mcp:authority}}\n```';
  const expanded = await expandRenderMacros(source, {
    pagePath: '/test', mcpAuthority: 'docs.example.test',
  });
  const html = createMarkdownRenderer('/test').render(expanded);
  assert.ok(html.includes('href="luwiki://docs.example.test/pages/manual"'));
  assert.ok(html.includes('<code>docs.example.test</code>'));
  assert.ok(html.includes('docs.example.test\n</code>'));
  assert.ok(source.includes('{{mcp:authority}}'));
});

test('Sandbox案内は正式記法をliteral表示し、例のURIだけを展開する', async () => {
  const sandbox = readFileSync(path.resolve(
    process.cwd(), '../assets/default_sandbox.md',
  ), 'utf8');
  const section = sandbox.split('##### mcp:authority')[1]
    .split('##### children')[0];
  const expanded = await expandRenderMacros(section, {
    pagePath: '/Sandbox', mcpAuthority: 'docs.example.test',
  });
  const html = createMarkdownRenderer('/Sandbox').render(expanded);
  assert.ok(html.includes('正式な記述（そのまま入力）: {{mcp:authority}}'));
  assert.ok(html.includes('luwiki://docs.example.test/pages/manual'));
});

test('HTML meta authorityはrunと同じhostname制約で検証する', () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'document');
  let value: string | null = null;
  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    value: { querySelector: () => value === null ? null : {
      getAttribute: () => value,
    } },
  });
  try {
    for (value of ['local.luwiki', 'Docs.Example-Test', 'a'.repeat(63)]) {
      assert.equal(getMcpAuthority(), value);
    }
    for (value of [null, '', ' a.test', 'a.test ', 'a.test\n', 'a..test',
      '-a.test', 'a-.test', 'a_1.test', 'https://a.test', 'a/b', 'あ.test',
      'a'.repeat(64), Array(5).fill('a'.repeat(63)).join('.')]) {
      assert.equal(getMcpAuthority(), null, String(value));
    }
  } finally {
    if (previous) {
      Object.defineProperty(globalThis, 'document', previous);
    } else {
      Reflect.deleteProperty(globalThis, 'document');
    }
  }
});
