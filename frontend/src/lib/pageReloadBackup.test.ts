import test from 'node:test';
import assert from 'node:assert/strict';
import { effectScope } from 'vue';
import { usePageView } from '../composables/usePageView';
import { readPageReloadBackup, reloadWithBackup, type PageReloadBackup } from './pageReloadBackup';
import { parseWebSocketPacket } from './websocket';

/** 同タブのprivate storageを外部資材なしで隔離する。 */
class MemoryStorage implements Storage {
  values = new Map<string, string>();
  get length(): number { return this.values.size; }
  clear(): void { this.values.clear(); }
  getItem(key: string): string | null { return this.values.get(key) ?? null; }
  key(index: number): string | null { return [...this.values.keys()][index] ?? null; }
  removeItem(key: string): void { this.values.delete(key); }
  setItem(key: string, value: string): void { this.values.set(key, value); }
}

/** 保存直前の合法な表示一回分。 */
function backup(): PageReloadBackup {
  return { id: 'page-one', revision: 1, path: '/p', viewer: 'viewer-one', tab: 'tab-one',
    source: '# prior', html: '<p>prior display</p>', assets: [], codes: [],
    stateToken: 'v1:' + 'a'.repeat(64) + ':' + 'b'.repeat(64),
    meta: { page_info: { path: { kind: 'current', value: '/p' }, short_path: { kind: 'unavailable' },
      revision_scope: { latest: 1, oldest: 1 }, rename_revisions: [1], deleted: false, locked: false } } };
}

/** 変更したglobalを試験後に復元する。 */
function environment(): { storage: MemoryStorage; cleanup: () => void } {
  const keys = ['location', 'history', 'sessionStorage'] as const;
  const previous = keys.map(key => Object.getOwnPropertyDescriptor(globalThis, key));
  const storage = new MemoryStorage(); storage.setItem('luwiki-tab-id', 'tab-one');
  Object.defineProperty(globalThis, 'location', { configurable: true, value: new URL('http://example.test/wiki/p?_view_reload=1') });
  Object.defineProperty(globalThis, 'sessionStorage', { configurable: true, value: storage });
  Object.defineProperty(globalThis, 'history', { configurable: true, value: { state: null,
    replaceState(_state: unknown, _title: string, url: string | URL): void {
      Object.defineProperty(globalThis, 'location', { configurable: true, value: new URL(String(url), 'http://example.test') });
    } } });
  return { storage, cleanup: () => keys.forEach((key, i) => {
    if (previous[i]) Object.defineProperty(globalThis, key, previous[i]!);
    else Reflect.deleteProperty(globalThis, key);
  }) };
}

test('退避は同じtab/pathかつ構造の正しい記録だけを利用する', () => {
  const env = environment();
  try {
    for (const value of [
      { ...backup(), tab: 'other-tab' }, { ...backup(), path: '/other' },
      { ...backup(), meta: {} }, { ...backup(), assets: [{}] }, { ...backup(), codes: [[1]] },
    ]) {
      env.storage.setItem('luwiki-page-reload-backup', JSON.stringify(value));
      assert.equal(readPageReloadBackup(), null);
    }
    env.storage.setItem('luwiki-page-reload-backup', JSON.stringify(backup()));
    assert.ok(readPageReloadBackup());
    env.storage.setItem('luwiki-page-reload-backup', '{');
    assert.equal(readPageReloadBackup(), null);
  } finally { env.cleanup(); }
});

test('再接続で確認した主体が異なる場合は退避本文を描画しない', () => {
  const env = environment(); const scope = effectScope();
  try {
    const view = scope.run(() => usePageView())!;
    view.recovery.value = backup(); view.restoreBackup('different-viewer');
    assert.equal(view.renderedHtml.value, '');
    assert.equal(view.source.value, '');
    assert.ok(view.errorMessage.value.includes('復元できません'));
  } finally { scope.stop(); env.cleanup(); }
});

test('同じ主体なら退避本文を復元し、容量不足ならURL変更前に停止する', () => {
  const env = environment(); const scope = effectScope();
  try {
    const view = scope.run(() => usePageView())!;
    view.recovery.value = backup(); view.restoreBackup('viewer-one');
    assert.equal(view.renderedHtml.value, '<p>prior display</p>');
    env.storage.setItem = () => { throw new Error('test quota'); };
    assert.throws(() => reloadWithBackup(backup()), /test quota/);
    assert.ok(!location.href.includes('_view_reload'));
  } finally { scope.stop(); env.cleanup(); }
});

test('共通parserは将来のtypeも機能へ渡せるがchannelを解釈しない', () => {
  assert.equal(parseWebSocketPacket('{"type":"future.changed","payload":{}}')?.type, 'future.changed');
  assert.equal(parseWebSocketPacket('{"type":"future.changed","payload":{},"channel":"future"}'), null);
});
