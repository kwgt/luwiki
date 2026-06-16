import test from 'node:test';
import assert from 'node:assert/strict';

import { CompletionContext, type Completion, type CompletionResult } from '@codemirror/autocomplete';
import { EditorState } from '@codemirror/state';
import {
  IndentContext,
  ensureSyntaxTree,
  foldEffect,
  foldable,
  foldedRanges,
  getIndentation,
  syntaxTree,
} from '@codemirror/language';
import { markdown } from '@codemirror/lang-markdown';
import { yamlFrontmatter } from '@codemirror/lang-yaml';

import { buildBaseExtensions } from './editorExtensions';
import {
  getFrontMatterFoldRange,
  getInitialFrontMatterFoldRange,
} from './frontMatterFold';
import {
  getFrontMatterIndentation,
  indentFrontMatterLess,
  indentFrontMatterMore,
} from './frontMatterIndent';
import { mermaidCompletionSource } from './mermaidCompletion';
import { wikiLinkExtension } from './markdown/wikiLinkExtension';

function createMarkdownState(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: [
      yamlFrontmatter({
        content: markdown({
          extensions: [wikiLinkExtension],
        }),
      }),
    ],
  });
}

async function resolveCompletionResult(
  state: EditorState,
  pos: number,
): Promise<CompletionResult | null> {
  return await mermaidCompletionSource(new CompletionContext(state, pos, true));
}

function createEditorState(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: buildBaseExtensions(),
  });
}

function getFoldRange(state: EditorState, lineNumber: number): {
  from: number;
  to: number;
} | null {
  const line = state.doc.line(lineNumber);
  return foldable(state, line.from, line.to);
}

function applyInitialFrontMatterFold(state: EditorState): EditorState {
  const range = getInitialFrontMatterFoldRange(state);
  if (!range) {
    return state;
  }
  return state.update({
    effects: foldEffect.of(range),
  }).state;
}

function collectFoldedRanges(state: EditorState): Array<{
  from: number;
  to: number;
}> {
  const ranges: Array<{ from: number; to: number }> = [];
  foldedRanges(state).between(0, state.doc.length, (from, to) => {
    ranges.push({ from, to });
  });
  return ranges;
}

function applyFrontMatterIndentMore(state: EditorState): { applied: boolean; state: EditorState } {
  let nextState = state;
  const applied = indentFrontMatterMore({
    state,
    dispatch: (transaction) => {
      nextState = transaction.state;
    },
  });

  return { applied, state: nextState };
}

function applyFrontMatterIndentLess(state: EditorState): { applied: boolean; state: EditorState } {
  let nextState = state;
  const applied = indentFrontMatterLess({
    state,
    dispatch: (transaction) => {
      nextState = transaction.state;
    },
  });

  return { applied, state: nextState };
}

test('front matter 付きでも本文見出しは Markdown として解析される', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
    '',
    '本文',
  ].join('\n');
  const state = createMarkdownState(doc);
  const ensured = ensureSyntaxTree(state, state.doc.length, 1_000);
  const tree = (ensured ?? syntaxTree(state)).toString();

  assert.match(tree, /ATXHeading1/);
});

test('front matter 付きでも本文 wiki link は既存拡張で解析される', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '[[リンク先|表示名]]',
  ].join('\n');
  const state = createMarkdownState(doc);
  const ensured = ensureSyntaxTree(state, state.doc.length, 1_000);
  const tree = (ensured ?? syntaxTree(state)).toString();

  assert.match(tree, /WikiLink/);
});

test('front matter 付きでも mermaid フェンス内補完は維持される', async () => {
  const doc = [
    '---',
    'mcp:',
    '  primitive: resource',
    '  name: Example',
    '  description: desc',
    '---',
    '```mermaid',
    'flo',
    '```',
  ].join('\n');
  const state = createMarkdownState(doc);
  const pos = doc.indexOf('flo') + 3;
  const result = await resolveCompletionResult(state, pos);

  assert.ok(result);
  assert.ok(result.options.some((option: Completion) => option.label === 'flowchart'));
  assert.ok(result.options.some((option: Completion) => option.label === 'graph'));
});

test('front matter の開始区切り行から全体を折りたためる', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
  ].join('\n');
  const state = createEditorState(doc);
  const range = getFoldRange(state, 1);

  assert.deepEqual(range, {
    from: state.doc.line(1).to,
    to: state.doc.line(5).to,
  });
});

test('front matter の折りたたみ範囲に本文を含めない', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
    '',
    '本文',
  ].join('\n');
  const state = createEditorState(doc);
  const range = getFoldRange(state, 1);

  assert.ok(range);
  assert.equal(state.doc.sliceString(range.from, range.to), [
    '',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
  ].join('\n'));
  assert.equal(range.to, state.doc.line(5).to);
});

test('front matter がない場合は独自折りたたみ範囲を返さない', () => {
  const doc = [
    '# 本文タイトル',
    '',
    '本文',
  ].join('\n');
  const state = createEditorState(doc);
  const firstLine = state.doc.line(1);

  assert.equal(
    getFrontMatterFoldRange(state, firstLine.from, firstLine.to),
    null,
  );
});

