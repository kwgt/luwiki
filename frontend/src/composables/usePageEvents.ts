import { webSocketUrl } from '../lib/websocket';
import { ensureHelloAuth } from '../api/hello';
import { onBeforeUnmount, ref, watch } from 'vue';
import { isPageEventState, parsePageEvent, type PageEventState, type PageUpdateMode } from '../lib/pageEvents';

/** ページ機能から接続管理へ渡す境界。 */
interface Options {
  ready: () => boolean;
  mode: () => PageUpdateMode;
  path: () => string;
  pageId: () => string;
  stateToken: () => string;
  observed: (value: PageEventState) => void;
  reload: (viewer: string) => void;
}

/** 一接続の状態通知を閲覧UIへ接続する。 */
export function usePageEvents(options: Options) {
  const notice = ref('');
  const actionLabel = ref('');
  const busy = ref(false);
  const current = ref<PageEventState | null>(null);
  let socket: WebSocket | null = null;
  let generation = 0;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let failures = 0;
  let blocked = false;
  let closedNotice = '';
  let pending: { resolve: (value: PageEventState) => void; reject: () => void; timer: ReturnType<typeof setTimeout> } | null = null;

  /** 古いsocket・再試行・確認要求を解放する。 */
  function stop(): void {
    generation++;
    clearTimeout(retryTimer);
    if (pending) { clearTimeout(pending.timer); pending.reject(); pending = null; }
    const old = socket; socket = null; old?.close();
  }

  /** 表示中の変更通知を一つに集約する。 */
  function display(value: PageEventState): void {
    current.value = value;
    options.observed(value);
    if (value.kind === 'unchanged') { notice.value = ''; actionLabel.value = ''; return; }
    if ((value.kind === 'updated' || value.kind === 'replaced') && options.mode() === 'auto') {
      if (!busy.value) {
        busy.value = true;
        try { options.reload(value.viewer_key); }
        catch { busy.value = false; notice.value = '表示を退避できないため更新を中止しました'; actionLabel.value = '更新'; }
      }
      return;
    }
    if (closedNotice === JSON.stringify(value)) return;
    if (value.kind === 'renamed') {
      notice.value = `ページが移動しました${value.navigation ? '：' + value.navigation.path : ''}`;
      actionLabel.value = value.navigation ? '移動先を開く' : '再確認';
    } else if (value.kind === 'unavailable') {
      notice.value = 'このページは削除されたか、閲覧できない状態になりました';
      actionLabel.value = value.navigation ? '先祖ページを開く' : '再確認';
    } else {
      notice.value = value.kind === 'replaced' ? 'このパスのページが置き換わりました' : 'ページが更新されました';
      actionLabel.value = '更新';
    }
  }

  /** 可視・有効設定のときだけ新しい接続を確立する。 */
  function connect(): void {
    if (!options.ready() || options.mode() === 'ignore' || document.hidden || blocked) return;
    const mine = ++generation;
    const next = new WebSocket(webSocketUrl());
    socket = next;
    next.onopen = () => {
      if (mine !== generation) return;
      next.send(JSON.stringify({ type: 'page.subscribe', payload: {
        path: options.path(), page_id: options.pageId(), expected_state: options.stateToken(),
      } }));
    };
    next.onmessage = event => {
      if (mine !== generation || typeof event.data !== 'string') return;
      const message = parsePageEvent(event.data);
      if (!message) { notice.value = '通知の形式を確認できませんでした'; actionLabel.value = '再確認'; return; }
      if (!isPageEventState(message.payload)) {
        notice.value = message.payload.code === 'forbidden' ? '認証またはアクセス権を確認してください' : '変更を確認できませんでした';
        actionLabel.value = '再確認';
        if (message.payload.code === 'forbidden') { blocked = true; stop(); }
        else if (pending) { clearTimeout(pending.timer); pending.reject(); pending = null; }
        return;
      }
      failures = 0;
      if (message.type === 'page.checked' && pending) {
        const request = pending; pending = null; clearTimeout(request.timer); request.resolve(message.payload);
      } else display(message.payload);
    };
    next.onerror = () => { if (mine === generation) { notice.value = '変更通知に接続できません'; actionLabel.value = '再確認'; } };
    next.onclose = () => {
      if (mine !== generation) return;
      socket = null;
      if (pending) { clearTimeout(pending.timer); pending.reject(); pending = null; }
      notice.value = '変更を確認できませんでした'; actionLabel.value = '再確認';
      const delay = [1, 2, 5, 10, 30][Math.min(failures++, 4)];
      retryTimer = setTimeout(connect, delay * 1000);
    };
  }

  /** 応答と自発通知を区別して現在の案内を確認する。 */
  function check(): Promise<PageEventState> {
    return new Promise((resolve, reject) => {
      if (!socket || socket.readyState !== WebSocket.OPEN || pending) { reject(new Error('not connected')); return; }
      const timer = setTimeout(() => { pending = null; reject(new Error('check timed out')); }, 10000);
      pending = { resolve, reject: () => reject(new Error('check failed')), timer };
      socket.send(JSON.stringify({ type: 'page.check', payload: {} }));
    });
  }

  /** 更新・移動操作は直前の状態を再確認してから行う。 */
  async function act(): Promise<void> {
    if (busy.value) return;
    if (!socket || blocked) {
      try { await ensureHelloAuth(); blocked = false; stop(); connect(); }
      catch { notice.value = '認証または接続状態を確認してください'; }
      return;
    }
    busy.value = true;
    try {
      const previous = current.value;
      const value = await check(); options.observed(value);
      if (options.mode() === 'ignore') return;
      if (value.kind === 'updated' || value.kind === 'replaced') options.reload(value.viewer_key);
      else if (value.navigation && previous?.navigation?.path === value.navigation.path
        && previous.kind === value.kind) {
        location.assign('/wiki' + value.navigation.path.split('/').map(encodeURIComponent).join('/'));
      } else display(value);
    } catch { if (options.mode() !== 'ignore') notice.value = '更新または移動を確認できませんでした'; }
    finally { busy.value = false; }
  }

  /** 現在観測した状態だけを既読にする。 */
  function dismiss(): void {
    closedNotice = JSON.stringify(current.value); notice.value = ''; actionLabel.value = '';
  }

  /** 表示・設定変更時に接続を作り直す。 */
  function restart(): void {
    stop(); busy.value = false;
    if (options.mode() === 'ignore') { notice.value = ''; actionLabel.value = ''; }
    else connect();
  }
  watch([options.ready, options.mode, options.pageId, options.stateToken], restart, { immediate: true });
  document.addEventListener('visibilitychange', restart);
  window.addEventListener('pagehide', stop);
  window.addEventListener('pageshow', restart);
  onBeforeUnmount(() => { document.removeEventListener('visibilitychange', restart); window.removeEventListener('pagehide', stop); window.removeEventListener('pageshow', restart); stop(); });
  return { notice, actionLabel, busy, current, act, dismiss };
}
