import test from 'node:test';
import assert from 'node:assert/strict';

import { CompletionContext, type Completion, type CompletionResult } from '@codemirror/autocomplete';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { yamlFrontmatter } from '@codemirror/lang-yaml';

import {
  debugMcpResourceBlockSnippet,
  debugFrontMatterCompletionTarget,
  frontMatterCompletionSource,
} from './frontMatterCompletion';

function createState(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: [yamlFrontmatter({ content: markdown() })],
  });
}

async function resolveCompletionResult(
  state: EditorState,
  pos: number,
): Promise<CompletionResult | null> {
  return await frontMatterCompletionSource(new CompletionContext(state, pos, true));
}

function labelsOf(result: CompletionResult): string[] {
  return result.options.map((option: Completion) => option.label);
}

test('front matter top-level では wiki と mcp と custom_meta を補完候補に含める', async () => {
  const doc = ['---', 'wi', '---', '# title'].join('\n');
  const state = createState(doc);
  const pos = doc.indexOf('wi') + 2;
  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);
  assert.ok(result.options.some((option: Completion) => option.label === 'wiki'));
  assert.ok(result.options.some((option: Completion) => option.label === 'mcp'));
  assert.ok(result.options.some((option: Completion) => option.label === 'custom_meta'));
});

test('mcp.primitive の値補完で prompt と resource を出す', async () => {
  const doc = ['---', 'mcp:', '  primitive: pr', '---', '# title'].join('\n');
  const state = createState(doc);
  const pos = doc.lastIndexOf('pr') + 2;
  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);
  assert.deepEqual(
    labelsOf(result).filter((label: string) => label === 'prompt' || label === 'resource'),
    ['prompt', 'resource'],
  );
});

test('mcp 配下では resource_path を補完候補に含める', async () => {
  const doc = ['---', 'mcp:', '  res', '---', '# title'].join('\n');
  const state = createState(doc);
  const pos = doc.lastIndexOf('res') + 3;
  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);
  assert.ok(result.options.some((option: Completion) => option.label === 'resource_path'));
});

test('mcp resource block スニペットは resource_path null と用途コメントを含める', () => {
  const snippet = debugMcpResourceBlockSnippet();
  assert.match(snippet, /resource_path: null/);
  assert.match(snippet, /resource_path は絶対 URI path/);
  assert.match(snippet, /null は \/pages\/<ページ path>/);
});

test('mcp.resource_path の detail は絶対 path と pages fallback を案内する', async () => {
  const doc = ['---', 'mcp:', '  res', '---', '# title'].join('\n');
  const state = createState(doc);
  const pos = doc.lastIndexOf('res') + 3;
  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);

  const resourcePathOption = result.options.find((option: Completion) => option.label === 'resource_path');
  assert.equal(resourcePathOption?.detail, 'absolute resource URI path; null uses /pages fallback');
});

test('mcp.resource_acl 配下では default list read を補完候補に含める', async () => {
  const doc = ['---', 'mcp:', '  resource_acl:', '    ', '---', '# title'].join('\n');
  const state = createState(doc);
  const pos = doc.indexOf('    ') + 4;
  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);
  assert.ok(result.options.some((option: Completion) => option.label === 'default'));
  assert.ok(result.options.some((option: Completion) => option.label === 'list'));
  assert.ok(result.options.some((option: Completion) => option.label === 'read'));
});

test('mcp.resource_acl.list 配下では allow deny を補完候補に含める', async () => {
  const doc = ['---', 'mcp:', '  resource_acl:', '    list:', '      ', '---', '# title'].join('\n');
  const state = createState(doc);
  const pos = doc.indexOf('      ') + 6;
  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);
  assert.ok(result.options.some((option: Completion) => option.label === 'allow'));
  assert.ok(result.options.some((option: Completion) => option.label === 'deny'));
});

