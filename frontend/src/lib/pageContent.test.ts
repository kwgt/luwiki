import test from 'node:test';
import assert from 'node:assert/strict';
import { normalizeTemplateBody, stripFrontMatter } from './pageContent';
import { resolveTemplateImmediateMacros } from './macroEngine';

const frontMatter = '---\nwiki:\n  template:\n    name: matrix\n---\n';

/** 合意済みの原文と適用後本文を直接比較する。 */
const cases: ReadonlyArray<readonly [string, string, string]> = [
  ['空行なし', frontMatter + '# Body\n', '# Body\n'],
  ['空行1行', frontMatter + '\n# Body\n', '# Body\n'],
  ['空行複数', frontMatter + '\n\n\n# Body\n', '# Body\n'],
  ['spaceとtabだけの行', frontMatter + ' \t\n\t \n  # Body\n', '  # Body\n'],
  ['indent code', frontMatter + '\n    code  \n', '    code  \n'],
  ['本文内部と末尾', frontMatter + '\n# Body\n\n\nText  \n\n', '# Body\n\n\nText  \n\n'],
  ['code内の空白', frontMatter + '\n` a  `\n\n```text\n  keep  \n\n```\n', '` a  `\n\n```text\n  keep  \n\n```\n'],
  ['終了区切りで終わる', frontMatter.slice(0, -1), ''],
  ['空本文', frontMatter, ''],
  ['空行だけ', frontMatter + '\n', ''],
  ['空白だけの本文', frontMatter + ' \t\n\t ', ''],
  ['front matterなし', '# Body\n', '# Body\n'],
  ['front matterなしの先頭空行', '\n\n# Body\n', '# Body\n'],
  ['front matterなしのindent', '\n    code\n', '    code\n'],
  ['空文字', '', ''],
  ['改行のないspaceとtab', ' \t ', ''],
  ['CRLFの本文を保持', frontMatter.replace(/\n/g, '\r\n') + '\r\n  # Body\r\n\r\n', '  # Body\r\n\r\n'],
  ['本文の全角spaceを保持', frontMatter + '\n　Body\n', '　Body\n'],
];

for (const [name, raw, expected] of cases) {
  test(`テンプレート本文の調整: ${name}`, () => {
    assert.equal(normalizeTemplateBody(stripFrontMatter(raw)), expected);
  });
}

for (const flag of [false, true, undefined]) {
  test(`macro_expand=${String(flag)}の展開結果とcode内placeholderを保持する`, () => {
    const body = '{{!page:basename}}\n\n`{{!page:basename}}`\n```text\n  {{!page:basename}}  \n```\n';
    const expanded = resolveTemplateImmediateMacros(
      stripFrontMatter(frontMatter + '\n\n' + body),
      { pagePath: '/matrix/target' },
      flag === true,
    );
    const expected = flag === true ? 'target' + body.slice('{{!page:basename}}'.length) : body;
    assert.equal(normalizeTemplateBody(expanded), expected);
  });
}

test('共有front matter除去では先頭空行を保持し、適用境界だけで除去する', () => {
  const source = frontMatter + '\n\n    code  \n\n';
  const body = stripFrontMatter(source);
  assert.equal(body, '\n\n    code  \n\n');
  assert.equal(normalizeTemplateBody(body), '    code  \n\n');
});
