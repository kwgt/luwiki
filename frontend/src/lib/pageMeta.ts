const DEFAULT_WIKI_TITLE = 'LUWIKI';

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
