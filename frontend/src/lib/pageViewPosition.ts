/** 内容の同一性と可視基準からの位置。 */
interface ContentAnchor { tag: string; text: string; offset: number }
/** 同じページだけに復元する記録。 */
export interface ViewPosition {
  path: string; id: string; hash: string;
  documentTop: number; articleTop: number;
  anchor: ContentAnchor | null; heading: ContentAnchor | null;
}
const POSITION_KEY = 'luwiki-view-position-v1';
const BLOCKS = 'h1,h2,h3,h4,h5,h6,p,li,pre,blockquote,table';

/** URLをWikiの正規形pathへ一度だけ解釈する。 */
export function currentViewPath(): string {
  try { return decodeURIComponent(location.pathname.replace(/^\/wiki/, '') || '/'); }
  catch { return '/'; }
}
/** コピー等の操作要素を除いた内容識別。 */
function textOf(element: Element): string {
  const copy = element.cloneNode(true) as Element;
  copy.querySelectorAll('button,[role="status"]').forEach(node => node.remove());
  return (copy.textContent ?? '').replace(/\s+/g, ' ').trim();
}
/** 現在の実scroll領域を判定する。 */
function internal(article: HTMLElement): boolean {
  return article.scrollHeight > article.clientHeight + 1 && ['auto', 'scroll'].includes(getComputedStyle(article).overflowY);
}
/** viewportと本文の交差領域上端。 */
function visibleTop(article: HTMLElement): number {
  return Math.max(0, article.getBoundingClientRect().top + article.clientTop);
}
/** 一意に残る内容だけを基準とする。 */
function anchorOf(element: Element | undefined, candidates: Element[], top: number): ContentAnchor | null {
  if (!element) return null;
  const text = textOf(element);
  if (!text || text.length > 2048 || candidates.filter(other => other.tagName === element.tagName && textOf(other) === text).length !== 1) return null;
  return { tag: element.tagName, text, offset: element.getBoundingClientRect().top - top };
}
/** 差し替え・reload直前の読書位置を取得する。 */
export function captureViewPosition(article: HTMLElement, id: string): ViewPosition {
  const top = visibleTop(article);
  const candidates = [...article.querySelectorAll(BLOCKS)];
  const visible = candidates.filter(element => element.getBoundingClientRect().bottom > top);
  const atTop = visible.filter(element => element.getBoundingClientRect().top <= top + 24);
  const block = atTop.reverse().find(element => !element.querySelector(BLOCKS)) ?? atTop[0] ?? visible[0];
  const heading = candidates.filter(element => /^H[1-6]$/.test(element.tagName) && element.getBoundingClientRect().top <= (block?.getBoundingClientRect().top ?? top)).pop();
  return { path: currentViewPath(), id, hash: location.hash,
    documentTop: document.scrollingElement?.scrollTop ?? window.scrollY,
    articleTop: article.scrollTop,
    anchor: anchorOf(block, candidates, top), heading: anchorOf(heading, candidates, top) };
}
/** private storageの破損値を検証する。 */
export function isViewPosition(value: unknown): value is ViewPosition {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as Record<string, unknown>;
  const anchor = (a: unknown): boolean => a === null || (typeof a === 'object' && a !== null
    && typeof (a as ContentAnchor).tag === 'string' && typeof (a as ContentAnchor).text === 'string'
    && Number.isFinite((a as ContentAnchor).offset));
  return typeof v.path === 'string' && typeof v.id === 'string' && typeof v.hash === 'string'
    && typeof v.documentTop === 'number' && Number.isFinite(v.documentTop) && v.documentTop >= 0
    && typeof v.articleTop === 'number' && Number.isFinite(v.articleTop) && v.articleTop >= 0
    && anchor(v.anchor) && anchor(v.heading);
}
/** 最大scroll量へ補正する。 */
export function clampedScroll(saved: number, scrollHeight: number, clientHeight: number): number {
  return Math.max(0, Math.min(saved, scrollHeight - clientHeight));
}
/** 通知リロード用の一時パラメータを除いた履歴 URL。 */
function viewEntryUrl(): string {
  const url = new URL(location.href);
  url.searchParams.delete('_view_reload');
  return url.pathname + url.search + url.hash;
}

/** URL が一致する現在の履歴エントリに対応する保存先を返す。 */
function viewEntryKey(): string | null {
  const entry: unknown = history.state?.luwikiViewPosition;
  if (typeof entry !== 'object' || entry === null) return null;
  const value = entry as { id?: unknown; url?: unknown };
  return typeof value.id === 'string' && /^\d+(-\d+){3}$/.test(value.id)
    && value.url === viewEntryUrl() ? POSITION_KEY + ':' + value.id : null;
}

