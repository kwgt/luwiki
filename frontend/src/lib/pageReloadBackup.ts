import type { PageMetaResponse } from '../api/pages';
import type { PageAsset } from '../api/assets';
import { getTabId } from './lockToken';
import { currentViewPath } from './pageViewPosition';

/** 通知reload一回分の既存表示。 */
export interface PageReloadBackup {
  id: string; revision: number | null; path: string; viewer: string; tab: string;
  source: string; meta: PageMetaResponse; assets: PageAsset[];
  html: string; codes: [string, string][]; stateToken: string;
}
const KEY = 'luwiki-page-reload-backup';

/** private storageでも外部境界として構造を検証する。 */
function object(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
/** asset一覧の復元で不正値を表示処理へ渡さない。 */
function storedAsset(value: unknown): value is PageAsset {
  return object(value) && typeof value.id === 'string' && typeof value.file_name === 'string'
    && typeof value.mime_type === 'string' && typeof value.size === 'number' && Number.isFinite(value.size) && value.size >= 0
    && typeof value.timestamp === 'string' && typeof value.username === 'string';
}
/** モーダル等が参照するメタ情報も検証する。 */
function storedMeta(value: unknown): value is PageMetaResponse {
  if (!object(value) || !object(value.page_info)) return false;
  const info = value.page_info;
  if (!object(info.path) || !['current', 'last_deleted'].includes(String(info.path.kind)) || typeof info.path.value !== 'string'
    || !object(info.short_path) || !['available', 'unavailable'].includes(String(info.short_path.kind))
    || (info.short_path.kind === 'available' && typeof info.short_path.value !== 'string')
    || !object(info.revision_scope) || typeof info.revision_scope.latest !== 'number' || typeof info.revision_scope.oldest !== 'number'
    || !Number.isSafeInteger(info.revision_scope.latest) || !Number.isSafeInteger(info.revision_scope.oldest)
    || !Array.isArray(info.rename_revisions) || !info.rename_revisions.every((n: unknown) => typeof n === 'number' && Number.isSafeInteger(n))
    || typeof info.deleted !== 'boolean' || typeof info.locked !== 'boolean') return false;
  if (value.revision_info !== undefined) {
    const revision = value.revision_info;
    if (!object(revision) || typeof revision.revision !== 'number' || typeof revision.timestamp !== 'string' || typeof revision.username !== 'string') return false;
    if (revision.rename_info !== undefined) {
      const rename = revision.rename_info;
      if (!object(rename) || !['active', 'removed_by_migrate'].includes(String(rename.kind))) return false;
      if (rename.kind === 'active' && ((rename.from !== null && typeof rename.from !== 'string') || typeof rename.to !== 'string'
        || !object(rename.link_refs) || !Object.values(rename.link_refs).every(v => v === null || typeof v === 'string'))) return false;
    }
  }
  return true;
}

/** 自分が保存した同タブ・同pathの退避だけを返す。 */
export function readPageReloadBackup(): PageReloadBackup | null {
  try {
    if (new URL(location.href).searchParams.get('_view_reload') !== '1') return null;
    const value: unknown = JSON.parse(sessionStorage.getItem(KEY) ?? 'null');
    if (typeof value !== 'object' || value === null) return null;
    const v = value as Record<string, unknown>;
    if (v.path !== currentViewPath() || v.tab !== getTabId()
      || typeof v.id !== 'string' || typeof v.source !== 'string' || typeof v.viewer !== 'string'
      || typeof v.html !== 'string' || typeof v.stateToken !== 'string' || !/^v1:[a-f0-9]{64}:[a-f0-9]{64}$/.test(v.stateToken)
      || !Array.isArray(v.assets) || !v.assets.every(storedAsset) || !Array.isArray(v.codes)
      || !v.codes.every(entry => Array.isArray(entry) && entry.length === 2 && entry.every(part => typeof part === 'string'))
      || (v.revision !== null && (typeof v.revision !== 'number' || !Number.isSafeInteger(v.revision)))
      || !storedMeta(v.meta)) return null;
    return v as unknown as PageReloadBackup;
  } catch { return null; }
}

/** 保存成功を確認してから一時印付きで通常reloadする。 */
export function reloadWithBackup(value: PageReloadBackup): void {
  if (!value.viewer) throw new Error('viewer not confirmed');
  const serialized = JSON.stringify(value);
  sessionStorage.setItem(KEY, serialized);
  if (sessionStorage.getItem(KEY) !== serialized) throw new Error('backup failed');
  const url = new URL(location.href); url.searchParams.set('_view_reload', '1');
  history.replaceState(history.state, '', url);
  location.reload();
}

/** 新文書の表示確定時に一時印と退避を消す。 */
export function clearPageReloadBackup(): void {
  try { sessionStorage.removeItem(KEY); } catch { /* 表示結果を変更しない。 */ }
  const url = new URL(location.href);
  if (url.searchParams.has('_view_reload')) { url.searchParams.delete('_view_reload'); history.replaceState(history.state, '', url); }
}
