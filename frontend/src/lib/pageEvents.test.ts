import test from 'node:test';
import assert from 'node:assert/strict';
import { parsePageEvent, pageUpdateMode } from './pageEvents';
import {
  clampedScroll, isViewPosition, readHistoryViewPosition, readViewPosition,
  storeViewPosition, type ViewPosition,
} from './pageViewPosition';

test('共通packetはtype/payloadだけを受理し旧channelを拒否する', () => {
  const value = { type: 'page.changed', payload: {
    kind: 'unavailable', viewer_key: 'viewer', page_state: null, navigation: null,
  } };
  assert.ok(parsePageEvent(JSON.stringify(value)));
  assert.equal(parsePageEvent(JSON.stringify({ ...value, channel: 'page_changes' })), null);
  assert.equal(parsePageEvent(JSON.stringify({ ...value, payload: null })), null);
  assert.equal(parsePageEvent('{'), null);
  assert.equal(parsePageEvent(JSON.stringify({ ...value, type: 'other.changed' })), null);
});

test('通知・初期応答・確認応答をtypeで識別し不正な状態は適用しない', () => {
  const payload = { kind: 'updated', viewer_key: 'viewer', navigation: null,
    page_state: { id: 'page', revision: 2, instance_id: null, state_token: 'v1:' + 'a'.repeat(64) + ':' + 'b'.repeat(64) } };
  for (const type of ['page.state', 'page.changed', 'page.checked']) {
    assert.equal(parsePageEvent(JSON.stringify({ type, payload }))?.type, type);
  }
  assert.equal(parsePageEvent(JSON.stringify({ type: 'page.changed', payload: { ...payload, page_state: null } })), null);
});

test('設定既定と短縮本文のscroll補正・破損記録を扱う', () => {
  assert.equal(pageUpdateMode(null), 'manual');
  assert.equal(pageUpdateMode('bad'), 'manual');
  assert.equal(pageUpdateMode('ignore'), 'ignore');
  assert.equal(clampedScroll(900, 500, 600), 0);
  assert.equal(clampedScroll(900, 1000, 500), 500);
  assert.equal(isViewPosition({ path: '/p', id: 'p', hash: '', documentTop: Infinity, articleTop: 0, anchor: null, heading: null }), false);
});


test('戻る・進むは同じ URL の別エントリを区別し、単一のリロード用記録を流用しない', (t) => {
  const values = new Map<string, string>();
  const history = {
    state: null as object | null,
    replaceState(state: object) { this.state = state; },
  };
  let navigationType = 'navigate';
  const location = { href: 'https://wiki.test/wiki/p', pathname: '/wiki/p', hash: '' };
  for (const [key, value] of Object.entries({
    history, location,
    performance: { getEntriesByType: () => [{ type: navigationType }] },
    sessionStorage: { setItem: values.set.bind(values), getItem: values.get.bind(values) },
  })) {
    const original = Object.getOwnPropertyDescriptor(globalThis, key);
    Object.defineProperty(globalThis, key, { configurable: true, value });
    t.after(() => {
      if (original) Object.defineProperty(globalThis, key, original);
      else Reflect.deleteProperty(globalThis, key);
    });
  }

  const position: ViewPosition = {
    path: '/p', id: 'page', hash: '', documentTop: 100, articleTop: 0,
    anchor: null, heading: null,
  };
  storeViewPosition(position);
  const firstEntry = history.state;
  history.state = null;
  storeViewPosition({ ...position, documentTop: 400 });
  const secondEntry = history.state;
  assert.notDeepEqual(firstEntry, secondEntry);
  assert.equal(readViewPosition('page'), null);

  navigationType = 'back_forward';
  history.state = firstEntry;
  assert.deepEqual(readViewPosition('page'), position);
  assert.equal(readViewPosition('replacement'), null);
  history.state = secondEntry;
  assert.equal(readViewPosition('page')?.documentTop, 400);
  history.state = null;
  assert.equal(readViewPosition('page'), null);

  history.state = firstEntry;
  location.href += '#heading';
  location.hash = '#heading';
  assert.equal(readHistoryViewPosition('page'), null);
  location.href = 'https://wiki.test/wiki/p';
  location.hash = '';
  navigationType = 'reload';
  assert.equal(readViewPosition('page')?.documentTop, 100);
  history.state = null;
  assert.equal(readViewPosition('page')?.documentTop, 400);
});