test('ソース途中の区切りを front matter として折りたたまない', () => {
  const doc = [
    '本文',
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
  ].join('\n');
  const state = createEditorState(doc);

  assert.equal(getFoldRange(state, 2), null);
});

test('終了区切りがない front matter は折りたたまない', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
  ].join('\n');
  const state = createEditorState(doc);
  const firstLine = state.doc.line(1);

  assert.equal(
    getFrontMatterFoldRange(state, firstLine.from, firstLine.to),
    null,
  );
});

test('front matter 付きでも Markdown 見出しを標準規則で折りたためる', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
    '',
    '本文',
    '## 子見出し',
    '',
    '子本文',
    '# 次の見出し',
  ].join('\n');
  const state = createEditorState(doc);
  const range = getFoldRange(state, 6);

  assert.ok(range);
  assert.equal(range.from, state.doc.line(6).to);
  assert.equal(range.to, state.doc.line(11).to);
});

test('front matter がない Markdown 見出しも標準規則で折りたためる', () => {
  const doc = [
    '# 本文タイトル',
    '',
    '本文',
    '# 次の見出し',
  ].join('\n');
  const state = createEditorState(doc);
  const range = getFoldRange(state, 1);

  assert.ok(range);
  assert.equal(range.from, state.doc.line(1).to);
  assert.equal(range.to, state.doc.line(3).to);
});

test('front matter 内部の YAML 標準折りたたみを維持する', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
  ].join('\n');
  const state = createEditorState(doc);
  const range = getFoldRange(state, 2);

  assert.ok(range);
  assert.equal(range.from, state.doc.line(2).to);
  assert.equal(range.to, state.doc.line(4).to);
});

test('初期表示では front matter 全体だけを折りたたむ', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
    '',
    '本文',
  ].join('\n');
  const state = createEditorState(doc);
  const foldedState = applyInitialFrontMatterFold(state);

  assert.deepEqual(collectFoldedRanges(foldedState), [{
    from: state.doc.line(1).to,
    to: state.doc.line(5).to,
  }]);
});

test('front matter がない初期表示では折りたたまない', () => {
  const doc = [
    '# 本文タイトル',
    '',
    '本文',
  ].join('\n');
  const state = createEditorState(doc);
  const foldedState = applyInitialFrontMatterFold(state);

  assert.equal(foldedState, state);
  assert.deepEqual(collectFoldedRanges(foldedState), []);
});

test('初期表示では Markdown 見出しと YAML 内部を折りたたまない', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
    '',
    '本文',
  ].join('\n');
  const state = createEditorState(doc);
  const foldedState = applyInitialFrontMatterFold(state);
  const ranges = collectFoldedRanges(foldedState);

  assert.equal(ranges.length, 1);
  assert.deepEqual(ranges[0], {
    from: state.doc.line(1).to,
    to: state.doc.line(5).to,
  });
});

test('front matter 範囲では 2 文字インデントを返す', () => {
  const doc = [
    '---',
    'wiki:',
    '  template:',
    '    name: Sample',
    '---',
    '# 本文タイトル',
  ].join('\n');
  const state = createEditorState(doc);

  assert.equal(getIndentation(state, state.doc.line(2).from), 0);
  assert.equal(getIndentation(state, state.doc.line(3).from), 2);
  assert.equal(getIndentation(state, state.doc.line(4).from), 4);
});

test('front matter 終了後は front matter 専用インデントが本文を上書きしない', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '- item',
    '    nested',
  ].join('\n');
  const state = createEditorState(doc);

  assert.equal(getFrontMatterIndentation(state, state.doc.line(6).from), undefined);
  assert.equal(getFrontMatterIndentation(state, state.doc.line(7).from), undefined);
});

test('front matter がない本文でも front matter 専用インデントは介入しない', () => {
  const doc = [
    '- item',
    '    nested',
  ].join('\n');
  const state = createEditorState(doc);

  assert.equal(getFrontMatterIndentation(state, state.doc.line(1).from), undefined);
  assert.equal(getFrontMatterIndentation(state, state.doc.line(2).from), undefined);
});

test('front matter 終了後の本文空行でも front matter 専用インデントは介入しない', () => {
  const doc = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '- item',
    '',
  ].join('\n');
  const state = createEditorState(doc);

  assert.equal(getFrontMatterIndentation(state, state.doc.line(7).from), undefined);
});

test('front matter の key 行で Enter すると 1 段深い 2 文字インデントへ入る', () => {
  const doc = [
    '---',
    'wiki:',
    '---',
  ].join('\n');
  const state = createEditorState(doc);
  const pos = doc.indexOf('wiki:') + 'wiki:'.length;
  const context = new IndentContext(state, { simulateBreak: pos });

  assert.equal(getIndentation(context, pos), 2);
});

test('front matter の配列項目で Enter すると同じ配列レベルを維持する', () => {
  const doc = [
    '---',
    'tags:',
    '  - rust',
    '---',
  ].join('\n');
  const state = createEditorState(doc);
  const pos = doc.indexOf('  - rust') + '  - rust'.length;
  const context = new IndentContext(state, { simulateBreak: pos });

  assert.equal(getIndentation(context, pos), 2);
});

