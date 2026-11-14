import { parseWebSocketPacket } from './websocket';

/** 閲覧設定の保存値。 */
export type PageUpdateMode = 'auto' | 'manual' | 'ignore';
/** 同一snapshotの状態分類。 */
export type PageChangeKind = 'unchanged' | 'updated' | 'replaced' | 'renamed' | 'unavailable';

/** 認可済みのページ通知payload。 */
export interface PageEventState {
  kind: PageChangeKind;
  viewer_key: string;
  page_state: null | {
    id: string;
    revision: number;
    instance_id: string | null;
    state_token: string;
  };
  navigation: null | { kind: 'renamed' | 'ancestor'; path: string };
}

/** 共通形式を通した受信値。 */
export interface PageEventPacket {
  type: 'page.state' | 'page.changed' | 'page.checked' | 'page.error' | 'error';
  payload: PageEventState | { code: string; message: string };
}

/** 外部objectを安全に絞り込む。 */
function object(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** wireの状態値を型検証する。 */
export function isPageEventState(value: unknown): value is PageEventState {
  if (!object(value) || !['unchanged', 'updated', 'replaced', 'renamed', 'unavailable'].includes(String(value.kind))
    || typeof value.viewer_key !== 'string') return false;
  if (value.navigation !== null && (!object(value.navigation)
    || !['renamed', 'ancestor'].includes(String(value.navigation.kind))
    || typeof value.navigation.path !== 'string' || !value.navigation.path.startsWith('/'))) return false;
  if (value.page_state === null) return ['renamed', 'unavailable'].includes(String(value.kind));
  const state = value.page_state;
  return object(state) && typeof state.id === 'string'
    && typeof state.revision === 'number' && Number.isSafeInteger(state.revision) && state.revision >= 1
    && (state.instance_id === null || typeof state.instance_id === 'string')
    && typeof state.state_token === 'string' && /^v1:[0-9a-f]{64}:[0-9a-f]{64}$/.test(state.state_token)
    && ['unchanged', 'updated', 'replaced'].includes(String(value.kind));
}

/** 2項目だけの共通形式とpage.*のpayloadを検証する。 */
export function parsePageEvent(text: string): PageEventPacket | null {
  const value = parseWebSocketPacket(text);
  if (!value) return null;
  if (value.type === 'error' || value.type === 'page.error') {
    if (!object(value.payload) || typeof value.payload.code !== 'string' || typeof value.payload.message !== 'string') return null;
    return { type: value.type, payload: { code: value.payload.code, message: value.payload.message } };
  }
  if (!['page.state', 'page.changed', 'page.checked'].includes(value.type) || !isPageEventState(value.payload)) return null;
  return { type: value.type as 'page.state' | 'page.changed' | 'page.checked', payload: value.payload };
}

/** 設定の不正値・未設定を半自動へ戻す。 */
export function pageUpdateMode(value: unknown): PageUpdateMode {
  return value === 'auto' || value === 'ignore' ? value : 'manual';
}
