import test from 'node:test';
import assert from 'node:assert/strict';
import { createRenderer, nextTick, ref } from 'vue';
import { usePageEvents } from './usePageEvents';
import type { PageEventState, PageUpdateMode } from '../lib/pageEvents';

/** 通信の順序・切断を試験側から制御する接続。 */
class TestSocket {
  static OPEN = 1;
  static connections: TestSocket[] = [];
  readyState = 1;
  closed = false;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(readonly url: string) { TestSocket.connections.push(this); }
  send(text: string): void { this.sent.push(text); }
  close(): void { this.closed = true; this.readyState = 3; }
  receive(type: string, payload: unknown): void { this.onmessage?.({ data: JSON.stringify({ type, payload }) }); }
}

/** DOMに依存せず、実際のVueのwatchとunmountを実行する。 */
function fixture() {
  const previous = new Map(['document', 'window', 'location', 'WebSocket'].map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  const documentEvents = Object.assign(new EventTarget(), { hidden: false });
  const windowEvents = new EventTarget();
  const assigned: string[] = [];
  Object.assign(globalThis, {
    document: documentEvents, window: windowEvents,
    location: { protocol: 'http:', host: 'localhost:1234', assign: (url: string) => assigned.push(url) },
    WebSocket: TestSocket,
  });
  TestSocket.connections = [];
  const mode = ref<PageUpdateMode>('manual');
  const pageId = ref('page');
  const observed: PageEventState[] = [];
  const reloads: string[] = [];
  const renderer = createRenderer<object, object>({
    patchProp() {}, insert() {}, remove() {}, createElement: () => ({}),
    createText: () => ({}), createComment: () => ({}), setText() {},
    setElementText() {}, parentNode: () => null, nextSibling: () => null,
  });
  let events!: ReturnType<typeof usePageEvents>;
  const app = renderer.createApp({ setup() {
    events = usePageEvents({ ready: () => true, mode: () => mode.value,
      path: () => '/p', pageId: () => pageId.value, stateToken: () => 'baseline',
      observed: value => observed.push(value), reload: viewer => reloads.push(viewer),
    });
    return () => null;
  } });
  app.mount({});
  return { mode, pageId, observed, reloads, assigned, events, documentEvents, windowEvents,
    dispose() {
      app.unmount();
      for (const [key, descriptor] of previous) {
        if (descriptor) Object.defineProperty(globalThis, key, descriptor);
        else Reflect.deleteProperty(globalThis, key);
      }
    },
  };
}

/** 表示済み基準と異なる状態を生成する。 */
function state(revision = 2): PageEventState {
  return { kind: 'updated', viewer_key: 'viewer', navigation: null,
    page_state: { id: 'page', revision, instance_id: null, state_token: 'v1:' + 'a'.repeat(64) + ':' + String(revision).repeat(64) } };
}

test('通知を閉じても未反映状態を維持し、新しい変更は再通知する', () => {
  const f = fixture();
  try {
    const socket = TestSocket.connections[0]; socket.onopen?.();
    assert.equal(socket.url, 'ws://localhost:1234/ws');
    assert.equal(JSON.parse(socket.sent[0]).type, 'page.subscribe');
    socket.receive('page.state', state());
    f.events.dismiss();
    socket.receive('page.changed', state());
    assert.equal(f.events.notice.value, '');
    assert.equal(f.events.current.value?.page_state?.revision, 2);
    socket.receive('page.changed', state(3));
    assert.equal(f.events.notice.value, 'ページが更新されました');
  } finally { f.dispose(); }
});

test('確認中の自発通知では移動せず、確認応答の現在案内を再評価する', async () => {
  const f = fixture();
  try {
    const socket = TestSocket.connections[0];
    const renamed = { ...state(), kind: 'renamed', page_state: null, navigation: { kind: 'renamed', path: '/moved' } };
    socket.receive('page.state', renamed);
    const action = f.events.act();
    assert.equal(JSON.parse(socket.sent[socket.sent.length - 1]).type, 'page.check');
    socket.receive('page.changed', { ...renamed, navigation: { kind: 'renamed', path: '/newer' } });
    assert.deepEqual(f.assigned, []);
    socket.receive('page.checked', { ...renamed, navigation: { kind: 'renamed', path: '/newer' } });
    await action;
    assert.deepEqual(f.assigned, []);
    const confirmed = f.events.act();
    socket.receive('page.checked', { ...renamed, navigation: { kind: 'renamed', path: '/newer' } });
    await confirmed;
    assert.deepEqual(f.assigned, ['/wiki/newer']);
  } finally { f.dispose(); }
});

test('連続自動通知は一回のreloadに集約し、無視へ切り替えると接続を解放する', async () => {
  const f = fixture();
  try {
    f.mode.value = 'auto'; await nextTick();
    const socket = TestSocket.connections[TestSocket.connections.length - 1];
    socket.receive('page.state', state()); socket.receive('page.changed', state(3));
    assert.deepEqual(f.reloads, ['viewer']);
    f.mode.value = 'ignore'; await nextTick();
    assert.ok(socket.closed);
    socket.receive('page.changed', state(4));
    assert.deepEqual(f.reloads, ['viewer']);
    assert.equal(f.events.notice.value, '');
  } finally { f.dispose(); }
});

test('旧接続の応答を棄却し、確認中の切断・非表示・離脱・unmountで解放する', async () => {
  const f = fixture();
  try {
    const old = TestSocket.connections[0];
    const action = f.events.act();
    f.pageId.value = 'new-page'; await nextTick(); await action;
    assert.ok(old.closed);
    old.receive('page.checked', state());
    assert.deepEqual(f.reloads, []);
    assert.deepEqual(f.observed, []);
    const current = TestSocket.connections[TestSocket.connections.length - 1];
    const disconnected = f.events.act(); current.onclose?.(); await disconnected;
    assert.deepEqual(f.reloads, []);
    f.documentEvents.hidden = true;
    f.documentEvents.dispatchEvent(new Event('visibilitychange'));
    const count = TestSocket.connections.length;
    f.documentEvents.hidden = false;
    f.documentEvents.dispatchEvent(new Event('visibilitychange'));
    assert.equal(TestSocket.connections.length, count + 1);
    const visible = TestSocket.connections[TestSocket.connections.length - 1];
    f.windowEvents.dispatchEvent(new Event('pagehide'));
    assert.ok(visible.closed);
    f.windowEvents.dispatchEvent(new Event('pageshow'));
    assert.equal(TestSocket.connections.length, count + 2);
  } finally { f.dispose(); }
  assert.ok(TestSocket.connections[TestSocket.connections.length - 1].closed);
});
