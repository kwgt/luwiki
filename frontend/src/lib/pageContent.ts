export function stripFrontMatter(markdown: string): string {
  if (
    markdown !== '---'
    && !markdown.startsWith('---\n')
    && !markdown.startsWith('---\r\n')
  ) {
    return markdown;
  }

  const openDelimiterLength = markdown === '---'
    ? 3
    : markdown.startsWith('---\r\n')
      ? 5
      : 4;
  const closingIndex = findClosingDelimiter(markdown, openDelimiterLength);
  if (closingIndex < 0) {
    return markdown;
  }

  const closingLineBreakIndex = markdown.indexOf('\n', closingIndex);
  if (closingLineBreakIndex < 0) {
    return '';
  }

  return markdown.slice(closingLineBreakIndex + 1);
}

export function stripLeadingTitleHeading(markdown: string): string {
  const lines = markdown.split(/\r?\n/);
  let index = 0;

  while (index < lines.length && lines[index].trim().length === 0) {
    index += 1;
  }

  if (index >= lines.length || !/^#\s+.+$/.test(lines[index])) {
    return markdown;
  }

  const remaining = lines.slice(index + 1);
  return remaining.join('\n').replace(/^\n+/, '');
}

function findClosingDelimiter(markdown: string, searchStart: number): number {
  let index = searchStart;

  while (index < markdown.length) {
    const lineEnd = markdown.indexOf('\n', index);
    const endIndex = lineEnd >= 0 ? lineEnd : markdown.length;
    const line = markdown.slice(index, endIndex).replace(/\r$/, '');
    if (line === '---') {
      return index;
    }
    if (lineEnd < 0) {
      break;
    }
    index = lineEnd + 1;
  }

  return -1;
}