test('front matter のインライン object 配列項目で Enter すると 1 段深い位置へ入る', () => {
  const doc = [
    '---',
    'items:',
    '  - name: rust',
    '---',
  ].join('\n');
  const state = createEditorState(doc);
  const pos = doc.indexOf('  - name: rust') + '  - name: rust'.length;
  const context = new IndentContext(state, { simulateBreak: pos });

  assert.equal(getIndentation(context, pos), 4);
});

test('front matter の closing delimiter で Enter しても余計なインデントを入れない', () => {
  const doc = [
    '---',
    'wiki:',
    '---',
  ].join('\n');
  const state = createEditorState(doc);
  const pos = doc.lastIndexOf('---') + 3;
  const context = new IndentContext(state, { simulateBreak: pos });

  assert.equal(getIndentation(context, pos), 0);
});

test('front matter のネスト配下空行では直前の YAML 構造に沿って継続入力できる', () => {
  const doc = [
    '---',
    'wiki:',
    '  template:',
    '    name: Sample',
    '',
    '---',
  ].join('\n');
  const state = createEditorState(doc);

  assert.equal(getIndentation(state, state.doc.line(5).from), 4);
});

test('custom_meta 配下でも front matter の 2 文字インデント規則を維持する', () => {
  const doc = [
    '---',
    'custom_meta:',
    '  project:',
    '    name: alpha',
    '---',
  ].join('\n');
  const state = createEditorState(doc);
  const pos = doc.indexOf('  project:') + '  project:'.length;
  const context = new IndentContext(state, { simulateBreak: pos });

  assert.equal(getIndentation(state, state.doc.line(3).from), 2);
  assert.equal(getIndentation(state, state.doc.line(4).from), 4);
  assert.equal(getIndentation(context, pos), 4);
});

test('front matter 行で Tab すると 2 文字だけ深くなる', () => {
  const doc = [
    '---',
    'wiki:',
    '  template:',
    '---',
  ].join('\n');
  const state = createEditorState(doc).update({
    selection: { anchor: doc.indexOf('  template:') + 2 },
  }).state;

  const result = applyFrontMatterIndentMore(state);

  assert.equal(result.applied, true);
  assert.equal(result.state.doc.toString(), ['---', 'wiki:', '    template:', '---'].join('\n'));
});

test('front matter の空行先頭で Tab すると 2 文字インデントする', () => {
  const doc = [
    '---',
    'wiki:',
    '',
    '---',
  ].join('\n');
  const state = createEditorState(doc).update({
    selection: { anchor: '---\nwiki:\n'.length },
  }).state;

  const result = applyFrontMatterIndentMore(state);

  assert.equal(result.applied, true);
  assert.equal(result.state.doc.toString(), ['---', 'wiki:', '  ', '---'].join('\n'));
});

test('front matter 行で Shift-Tab すると 2 文字だけ浅くなる', () => {
  const doc = [
    '---',
    'wiki:',
    '    template:',
    '---',
  ].join('\n');
  const state = createEditorState(doc).update({
    selection: { anchor: doc.indexOf('    template:') + 4 },
  }).state;

  const result = applyFrontMatterIndentLess(state);

  assert.equal(result.applied, true);
  assert.equal(result.state.doc.toString(), ['---', 'wiki:', '  template:', '---'].join('\n'));
});

test('front matter 行で Shift-Tab は 1 レベルずつ戻る', () => {
  const doc = [
    '---',
    'wiki:',
    '      template:',
    '---',
  ].join('\n');
  const state = createEditorState(doc).update({
    selection: { anchor: doc.indexOf('      template:') + 6 },
  }).state;

  const firstResult = applyFrontMatterIndentLess(state);
  assert.equal(firstResult.applied, true);
  assert.equal(firstResult.state.doc.toString(), ['---', 'wiki:', '    template:', '---'].join('\n'));

  const secondResult = applyFrontMatterIndentLess(firstResult.state);
  assert.equal(secondResult.applied, true);
  assert.equal(secondResult.state.doc.toString(), ['---', 'wiki:', '  template:', '---'].join('\n'));
});

test('key 行の Enter 後にできた空行でも Tab で 2 文字インデントする', () => {
  const doc = [
    '---',
    'aaaaa:',
    '  ',
    '---',
  ].join('\n');
  const state = createEditorState(doc).update({
    selection: { anchor: '---\naaaaa:\n  '.length },
  }).state;

  const tabbed = applyFrontMatterIndentMore(state);
  assert.equal(tabbed.applied, true);
  assert.equal(tabbed.state.doc.toString(), ['---', 'aaaaa:', '    ', '---'].join('\n'));
});

test('front matter 外では Tab インデントハンドラが既存挙動へ委譲する', () => {
  const doc = [
    '---',
    'wiki:',
    '---',
    '- item',
  ].join('\n');
  const state = createEditorState(doc).update({
    selection: { anchor: doc.length },
  }).state;

  const result = applyFrontMatterIndentMore(state);

  assert.equal(result.applied, false);
  assert.equal(result.state, state);
});
