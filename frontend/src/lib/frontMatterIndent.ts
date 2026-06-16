import {
  EditorSelection,
  type EditorState,
  type SelectionRange,
  type StateCommand,
} from '@codemirror/state';
import { type IndentContext } from '@codemirror/language';

function countLeadingSpaces(text: string): number {
  let count = 0;
  while (count < text.length && text[count] === ' ') {
    count += 1;
  }
  return count;
}

function isDelimiterLine(text: string): boolean {
  return text.trim() === '---';
}

function findFrontMatterEndLine(doc: EditorState['doc']): number | null {
  for (let lineNumber = 2; lineNumber <= doc.lines; lineNumber += 1) {
    if (isDelimiterLine(doc.line(lineNumber).text)) {
      return lineNumber;
    }
  }
  return null;
}

function getFrontMatterLineRange(state: EditorState): { start: number; end: number | null } | null {
  if (state.doc.lines === 0 || !isDelimiterLine(state.doc.line(1).text)) {
    return null;
  }

  return {
    start: 1,
    end: findFrontMatterEndLine(state.doc),
  };
}

function isFrontMatterLine(state: EditorState, lineNumber: number): boolean {
  const range = getFrontMatterLineRange(state);
  if (!range) {
    return false;
  }

  if (range.end === null) {
    return lineNumber >= range.start;
  }

  return lineNumber >= range.start && lineNumber <= range.end;
}

export function isInFrontMatter(state: EditorState, pos: number): boolean {
  const line = state.doc.lineAt(pos);
  return isFrontMatterLine(state, line.number);
}

function isFrontMatterSelection(state: EditorState, range: SelectionRange): boolean {
  const startLine = state.doc.lineAt(range.from).number;
  const endLine = state.doc.lineAt(range.to).number;

  for (let lineNumber = startLine; lineNumber <= endLine; lineNumber += 1) {
    if (!isFrontMatterLine(state, lineNumber)) {
      return false;
    }
  }

  return true;
}

function findPreviousContentLine(doc: EditorState['doc'], lineNumber: number): string | null {
  for (let current = lineNumber - 1; current >= 1; current -= 1) {
    const text = doc.line(current).text;
    const trimmed = text.trim();
    if (trimmed.length === 0 || trimmed.startsWith('#')) {
      continue;
    }
    return text;
  }
  return null;
}

function isYamlKeyOnlyLine(text: string): boolean {
  return /^[ ]*([A-Za-z_][\w.-]*|-)\s*:\s*(#.*)?$/.test(text);
}

function isYamlListItemLine(text: string): boolean {
  return /^[ ]*-\s+/.test(text);
}

function isYamlInlineObjectListItemLine(text: string): boolean {
  return /^[ ]*-\s+[A-Za-z_][\w.-]*\s*:\s*\S.*$/.test(text);
}

function normalizeIndent(indent: number): number {
  return indent >= 0 ? indent - (indent % 2) : 0;
}

function getFrontMatterBreakIndentation(context: IndentContext, pos: number): number | undefined {
  if (!isInFrontMatter(context.state, pos)) {
    return undefined;
  }

  const line = context.lineAt(pos, -1);
  if (isDelimiterLine(line.text)) {
    return 0;
  }

  const trimmed = line.text.trim();
  const currentIndent = countLeadingSpaces(line.text);
  if (trimmed.length === 0) {
    return getFrontMatterIndentation(context.state, pos) ?? 0;
  }
  if (isYamlKeyOnlyLine(line.text)) {
    return currentIndent + 2;
  }
  if (isYamlInlineObjectListItemLine(line.text)) {
    return currentIndent + 2;
  }
  if (isYamlListItemLine(line.text)) {
    return currentIndent;
  }
  return normalizeIndent(currentIndent);
}

function buildFrontMatterIndentChanges(
  state: EditorState,
  delta: number,
) {
  return state.changeByRange((range) => {
    if (!isFrontMatterSelection(state, range)) {
      return {
        changes: [],
        range,
      };
    }

    const startLine = state.doc.lineAt(range.from).number;
    const endLine = state.doc.lineAt(range.to).number;
    const changes: { from: number; to?: number; insert: string }[] = [];
    let primaryLineIndentDelta = 0;
    let primaryLineFrom = -1;
    let primaryLineCurrentIndent = 0;

    for (let lineNumber = startLine; lineNumber <= endLine; lineNumber += 1) {
      const line = state.doc.line(lineNumber);
      const currentIndent = countLeadingSpaces(line.text);
      const nextIndent = Math.max(0, currentIndent + delta);
      const normalizedIndent = normalizeIndent(nextIndent);
      if (normalizedIndent === currentIndent) {
        continue;
      }

      if (range.empty && range.head >= line.from && range.head <= line.to) {
        primaryLineIndentDelta = normalizedIndent - currentIndent;
        primaryLineFrom = line.from;
        primaryLineCurrentIndent = currentIndent;
      }

      changes.push({
        from: line.from,
        to: line.from + currentIndent,
        insert: ' '.repeat(normalizedIndent),
      });
    }

    let mappedRange = changes.length > 0
      ? range.map(state.changes(changes), delta > 0 ? 1 : -1)
      : range;

    if (range.empty && primaryLineFrom >= 0) {
      const cursorOffset = Math.max(0, range.head - primaryLineFrom);
      const nextCursor = cursorOffset <= primaryLineCurrentIndent
        ? primaryLineFrom + Math.max(0, cursorOffset + primaryLineIndentDelta)
        : range.head + primaryLineIndentDelta;
      mappedRange = EditorSelection.cursor(Math.max(primaryLineFrom, nextCursor));
    }

    return {
      changes,
      range: mappedRange,
    };
  });
}

export function getFrontMatterIndentation(state: EditorState, pos: number): number | undefined {
  if (!isInFrontMatter(state, pos)) {
    return undefined;
  }

  const line = state.doc.lineAt(pos);
  const trimmed = line.text.trim();
  if (isDelimiterLine(line.text)) {
    return 0;
  }

  const currentIndent = countLeadingSpaces(line.text);
  if (trimmed.length > 0) {
    return normalizeIndent(currentIndent);
  }

  const previousLine = findPreviousContentLine(state.doc, line.number);
  if (!previousLine || isDelimiterLine(previousLine)) {
    return 0;
  }

  const previousIndent = countLeadingSpaces(previousLine);
  if (isYamlKeyOnlyLine(previousLine)) {
    return previousIndent + 2;
  }
  if (isYamlInlineObjectListItemLine(previousLine)) {
    return previousIndent + 2;
  }
  if (isYamlListItemLine(previousLine)) {
    return previousIndent;
  }
  return normalizeIndent(previousIndent);
}

export function getFrontMatterIndentationForContext(
  context: IndentContext,
  pos: number,
): number | undefined {
  if (context.simulatedBreak === pos) {
    return getFrontMatterBreakIndentation(context, pos);
  }

  return getFrontMatterIndentation(context.state, pos);
}

export const indentFrontMatterMore: StateCommand = ({ state, dispatch }) => {
  if (state.readOnly || !isFrontMatterSelection(state, state.selection.main)) {
    return false;
  }

  dispatch(state.update(buildFrontMatterIndentChanges(state, 2), {
    userEvent: 'input.indent',
  }));
  return true;
};

export const indentFrontMatterLess: StateCommand = ({ state, dispatch }) => {
  if (state.readOnly || !isFrontMatterSelection(state, state.selection.main)) {
    return false;
  }

  dispatch(state.update(buildFrontMatterIndentChanges(state, -2), {
    userEvent: 'delete.dedent',
  }));
  return true;
};
