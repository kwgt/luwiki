import test from 'node:test';
import assert from 'node:assert/strict';

import {
  extractTitle,
  extractToc,
  getMetaContent,
  getWikiIconUrl,
  getWikiTitle,
} from './pageCommon';
import { stripFrontMatter, stripLeadingTitleHeading } from './pageContent';

const documentMock = {
  head: {
    innerHTML: '',
  },
  querySelector(selector: string): { getAttribute(name: string): string | null } | null {
    const matched = selector.match(/^meta\[name="(.+)"\]$/);
    if (!matched) {
      return null;
    }

    const name = matched[1];
    const pattern = new RegExp(
      `<meta\\s+name="${name}"\\s+content="([^"]*)">`,
    );
    const tag = this.head.innerHTML.match(pattern);
    if (!tag) {
      return null;
    }

    const content = tag[1] ?? '';
    return {
      getAttribute(attributeName: string): string | null {
        return attributeName === 'content' ? content : null;
      },
    };
  },
};

Object.defineProperty(globalThis, 'document', {
  value: documentMock,
  configurable: true,
});

test('stripLeadingTitleHeading は先頭のトップレベル見出しだけを除去する', () => {
  const markdown = '# タイトル\n\n導入文です。\n\n## セクション\n\n本文';

  assert.equal(
    stripLeadingTitleHeading(markdown),
    '導入文です。\n\n## セクション\n\n本文',
  );
});

test('stripFrontMatter は先頭の front matter だけを除去する', () => {
  const markdown = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# タイトル',
    '',
    '本文',
  ].join('\n');

  assert.equal(stripFrontMatter(markdown), '# タイトル\n\n本文');
});

test('stripFrontMatter は先頭以外の区切りブロックを残す', () => {
  const markdown = [
    '# タイトル',
    '',
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '本文',
  ].join('\n');

  assert.equal(stripFrontMatter(markdown), markdown);
});

test('stripFrontMatter は閉じ区切りがない場合に原文を返す', () => {
  const markdown = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '# タイトル',
  ].join('\n');

  assert.equal(stripFrontMatter(markdown), markdown);
});

test('閲覧側のタイトル抽出は front matter 除去後本文を前提にできる', () => {
  const markdown = [
    '---',
    'title: metadata only',
    '---',
    '# 本文タイトル',
    '',
    '本文',
  ].join('\n');

  assert.equal(
    extractTitle(stripFrontMatter(markdown), '/docs/example'),
    '本文タイトル',
  );
});

test('閲覧側の TOC 抽出は front matter 除去後本文を前提にできる', () => {
  const markdown = [
    '---',
    'note: metadata only',
    '---',
    '# 本文タイトル',
    '',
    '## セクションA',
    '',
    '本文',
    '',
    '### セクションB',
  ].join('\n');

  assert.deepEqual(
    extractToc(stripFrontMatter(markdown)),
    [
      {
        level: 2,
        text: 'セクションA',
        anchor: 'セクションa',
      },
      {
        level: 3,
        text: 'セクションB',
        anchor: 'セクションb',
      },
    ],
  );
});

test('編集プレビューは front matter を含む raw source から本文だけを対象にできる', () => {
  const rawSource = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
    '',
    '本文',
  ].join('\n');

  assert.equal(
    stripFrontMatter(rawSource),
    '# 本文タイトル\n\n本文',
  );
});

test('編集プレビュー向け本文は front matter 除去後にマクロ展開へ渡せる形を維持する', () => {
  const rawSource = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
    '',
    '{{toc}}',
    '',
    '本文',
  ].join('\n');

  assert.equal(
    stripFrontMatter(rawSource),
    '# 本文タイトル\n\n{{toc}}\n\n本文',
  );
});

test('編集プレビュー向け本文は front matter 除去後も本文見出しを維持する', () => {
  const rawSource = [
    '---',
    'wiki:',
    '  tags:',
    '    - rust',
    '---',
    '# 本文タイトル',
    '',
    '## セクション',
    '',
    '本文',
  ].join('\n');

  const previewSource = stripFrontMatter(rawSource);

  assert.equal(previewSource, '# 本文タイトル\n\n## セクション\n\n本文');
  assert.equal(
    stripLeadingTitleHeading(previewSource),
    '## セクション\n\n本文',
  );
});

test('front matter 内の見出し風文字列はタイトル抽出と TOC 抽出の対象にならない', () => {
  const markdown = [
    '---',
    'summary: "# front matter title"',
    'outline: "## front matter section"',
    '---',
    '# 本文タイトル',
    '',
    '## 本文セクション',
    '',
    '本文',
  ].join('\n');

  const sourceBody = stripFrontMatter(markdown);

  assert.equal(extractTitle(sourceBody, '/docs/example'), '本文タイトル');
  assert.deepEqual(
    extractToc(sourceBody),
    [
      {
        level: 2,
        text: '本文セクション',
        anchor: '本文セクション',
      },
    ],
  );
});

test('stripLeadingTitleHeading は先頭以外のトップレベル見出しを残す', () => {
  const markdown = '導入文です。\n\n# タイトル\n\n本文';

  assert.equal(stripLeadingTitleHeading(markdown), markdown);
});

test('stripLeadingTitleHeading は先頭の空行を許容する', () => {
  const markdown = '\n\n# タイトル\n\n本文';

  assert.equal(stripLeadingTitleHeading(markdown), '本文');
});

test('stripLeadingTitleHeading は二階層目以降の見出しを除去しない', () => {
  const markdown = '## セクション\n\n本文';

  assert.equal(stripLeadingTitleHeading(markdown), markdown);
});

test('getWikiIconUrl は meta の URL を返す', () => {
  document.head.innerHTML = '<meta name="wiki-icon-url" content="/wiki-icon">';

  assert.equal(getWikiIconUrl(), '/wiki-icon');
});

test('getMetaContent は指定した meta content を返す', () => {
  document.head.innerHTML = [
    '<meta name="wiki-title" content="My Wiki">',
    '<meta name="wiki-icon-url" content="/wiki-icon">',
  ].join('');

  assert.equal(getMetaContent('wiki-title'), 'My Wiki');
  assert.equal(getMetaContent('wiki-icon-url'), '/wiki-icon');
});

test('getWikiTitle は meta の wiki-title を返す', () => {
  document.head.innerHTML = '<meta name="wiki-title" content="Sandbox Wiki">';

  assert.equal(getWikiTitle(), 'Sandbox Wiki');
});

test('getWikiTitle は未設定時に既定値を返す', () => {
  document.head.innerHTML = '';

  assert.equal(getWikiTitle(), 'LUWIKI');
});

test('getWikiTitle は空文字または空白のみを既定値として扱う', () => {
  document.head.innerHTML = '<meta name="wiki-title" content="   ">';

  assert.equal(getWikiTitle(), 'LUWIKI');
});

test('getWikiIconUrl は未設定時に null を返す', () => {
  document.head.innerHTML = '';

  assert.equal(getWikiIconUrl(), null);
});

test('getWikiIconUrl は空文字または空白のみを null として扱う', () => {
  document.head.innerHTML = '<meta name="wiki-icon-url" content="   ">';

  assert.equal(getWikiIconUrl(), null);
});
