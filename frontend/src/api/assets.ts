import { apiClient } from './client';

export interface PageAsset {
  id: string;
  file_name: string;
  mime_type: string;
  size: number;
  timestamp: string;
  username: string;
}

export interface UploadAssetResponse {
  id: string;
}

export interface AssetMetaResponse {
  file_name: string;
  mime_type: string;
  size: number;
  timestamp: string;
  username: string;
}

/**
 * ページに付随するアセット一覧を取得する
 */
export async function fetchPageAssets(
  pageId: string,
  cacheBust?: string | number,
): Promise<PageAsset[]> {
  const suffix = cacheBust ? `?t=${encodeURIComponent(String(cacheBust))}` : '';
  const res = await apiClient.get<PageAsset[]>(
    `/pages/${pageId}/assets${suffix}`,
  );
  return res.data;
}

/**
 * ページにアセットをアップロードする
 */
export async function uploadPageAsset(
  pageId: string,
  fileName: string,
  data: Blob,
  mimeType?: string,
  onProgress?: (loaded: number, total?: number) => void,
  lockToken?: string,
): Promise<UploadAssetResponse> {
  const encoded = encodeURIComponent(fileName);
  const contentType = mimeType && mimeType.trim().length > 0
    ? mimeType
    : 'application/octet-stream';
  const headers: Record<string, string> = {
    'Content-Type': contentType,
  };
  if (lockToken) {
    headers['X-Lock-Authentication'] = `token=${lockToken}`;
  }
  const res = await apiClient.post<UploadAssetResponse>(
    `/pages/${pageId}/assets/${encoded}`,
    data,
    {
      headers,
      validateStatus: () => true,
      onUploadProgress: (event) => {
        if (onProgress) {
          onProgress(event.loaded, event.total ?? undefined);
        }
      },
    },
  );
  if (res.status >= 400) {
    const err = new Error('request failed') as Error & {
      status?: number;
      reason?: string;
      data?: unknown;
    };
    err.status = res.status;
    err.data = res.data;
    if (res.data && typeof res.data === 'object' && 'reason' in res.data) {
      const reason = (res.data as { reason?: unknown }).reason;
      if (typeof reason === 'string') {
        err.reason = reason;
      }
    }
    throw err;
  }
  return res.data;
}

/**
 * アセットのメタ情報を取得する
 */
export async function fetchAssetMeta(assetId: string): Promise<AssetMetaResponse> {
  const res = await apiClient.get<AssetMetaResponse>(`/assets/${assetId}/meta`);
  return res.data;
}

/**
 * アセットを削除する
 */
export async function deleteAsset(assetId: string): Promise<void> {
  await apiClient.delete(`/assets/${assetId}`);
}