test('mcp.resource_acl.default.list の値補完で true と false を出す', async () => {
  const doc = [
    '---',
    'mcp:',
    '  resource_acl:',
    '    default:',
    '      list: tr',
    '---',
    '# title',
  ].join('\n');
  const state = createState(doc);
  const pos = doc.lastIndexOf('tr') + 2;
  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);
  assert.deepEqual(
    labelsOf(result).filter((label: string) => label === 'true' || label === 'false'),
    ['true', 'false'],
  );
});

test('mcp.arguments 配下では argument key を補完対象として認識する', async () => {
  const doc = [
    '---',
    'mcp:',
    '  primitive: prompt',
    '  name: page summary',
    '  description: desc',
    '  arguments:',
    '    - na',
    '---',
    '# title',
  ].join('\n');
  const state = createState(doc);
  const pos = doc.lastIndexOf('na') + 2;
  const target = debugFrontMatterCompletionTarget(state, pos);
  assert.deepEqual(target, {
    type: 'key',
    from: doc.lastIndexOf('na'),
    to: pos,
    partial: 'na',
    parentPath: ['mcp', 'arguments', '[]'],
  });
  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);
  assert.ok(result.options.some((option: Completion) => option.label === 'name'));
  assert.ok(result.options.some((option: Completion) => option.label === 'description'));
  assert.ok(result.options.some((option: Completion) => option.label === 'required'));
});

test('front matter 外では補完しない', async () => {
  const doc = ['---', 'wiki:', '  tags:', '    - rust', '---', '# title', 'mc'].join('\n');
  const state = createState(doc);
  const pos = doc.lastIndexOf('mc') + 2;
  const result = await resolveCompletionResult(state, pos);
  assert.equal(result, null);
});

test('front matter がない Markdown では front matter 補完を出さない', async () => {
  const doc = ['# title', '', 'wiki'].join('\n');
  const state = createState(doc);
  const pos = doc.lastIndexOf('wiki') + 4;
  const result = await resolveCompletionResult(state, pos);
  assert.equal(result, null);
});

test('閉じ区切りが欠落した場合でも編集中の front matter 補完は維持する', async () => {
  const doc = ['---', 'wiki', '# title'].join('\n');
  const state = createState(doc);
  const pos = doc.indexOf('wiki') + 4;
  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);
  assert.ok(result.options.some((option: Completion) => option.label === 'wiki'));
  assert.ok(result.options.some((option: Completion) => option.label === 'mcp'));
  assert.ok(result.options.some((option: Completion) => option.label === 'custom_meta'));
});

test('インライン object 配列項目の次行でも同じ配列要素配下の key 補完を維持する', async () => {
  const doc = [
    '---',
    'mcp:',
    '  primitive: prompt',
    '  name: page summary',
    '  description: desc',
    '  arguments:',
    '    - name: topic',
    '      de',
    '---',
  ].join('\n');
  const state = createState(doc);
  const pos = doc.lastIndexOf('de') + 2;
  const target = debugFrontMatterCompletionTarget(state, pos);

  assert.deepEqual(target, {
    type: 'key',
    from: doc.lastIndexOf('de'),
    to: pos,
    partial: 'de',
    parentPath: ['mcp', 'arguments', '[]'],
  });

  const result = await resolveCompletionResult(state, pos);
  assert.ok(result);
  assert.ok(result.options.some((option: Completion) => option.label === 'description'));
  assert.ok(result.options.some((option: Completion) => option.label === 'required'));
});

test('ネスト配下の継続入力位置でも front matter 値補完を維持する', async () => {
  const doc = [
    '---',
    'wiki:',
    '  template:',
    '    macro_expand: tr',
    '---',
  ].join('\n');
  const state = createState(doc);
  const pos = doc.lastIndexOf('tr') + 2;
  const result = await resolveCompletionResult(state, pos);

  assert.ok(result);
  assert.deepEqual(
    labelsOf(result).filter((label: string) => label === 'true' || label === 'false'),
    ['true', 'false'],
  );
});
