export type ClipboardCopyResult = 'copied' | 'unavailable' | 'failed';

/**
 * ユーザ操作中にテキストをコピーする。本文や生例外は記録しない。
 * @param text コピーする表示文字列
 * @returns 成功/未対応/失敗。明示的なAPI拒否後はfallbackしない。
 */
export async function copyCodeText(text: string): Promise<ClipboardCopyResult> {
  if (window.isSecureContext && typeof navigator.clipboard?.writeText === 'function') {
    try {
      await navigator.clipboard.writeText(text);
      return 'copied';
    } catch {
      return 'failed';
    }
  }
  if (typeof document.execCommand !== 'function') {
    return 'unavailable';
  }

  /* fallbackはawaitより前に同期実行し、user activationを保持する。 */
  const active = document.activeElement;
  const selection = window.getSelection();
  const ranges = selection ? Array.from({ length: selection.rangeCount },
    (_, index) => selection.getRangeAt(index).cloneRange()) : [];
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.readOnly = true;
  textarea.tabIndex = -1;
  textarea.setAttribute('aria-hidden', 'true');
  textarea.style.cssText = 'position:fixed;left:-10000px;top:0;opacity:0';
  try {
    document.body.append(textarea);
    textarea.focus({ preventScroll: true });
    textarea.select();
    return document.execCommand('copy') ? 'copied' : 'failed';
  } catch {
    return 'failed';
  } finally {
    textarea.remove();
    if (active instanceof HTMLElement && active.isConnected) {
      active.focus({ preventScroll: true });
    }
    if (selection) {
      selection.removeAllRanges();
      for (const range of ranges) {
        if (range.commonAncestorContainer.isConnected) {
          selection.addRange(range);
        }
      }
    }
  }
}
