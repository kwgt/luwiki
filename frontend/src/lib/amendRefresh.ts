const AMEND_REFRESH_KEY_PREFIX = 'luwiki-amend-refresh:';

export function buildAmendRefreshKey(pageId: string): string {
  return `${AMEND_REFRESH_KEY_PREFIX}${pageId}`;
}