/** リロード用と、履歴の戻る・進む用に閲覧位置を保存する。 */
export function storeViewPosition(value: ViewPosition): void {
  const serialized = JSON.stringify(value);
  sessionStorage.setItem(POSITION_KEY, serialized);
  try {
    let key = viewEntryKey();
    if (!key) {
      const id = crypto.getRandomValues(new Uint32Array(4)).join('-');
      history.replaceState({
        ...history.state, luwikiViewPosition: { id, url: viewEntryUrl() },
      }, '');
      key = POSITION_KEY + ':' + id;
    }
    sessionStorage.setItem(key, serialized);
  } catch { /* 履歴の保存が失敗してもリロード用の記録は保持する。 */ }
}

/** 同じページ・ハッシュに対応する有効な位置だけを返す。 */
function readStoredPosition(key: string | null, id: string): ViewPosition | null {
  if (!key) return null;
  try {
    const value: unknown = JSON.parse(sessionStorage.getItem(key) ?? 'null');
    return isViewPosition(value) && value.id === id
      && value.path === currentViewPath() && value.hash === location.hash ? value : null;
  } catch { return null; }
}

/** 履歴エントリの位置を取得する。 */
export function readHistoryViewPosition(id: string): ViewPosition | null {
  return readStoredPosition(viewEntryKey(), id);
}

/** 文書をリロードした場合や履歴から再読込した場合の位置を返す。 */
export function readViewPosition(id: string): ViewPosition | null {
  const entry = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined;
  if (entry?.type === 'back_forward') return readHistoryViewPosition(id);
  if (entry?.type !== 'reload') return null;
  return readHistoryViewPosition(id) ?? readStoredPosition(POSITION_KEY, id);
}
/** 基準・fallback量を現在レイアウトへ適用する。 */
export function applyViewPosition(article: HTMLElement, position: ViewPosition): void {
  const documentScroller = document.scrollingElement;
  if (documentScroller) documentScroller.scrollTop = clampedScroll(position.documentTop, documentScroller.scrollHeight, documentScroller.clientHeight);
  article.scrollTop = clampedScroll(position.articleTop, article.scrollHeight, article.clientHeight);
  const candidates = [...article.querySelectorAll(BLOCKS)];
  for (const anchor of [position.anchor, position.heading]) {
    if (!anchor) continue;
    const matching = candidates.filter(element => element.tagName === anchor.tag && textOf(element) === anchor.text);
    if (matching.length !== 1) continue;
    const delta = matching[0].getBoundingClientRect().top - visibleTop(article) - anchor.offset;
    if (internal(article)) article.scrollTop = clampedScroll(article.scrollTop + delta, article.scrollHeight, article.clientHeight);
    else if (documentScroller) documentScroller.scrollTop = clampedScroll(documentScroller.scrollTop + delta, documentScroller.scrollHeight, documentScroller.clientHeight);
    return;
  }
}
/** 描画後の高さ変化へ期限付きで補正し、解放関数を返す。 */
export function restoreViewPosition(article: HTMLElement, position: ViewPosition, finished: () => void): () => void {
  let quiet: ReturnType<typeof setTimeout> | undefined;
  let frame = 0;
  let stopped = false;
  const images = [...article.querySelectorAll('img')];
  const previousAnchoring = article.style.overflowAnchor;
  article.style.overflowAnchor = 'none';
  /** 記録を上書きする前に全監視を解放する。 */
  function stop(): void {
    if (stopped) return;
    stopped = true; clearTimeout(quiet); clearTimeout(deadline); cancelAnimationFrame(frame); observer.disconnect();
    images.forEach(image => { image.removeEventListener('load', update); image.removeEventListener('error', update); });
    ['wheel', 'touchstart', 'pointerdown', 'keydown', 'hashchange'].forEach(type => window.removeEventListener(type, stop, true));
    article.style.overflowAnchor = previousAnchoring;
    finished();
  }
  /** 高さ変化を次フレームへまとめる。 */
  function update(): void {
    if (stopped) return;
    clearTimeout(quiet); cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      if (stopped) return;
      applyViewPosition(article, position);
      if (images.every(image => image.complete)) quiet = setTimeout(stop, 500);
    });
  }
  const observer = new ResizeObserver(update);
  observer.observe(article.firstElementChild ?? article);
  const deadline = setTimeout(stop, 5000);
  images.forEach(image => { image.addEventListener('load', update); image.addEventListener('error', update); });
  ['wheel', 'touchstart', 'pointerdown', 'keydown', 'hashchange'].forEach(type => window.addEventListener(type, stop, true));
  update();
  return stop;
}
