import test from 'node:test';
import assert from 'node:assert/strict';
import { createMarkdownRenderer } from './pageCommon';
import { renderMarkdownWithCodeCopy } from './codeCopy';

test('fence copyは装飾HTMLではなく表示tokenを保存する', () => {
  const source = 'inline `x`\n\n    indent\n\n```unknown\r\n<& 日本語\r\n\r\n```\r\n'
    + '\n```mermaid\ngraph TD\n A-->B\n```\n\n```\n```\n';
  const md = createMarkdownRenderer('/test');
  const plain = md.render(source);
  assert.ok(!plain.includes('data-code-copy-id'));
  const first = renderMarkdownWithCodeCopy(md, source, 'view-1');
  assert.deepEqual([...first.codeBlocks.values()], [
    '<& 日本語\n\n', 'graph TD\n A-->B\n', '',
  ]);
  assert.equal(first.codeBlocks.size, 3);
  assert.ok(first.html.includes('&lt;&amp; 日本語'));
  assert.ok(first.html.includes('pre class="mermaid"'));
  assert.ok(first.html.includes('data-code-copy-id="view-1-0"'));
  const second = renderMarkdownWithCodeCopy(md, '```svg\n<svg/>\n```', 'view-2');
  assert.deepEqual([...second.codeBlocks.values()], ['<svg/>\n']);
  assert.ok(!second.codeBlocks.has('view-1-0'));
  assert.equal(first.codeBlocks.size, 3);
  assert.equal(md.render(source), plain);
});

test('renderごとのMapは本文をHTML属性に埋め込まない', () => {
  const source = '```text\n" onclick="secret-marker <tag>\n```';
  const rendered = renderMarkdownWithCodeCopy(createMarkdownRenderer('/x'), source, 'safe');
  assert.equal(rendered.codeBlocks.get('safe-0'), '" onclick="secret-marker <tag>\n');
  assert.ok(!rendered.html.includes('data-code-copy-id="secret-marker'));
  assert.ok(rendered.html.includes('role="status"'));
  assert.ok(rendered.html.includes('aria-label="コードをコピー"'));
  assert.ok(!rendered.html.includes('title="コードをコピー"'));
});
