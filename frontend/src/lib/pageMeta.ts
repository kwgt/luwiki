const DEFAULT_WIKI_TITLE = 'LUWIKI';

/**
 * 公開metaからauthorityを取得する。推測・正規化はしない。
 * @returns 有効なhostname風authority、または未取得/不正時のnull。
 */
export function getMcpAuthority(): string | null {
  const value = getMetaContent('mcp-authority');
  if (!value || value.length > 253) {
    return null;
  }
  const valid = value.split('.').every((label) => {
    return label.length > 0 && label.length <= 63
      && !/[^A-Za-z0-9-]/.test(label)
      && !label.startsWith('-') && !label.endsWith('-');
  });
  return valid ? value : null;
}

export function getMetaContent(name: string): string | null {
  const tag = document.querySelector(`meta[name="${name}"]`);
  return tag?.getAttribute('content') ?? null;
}

export function getWikiTitle(): string {
  const value = getMetaContent('wiki-title')?.trim() ?? '';
  return value.length > 0 ? value : DEFAULT_WIKI_TITLE;
}

export function getWikiIconUrl(): string | null {
  const value = getMetaContent('wiki-icon-url')?.trim() ?? '';
  return value.length > 0 ? value : null;
}
