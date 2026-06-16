import { syntaxTree } from '@codemirror/language';
import { type EditorState } from '@codemirror/state';

export interface FoldRange {
  from: number;
  to: number;
}

/**
 * ソース先頭にある完全な front matter 全体の折りたたみ範囲を返す。
 */
export function getInitialFrontMatterFoldRange(
  state: EditorState,
): FoldRange | null {
  const firstLine = state.doc.line(1);
  return getFrontMatterFoldRange(state, firstLine.from, firstLine.to);
}

/**
 * 指定行を起点とする front matter 全体の折りたたみ範囲を返す。
 */
export function getFrontMatterFoldRange(
  state: EditorState,
  lineStart: number,
  lineEnd: number,
): FoldRange | null {
  const firstLine = state.doc.line(1);
  if (lineStart !== firstLine.from || lineEnd !== firstLine.to) {
    return null;
  }

  const frontMatter = syntaxTree(state).topNode.getChild('Frontmatter');
  if (!frontMatter || frontMatter.from !== firstLine.from) {
    return null;
  }

  const delimiters = frontMatter.getChildren('DashLine');
  const openingDelimiter = delimiters[0];
  const closingDelimiter = delimiters[1];
  if (
    delimiters.length !== 2
    || !openingDelimiter
    || !closingDelimiter
    || openingDelimiter.from !== firstLine.from
  ) {
    return null;
  }

  if (closingDelimiter.to !== frontMatter.to || firstLine.to >= frontMatter.to) {
    return null;
  }

  const closingLine = state.doc.lineAt(closingDelimiter.from);
  return {
    from: firstLine.to,
    to: closingLine.to,
  };
}
