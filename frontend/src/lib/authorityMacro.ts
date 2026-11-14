const AUTHORITY_MACRO = '{{mcp:authority}}';

/**
 * 元sourceのauthority専用記法を一回展開する。
 * code内も対象。奇数個の直前バックスラッシュは展開を抑止する。
 * @param source 展開元本文
 * @param authority 解決済みauthority。未取得なら原文を返す。
 * @returns 展開後の文字列。元のバックスラッシュ・改行は保持する。
 */
export function expandAuthorityMacro(
  source: string,
  authority?: string | null,
): string {
  if (!authority) {
    return source;
  }

  /* 元のsourceの一致だけを走査し、生成結果は再処理しない。 */
  let search = 0;
  let copied = 0;
  const output: string[] = [];
  while (search < source.length) {
    const start = source.indexOf(AUTHORITY_MACRO, search);
    if (start < 0) {
      break;
    }
    const end = start + AUTHORITY_MACRO.length;
    let escapes = 0;
    for (let i = start - 1; i >= 0 && source[i] === '\\'; i -= 1) {
      escapes += 1;
    }
    if (escapes % 2 === 0) {
      output.push(source.slice(copied, start), authority);
      copied = end;
    }
    search = end;
  }
  if (output.length === 0) {
    return source;
  }
  output.push(source.slice(copied));
  return output.join('');
}
