const TAB_ID_KEY = 'luwiki-tab-id';
const TAB_ID_CHANNEL = 'luwiki-tab-id-channel';
const TAB_ID_WAIT_MS = 50;

let tabIdReadyPromise: Promise<string> | null = null;

function generateTabId(): string {
  return typeof crypto !== 'undefined' && crypto.randomUUID
    ? crypto.randomUUID()
    : `tab-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function getStoredTabId(): string | null {
  return sessionStorage.getItem(TAB_ID_KEY);
}

async function resolveTabId(): Promise<string> {
  let currentId = getStoredTabId();
  if (!currentId) {
    currentId = generateTabId();
    sessionStorage.setItem(TAB_ID_KEY, currentId);
  }

  if (typeof BroadcastChannel === 'undefined') {
    return currentId;
  }

  const channel = new BroadcastChannel(TAB_ID_CHANNEL);

  try {
    for (let attempt = 0; attempt < 2; attempt += 1) {
      let duplicateDetected = false;
      const onMessage = (event: MessageEvent) => {
        const data = event.data as { type?: string; id?: string } | null;
        if (!data || typeof data !== 'object') {
          return;
        }
        if (data.type === 'probe' && data.id === currentId) {
          channel.postMessage({ type: 'exists', id: currentId });
        }
        if (data.type === 'exists' && data.id === currentId) {
          duplicateDetected = true;
        }
      };

      channel.addEventListener('message', onMessage);
      channel.postMessage({ type: 'probe', id: currentId });
      await new Promise((resolve) => {
        window.setTimeout(resolve, TAB_ID_WAIT_MS);
      });
      channel.removeEventListener('message', onMessage);

      if (!duplicateDetected) {
        return currentId;
      }

      currentId = generateTabId();
      sessionStorage.setItem(TAB_ID_KEY, currentId);
    }
  } finally {
    channel.close();
  }

  return currentId;
}

export function ensureTabIdReady(): Promise<string> {
  if (!tabIdReadyPromise) {
    tabIdReadyPromise = resolveTabId();
  }
  return tabIdReadyPromise;
}

export function getTabId(): string | null {
  return getStoredTabId();
}

export function buildLockTokenKey(pageId: string): string {
  const tabId = getStoredTabId();
  if (!tabId) {
    throw new Error('tab id not ready');
  }
  return `luwiki-lock-token:${pageId}:${tabId}`;
}

export function tryBuildLockTokenKey(pageId: string): string | null {
  const tabId = getStoredTabId();
  if (!tabId) {
    return null;
  }
  return `luwiki-lock-token:${pageId}:${tabId}`;
}
