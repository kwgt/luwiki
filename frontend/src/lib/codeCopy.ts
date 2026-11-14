import type MarkdownIt from 'markdown-it';
import { copyCodeText, type ClipboardCopyResult } from './clipboard';

// GitHub Octicons copy-16 (MIT)。ライセンスはpublic/licenses/octicons.txtを参照。
// https://github.com/primer/octicons/blob/main/icons/copy-16.svg
const COPY_ICON = '<svg class="octicon octicon-copy" aria-hidden="true" focusable="false" fill="currentColo'
  + 'r" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path d'
  + '="M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 0 1 0 1.5h-1.5a.25.25 0 0 0-.25.25v7.5c0 .13'
  + '8.112.25.25.25h7.5a.25.25 0 0 0 .25-.25v-1.5a.75.75 0 0 1 1.5 0v1.5A1.75 1.75 0 0 1 9.25'
  + ' 16h-7.5A1.75 1.75 0 0 1 0 14.25Z"/><path d="M5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 '
  + '16 .784 16 1.75v7.5A1.75 1.75 0 0 1 14.25 11h-7.5A1.75 1.75 0 0 1 5 9.25Zm1.75-.25a.25.2'
  + '5 0 0 0-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.2'
  + '5Z"/></svg>';

interface CodeCopyState {
  prefix: string;
  blocks: Map<string, string>;
}

export interface CodeCopyRenderResult {
  html: string;
  codeBlocks: ReadonlyMap<string, string>;
}

/**
 * copy用render envを型検証する。
 * @param env Markdown rendererのenv
 * @returns 内部state、または通常renderの場合はnull。
 */
function getState(env: unknown): CodeCopyState | null {
  if (typeof env !== 'object' || env === null || !('codeCopy' in env)) {
    return null;
  }
  const state = env.codeCopy;
  if (typeof state !== 'object' || state === null
    || !('prefix' in state) || typeof state.prefix !== 'string'
    || !('blocks' in state) || !(state.blocks instanceof Map)) {
    return null;
  }
  return state as CodeCopyState;
}

/**
 * copy用envだけにtoolbarを追加する。既存highlight/fence出力は再利用する。
 * @param md 対象renderer
 */
export function installCodeCopyRenderer(md: MarkdownIt): void {
  const fence = md.renderer.rules.fence!;
  md.renderer.rules.fence = (tokens, index, options, env: unknown, self) => {
    const html = fence(tokens, index, options, env, self);
    const state = getState(env);
    if (!state) {
      return html;
    }
    const id = state.prefix + '-' + state.blocks.size;
    state.blocks.set(id, tokens[index].content);
    return '<div class="luwiki-code-block min-w-0">'
      + '<div class="code-copy-toolbar">'
      + '<span class="code-copy-status" role="status" aria-live="polite"></span>'
      + '<button type="button" class="code-copy-button" '
      + 'aria-label="コードをコピー" data-code-copy-id="' + md.utils.escapeHtml(id)
      + '">' + COPY_ICON + '</button></div>' + html + '</div>\n';
  };
}

/**
 * renderごとのコードMapを作る。生成文字列は再処理しない。
 * @param md 対象renderer
 * @param source マクロ展開後の表示本文
 * @param prefix 呼出し元のrender generation
 * @returns HTMLと、このrenderだけのコードMap。
 */
export function renderMarkdownWithCodeCopy(
  md: MarkdownIt, source: string, prefix: string,
): CodeCopyRenderResult {
  const state: CodeCopyState = { prefix, blocks: new Map() };
  return { html: md.render(source, { codeCopy: state }), codeBlocks: state.blocks };
}

/**
 * articleへ一度接続するclick handlerを作る。
 * @param getBlocks 最新renderのMapを返す関数
 * @returns ID/世代とfocusを保持するコピーhandler。
 */
export function createCodeCopyHandler(
  getBlocks: () => ReadonlyMap<string, string>,
): (event: MouseEvent) => Promise<void> {
  const dismissals = new WeakMap<HTMLElement, ReturnType<typeof setTimeout>>();
  return async (event) => {
    const root = event.currentTarget;
    if (!(root instanceof Element) || !(event.target instanceof Element)) {
      return;
    }
    const button = event.target.closest<HTMLButtonElement>('button[data-code-copy-id]');
    if (!button || !root.contains(button)) {
      return;
    }
    const blocks = getBlocks();
    const id = button.dataset.codeCopyId ?? '';
    if (!blocks.has(id)) {
      return;
    }
    event.preventDefault();
    if (button.getAttribute('aria-disabled') === 'true') {
      return;
    }
    const text = blocks.get(id)!;
    const status = button.closest('.luwiki-code-block')
      ?.querySelector<HTMLElement>('.code-copy-status');
    button.setAttribute('aria-disabled', 'true');
    button.classList.add('opacity-60');
    if (status) {
      clearTimeout(dismissals.get(status));
      dismissals.delete(status);
      status.textContent = 'コピー中…';
    }

    let result: ClipboardCopyResult;
    try {
      result = await copyCodeText(text);
    } catch {
      result = 'failed';
    }
    /* 古い要求から新しいDOMやMapを変更しない。 */
    if (getBlocks() !== blocks || !button.isConnected || !root.contains(button)) {
      return;
    }
    button.removeAttribute('aria-disabled');
    button.classList.remove('opacity-60');
    if (status) {
      status.textContent = result === 'copied' ? 'コピーしました'
        : 'コピーできませんでした。ブラウザの権限・接続方式を確認してください。';
      if (result === 'copied') {
        dismissals.set(status, setTimeout(() => {
          dismissals.delete(status);
          if (getBlocks() === blocks && root.contains(button)) {
            status.textContent = '';
          }
        }, 2000));
      }
    }
  };
}
