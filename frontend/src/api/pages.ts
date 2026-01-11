import { apiClient } from './client';

export interface PagePathInfo {
  kind: 'current' | 'last_deleted';
  value: string;
}

export interface PageInfo {
  path: PagePathInfo;
  revision_scope: {
    latest: number;
    oldest: number;
  };
  rename_revisions: number[];
  deleted: boolean;
  locked: boolean;
}

export interface RevisionInfo {
  revision: number;
  timestamp: string;
  username: string;
  rename_info?: {
    from?: string;
    to: string;
  };
}

export interface PageMetaResponse {
  page_info: PageInfo;
  revision_info?: RevisionInfo;
}

export interface CreatePageResponse {
  id: string;
  lockToken: string;
}

export interface PageLockInfo {
  expire: string;
  username: string;
}

export interface SearchResult {
  page_id: string;
  revision: number;
  score: number;
  path: string;
  deleted: boolean;
  text: string;
}

function parseLockToken(headerValue: string): string | null {
  const parts = headerValue.split(/\s+/);
  for (const part of parts) {
    if (part.startsWith('token=')) {
      const token = part.slice('token='.length);
      return token.length > 0 ? token : null;
    }
  }
  return null;
}

function extractErrorReason(data: unknown): string | undefined {
  if (data && typeof data === 'object' && 'reason' in data) {
    const reason = (data as { reason?: unknown }).reason;
    if (typeof reason === 'string' && reason.trim().length > 0) {
      return reason;
    }
  }
  return undefined;
}

function buildRequestError(status: number, data?: unknown): Error & {
  status?: number;
  reason?: string;
  data?: unknown;
} {
  const err = new Error('request failed') as Error & {
    status?: number;
    reason?: string;
    data?: unknown;
  };
  err.status = status;
  err.data = data;
  const reason = extractErrorReason(data);
  if (reason) {
    err.reason = reason;
  }
  return err;
}

/**
 * ページのメタ情報を取得する
 */
export async function fetchPageMeta(
  pageId: string,
  revision?: number,
  noCache?: boolean,
): Promise<PageMetaResponse> {
  const headers = noCache
    ? {
      'Cache-Control': 'no-cache',
      Pragma: 'no-cache',
    }
    : undefined;
  const res = await apiClient.get<PageMetaResponse>(
    `/pages/${pageId}/meta`,
    {
      headers,
      params: revision ? { rev: revision } : undefined,
    },
  );
  return res.data;
}

/**
 * 削除済みページ候補を取得する
 */
