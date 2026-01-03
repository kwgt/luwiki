export function ensureTabId(): string {
  const key = 'luwiki-tab-id';
  const existing = sessionStorage.getItem(key);
  if (existing) {
    return existing;
  }
  const value = typeof crypto !== 'undefined' && crypto.randomUUID
    ? crypto.randomUUID()
    : `tab-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  sessionStorage.setItem(key, value);
  return value;
}

export function buildLockTokenKey(pageId: string): string {
  const tabId = ensureTabId();
  return `luwiki-lock-token:${pageId}:${tabId}`;
}
