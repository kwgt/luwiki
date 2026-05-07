import test from 'node:test';
import assert from 'node:assert/strict';

import {
  getMetaContent,
  getWikiIconUrl,
  getWikiTitle,
} from './pageMeta.ts';
import { stripLeadingTitleHeading } from './pageContent.ts';

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