export async function fetchDeletedPageCandidates(
  path: string,
): Promise<string[]> {
  const res = await apiClient.get<string[]>(
    '/pages/deleted',
    {
      params: { path },
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
  return res.data;
}

/**
 * ページを復帰する
 */
export async function restorePagePath(
  pageId: string,
  restoreTo: string,
  recursive?: boolean,
): Promise<void> {
  const res = await apiClient.post(
    `/pages/${pageId}/path`,
    null,
    {
      params: {
        restore_to: restoreTo,
        ...(recursive ? { recursive: true } : {}),
      },
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
}

/**
 * ページを移動する
 */
export async function renamePagePath(
  pageId: string,
  renameTo: string,
  recursive?: boolean,
): Promise<void> {
  const res = await apiClient.post(
    `/pages/${pageId}/path`,
    null,
    {
      params: {
        rename_to: renameTo,
        ...(recursive ? { recursive: true } : {}),
      },
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
}

/**
 * ページを削除する
 */
export async function deletePage(
  pageId: string,
  lockToken?: string,
  recursive?: boolean,
): Promise<void> {
  const headers: Record<string, string> = {};
  if (lockToken) {
    headers['X-Lock-Authentication'] = `token=${lockToken}`;
  }
  const res = await apiClient.delete(
    `/pages/${pageId}`,
    {
      headers,
      params: recursive ? { recursive: true } : undefined,
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
}

/**
 * 親ページ情報を取得する
 */
export async function fetchParentPage(
  pageId: string,
  recursive?: boolean,
): Promise<{ id: string; path: string }> {
  const res = await apiClient.get<{ id: string; path: string }>(
    `/pages/${pageId}/parent`,
    {
      params: recursive === undefined ? undefined : { recursive },
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
  return res.data;
}

/**
 * ページを作成する
 */
export async function createPage(
  path: string,
): Promise<CreatePageResponse> {
  const res = await apiClient.post<CreatePageResponse>(
    '/pages',
    null,
    {
      params: { path },
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
  const headerValue = res.headers['x-page-lock'];
  if (!headerValue || typeof headerValue !== 'string') {
    throw new Error('lock token missing');
  }
  const token = parseLockToken(headerValue);
  if (!token) {
    throw new Error('lock token invalid');
  }
  return {
    id: res.data.id,
    lockToken: token,
  };
}

/**
 * ページのMarkdownソースを取得する
 */
export async function fetchPageSource(
  pageId: string,
  revision?: number,
  noCache?: boolean,
): Promise<string> {
  const headers = noCache
    ? {
      'Cache-Control': 'no-cache',
      Pragma: 'no-cache',
    }
    : undefined;
  const res = await apiClient.get<string>(`/pages/${pageId}/source`, {
    params: revision ? { rev: revision } : undefined,
    headers,
    responseType: 'text',
  });
  return res.data;
}

/**
 * ページソースを更新する
 */
export async function updatePageSource(
  pageId: string,
  source: string,
  lockToken?: string,
  amend?: boolean,
): Promise<void> {
  const headers: Record<string, string> = {
    'Content-Type': 'text/markdown',
  };
  if (lockToken) {
    headers['X-Lock-Authentication'] = `token=${lockToken}`;
  }
  const res = await apiClient.put(
    `/pages/${pageId}/source`,
    source,
    {
      params: amend === undefined ? undefined : { amend },
      headers,
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
}

/**
 * ページロックを取得する
 */
export async function acquirePageLock(pageId: string): Promise<string> {
  const res = await apiClient.post(`/pages/${pageId}/lock`);
  const headerValue = res.headers['x-page-lock'];
  if (!headerValue || typeof headerValue !== 'string') {
    throw new Error('lock token missing');
  }
  const token = parseLockToken(headerValue);
  if (!token) {
    throw new Error('lock token invalid');
  }
  return token;
}

/**
 * ページロックを延長する
 */
export async function extendPageLock(
  pageId: string,
  lockToken: string,
): Promise<string> {
  const res = await apiClient.put(
    `/pages/${pageId}/lock`,
    null,
    {
      headers: {
        'X-Lock-Authentication': `token=${lockToken}`,
      },
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
  const headerValue = res.headers['x-page-lock'];
  if (!headerValue || typeof headerValue !== 'string') {
    throw new Error('lock token missing');
  }
  const token = parseLockToken(headerValue);
  if (!token) {
    throw new Error('lock token invalid');
  }
  return token;
}

/**
 * ページロックを解除する
 */
export async function unlockPageLock(
  pageId: string,
  lockToken: string,
): Promise<void> {
  const res = await apiClient.delete(`/pages/${pageId}/lock`, {
    headers: {
      'X-Lock-Authentication': `token=${lockToken}`,
    },
    validateStatus: () => true,
  });
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
}

/**
 * ページロック情報を取得する
 */
export async function fetchPageLockInfo(
  pageId: string,
): Promise<PageLockInfo> {
  const res = await apiClient.get<PageLockInfo>(
    `/pages/${pageId}/lock`,
    {
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
  return res.data;
}

/**
 * ページを検索する
 */
export async function searchPages(params: {
  expression: string;
  targets: Array<'headings' | 'body' | 'code'>;
  withDeleted: boolean;
  allRevision: boolean;
}): Promise<SearchResult[]> {
  const targets = params.targets.length > 0 ? params.targets : ['body'];
  const res = await apiClient.get<SearchResult[]>(
    '/pages/search',
    {
      params: {
        expr: params.expression,
        target: targets.join(','),
        with_deleted: params.withDeleted,
        all_revision: params.allRevision,
      },
      validateStatus: () => true,
    },
  );
  if (res.status >= 400) {
    throw buildRequestError(res.status, res.data);
  }
  return res.data;
}
