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
