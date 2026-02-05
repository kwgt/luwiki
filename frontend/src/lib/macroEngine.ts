import { fetchPageList } from '../api/pages';
import { fetchPageAssets } from '../api/assets';
import {
  parseAssetSpec,
  resolveAssetUrl,
  resolvePagePath,
  slugifyHeading,
} from './pageCommon';

export interface MacroContext {
  pagePath: string;
  pageId?: string;
  userId?: string;
  userDisplayName?: string;
}

interface ParsedMacro {
  name: string;
  args: string[];
}

interface MacroArgs {
  flags: Set<string>;
  values: Record<string, string>;
}

interface Change {
  from: number;
  to: number;
  insert: string;
}

const INLINE_MACRO_RE = /\{\{([^{}\n]+)\}\}/g;
const FENCE_START_RE = /^(?:```|~~~)/;

export function collectImmediateMacroChanges(
  source: string,
  context: MacroContext,
): Change[] {
  const masked = maskCodeRegions(source);
  const changes: Change[] = [];
  for (const match of masked.text.matchAll(INLINE_MACRO_RE)) {
    const raw = match[0];
    const expr = match[1];
    const from = match.index ?? -1;
    if (from < 0) {
      continue;
    }
    const parsed = parseMacro(expr);
    if (!parsed) {
      continue;
    }
    const replacement = expandImmediateMacro(parsed, context);
    if (replacement === null) {
      continue;
    }
    changes.push({
      from,
      to: from + raw.length,
      insert: replacement,
    });
  }
  return changes;
}

export async function expandRenderMacros(
  source: string,
  context: MacroContext,
): Promise<string> {
  const maskedSpecial = maskCodeRegions(source);
  const withSpecial = maskedSpecial.restore(expandSpecialMacros(maskedSpecial.text));
  const maskedInline = maskCodeRegions(withSpecial);

  const replaced = await replaceInlineMacros(maskedInline.text, async (parsed, raw) => {
    switch (parsed.name) {
      case 'children':
        return expandChildrenMacro(parsed.args, context, raw);
      case 'toc':
        return expandTocMacro(parsed.args, maskedInline.text, raw);
      case 'include_code':
        return expandIncludeCodeMacro(parsed.args, context, raw);
      case 'include_csv':
        return expandIncludeCsvMacro(parsed.args, context, raw);
      default:
        return raw;
    }
  });

  return maskedInline.restore(replaced);
}

function parseMacro(expr: string): ParsedMacro | null {
  const tokens = expr
    .split(':')
    .map((token) => token.trim())
    .filter((token) => token.length > 0);
  if (tokens.length === 0) {
    return null;
  }
  const name = tokens[0].toLowerCase();
  const args = normalizeArgsForMacro(name, tokens.slice(1));
  return {
    name,
    args,
  };
}

function normalizeArgsForMacro(name: string, args: string[]): string[] {
  if (name !== 'include_code' && name !== 'include_csv') {
    return args;
  }

  const merged: string[] = [];
  let current = '';
  let currentIsSrc = false;

  for (const arg of args) {
    const lower = arg.toLowerCase();
    const startsNew = lower.startsWith('src=')
      || lower.startsWith('s=')
      || lower.startsWith('lang=')
      || lower.startsWith('l=');

    if (startsNew) {
      if (current) {
        merged.push(current);
      }
      current = arg;
      currentIsSrc = lower.startsWith('src=') || lower.startsWith('s=');
      continue;
    }

    if (current && currentIsSrc) {
      current = `${current}:${arg}`;
      continue;
    }

    if (current) {
      merged.push(current);
      current = '';
    }
    merged.push(arg);
  }

  if (current) {
    merged.push(current);
  }

  return merged;
}

function parseMacroArgs(args: string[]): MacroArgs {
  const flags = new Set<string>();
  const values: Record<string, string> = {};

  for (const rawArg of args) {
    const arg = rawArg.trim();
    if (!arg) {
      continue;
    }
    const eqIndex = arg.indexOf('=');
    if (eqIndex < 0) {
      flags.add(arg.toLowerCase());
      continue;
    }

    const key = arg.slice(0, eqIndex).trim().toLowerCase();
    const value = arg.slice(eqIndex + 1).trim();
    if (!key || !value) {
      continue;
    }
    values[key] = value;
  }

  return { flags, values };
}

function expandImmediateMacro(
  parsed: ParsedMacro,
  context: MacroContext,
): string | null {
  const args = parseMacroArgs(parsed.args);
  switch (parsed.name) {
    case 'now':
      return formatNow(args);
    case 'today':
      return formatToday(args);
    case 'page':
      return expandPageMacro(args, context);
    case 'user':
      return expandUserMacro(args, context);
    default:
      return null;
  }
}

function formatNow(args: MacroArgs): string {
  const useUtc = hasFlag(args, ['utc']);
  const useIso = hasFlag(args, ['iso8601', 'iso']);
  const now = new Date();

  if (useIso) {
    return now.toISOString().replace(/\.\d{3}Z$/, 'Z');
  }

  if (useUtc) {
    return `${pad4(now.getUTCFullYear())}/${pad2(now.getUTCMonth() + 1)}/${pad2(now.getUTCDate())} ${pad2(now.getUTCHours())}:${pad2(now.getUTCMinutes())}:${pad2(now.getUTCSeconds())}`;
  }

  return `${pad4(now.getFullYear())}/${pad2(now.getMonth() + 1)}/${pad2(now.getDate())} ${pad2(now.getHours())}:${pad2(now.getMinutes())}:${pad2(now.getSeconds())}`;
}

function formatToday(args: MacroArgs): string {
  const useUtc = hasFlag(args, ['utc']);
  const useIso = hasFlag(args, ['iso8601', 'iso']);
  const now = new Date();

  if (useIso) {
    return now.toISOString().slice(0, 10);
  }

  if (useUtc) {
    return `${pad4(now.getUTCFullYear())}/${pad2(now.getUTCMonth() + 1)}/${pad2(now.getUTCDate())}`;
  }

  return `${pad4(now.getFullYear())}/${pad2(now.getMonth() + 1)}/${pad2(now.getDate())}`;
}

function expandPageMacro(args: MacroArgs, context: MacroContext): string {
  if (hasFlag(args, ['id'])) {
    return context.pageId && context.pageId.length > 0
      ? context.pageId
      : context.pagePath;
  }
  return context.pagePath;
}

function expandUserMacro(args: MacroArgs, context: MacroContext): string | null {
  const useDisplay = hasFlag(args, ['display', 'd']);
  if (useDisplay) {
    if (!context.userDisplayName || context.userDisplayName.length === 0) {
      return null;
    }
    return context.userDisplayName;
  }

  if (!context.userId || context.userId.length === 0) {
    return null;
  }
  return context.userId;
}

async function expandChildrenMacro(
  rawArgs: string[],
  context: MacroContext,
  fallback: string,
): Promise<string> {
  const args = parseMacroArgs(rawArgs);
  const recursive = hasFlag(args, ['recursive', 'r']);
  const depthRaw = getValue(args, ['depth', 'd']);

  if (recursive && depthRaw !== null) {
    return fallback;
  }

  let depth = 1;
  if (recursive) {
    depth = Number.MAX_SAFE_INTEGER;
  } else if (depthRaw !== null) {
    const parsed = Number.parseInt(depthRaw, 10);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      return fallback;
    }
    depth = parsed;
  }

  try {
    const items = await listDescendantPages(context.pagePath);
    const baseDepth = countPathSegments(context.pagePath);
    const filtered = items
      .map((item) => item.path)
      .filter((path) => {
        const relDepth = countPathSegments(path) - baseDepth;
        return relDepth > 0 && relDepth <= depth;
      });

    if (filtered.length === 0) {
      return '';
    }

    return filtered
      .map((path) => `- [${escapeMarkdownText(extractPageName(path))}](${path})`)
      .join('\n');
  } catch {
    return fallback;
  }
}

function expandTocMacro(
  rawArgs: string[],
  source: string,
  fallback: string,
): string {
  const args = parseMacroArgs(rawArgs);
  const depthRaw = getValue(args, ['depth', 'd']);
  let depth = 3;

  if (depthRaw !== null) {
    const parsed = Number.parseInt(depthRaw, 10);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      return fallback;
    }
    depth = parsed;
  }

  const maxLevel = Math.min(6, 1 + depth);
  const usedSlugs: Record<string, true> = {};
  const lines = source.split(/\r?\n/);
  const items: Array<{ level: number; text: string; anchor: string }> = [];

  for (const line of lines) {
    const match = line.match(/^(#{2,6})\s+(.+)$/);
    if (!match) {
      continue;
    }
    const level = match[1].length;
    if (level > maxLevel) {
      continue;
    }
    const text = match[2].trim();
    const baseSlug = slugifyHeading(text);
    const anchor = createUniqueSlug(baseSlug, usedSlugs);
    items.push({ level, text, anchor });
  }

  if (items.length === 0) {
    return '';
  }

  return items
    .map((item) => {
      const indent = '  '.repeat(Math.max(0, item.level - 2));
      return `${indent}- [${escapeMarkdownText(item.text)}](#${item.anchor})`;
    })
    .join('\n');
}

async function expandIncludeCodeMacro(
  rawArgs: string[],
  context: MacroContext,
  fallback: string,
): Promise<string> {
  const args = parseMacroArgs(rawArgs);
  const src = getValue(args, ['src', 's']);
  if (!src) {
    return fallback;
  }

  const response = await loadAssetText(context, src);
  if (!response || !response.mimeType.startsWith('text/')) {
    return fallback;
  }

  const lang = getValue(args, ['lang', 'l']) ?? languageFromMime(response.mimeType);
  const fence = response.text.includes('```') ? '~~~' : '```';
  const suffix = lang ? lang : '';
  return `${fence}${suffix}\n${response.text}\n${fence}`;
}

async function expandIncludeCsvMacro(
  rawArgs: string[],
  context: MacroContext,
  fallback: string,
): Promise<string> {
  const args = parseMacroArgs(rawArgs);
  const src = getValue(args, ['src', 's']);
  if (!src) {
    return fallback;
  }

  const response = await loadAssetText(context, src);
  if (!response || response.mimeType !== 'text/csv') {
    return fallback;
  }

  const rows = parseCsv(response.text);
  if (rows.length === 0) {
    return '';
  }

  const normalized = normalizeRowWidths(rows);
  const header = normalized[0];
  const body = normalized.slice(1);
  const separator = header.map(() => '---');

  const tableLines = [
    toMarkdownTableRow(header),
    toMarkdownTableRow(separator),
    ...body.map((row) => toMarkdownTableRow(row)),
  ];

  return tableLines.join('\n');
}

async function replaceInlineMacros(
  source: string,
  handler: (parsed: ParsedMacro, raw: string) => Promise<string> | string,
): Promise<string> {
  let result = '';
  let lastIndex = 0;

  for (const match of source.matchAll(INLINE_MACRO_RE)) {
    const raw = match[0];
    const expr = match[1];
    const index = match.index ?? -1;
    if (index < 0) {
      continue;
    }

    result += source.slice(lastIndex, index);
    const parsed = parseMacro(expr);
    if (!parsed) {
      result += raw;
    } else {
      const replaced = await handler(parsed, raw);
      result += replaced;
    }
    lastIndex = index + raw.length;
  }

  result += source.slice(lastIndex);
  return result;
}

function maskCodeRegions(source: string): { text: string; restore: (text: string) => string } {
  const ranges = collectCodeRanges(source);
  if (ranges.length === 0) {
    return { text: source, restore: (text) => text };
  }

  const placeholders: string[] = [];
  let result = '';
  let last = 0;
  for (const [start, end] of ranges) {
    result += source.slice(last, start);
    const token = `\u0007CODE_MASK_${placeholders.length}\u0007`;
    placeholders.push(source.slice(start, end));
    result += token;
    last = end;
  }
  result += source.slice(last);

  const restore = (text: string) => {
    let restored = text;
    for (let i = 0; i < placeholders.length; i += 1) {
      const token = `\u0007CODE_MASK_${i}\u0007`;
      restored = restored.split(token).join(placeholders[i]);
    }
    return restored;
  };

  return { text: result, restore };
}

function collectCodeRanges(source: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  const lines = source.split(/\r?\n/);
  let offset = 0;
  let inFence = false;
  let fenceMarker = '';
  let fenceStart = 0;

  for (const line of lines) {
    const lineStart = offset;
    const lineEnd = lineStart + line.length;
    const trimmed = line.trim();

    if (!inFence && FENCE_START_RE.test(trimmed)) {
      const marker = trimmed.startsWith('```') ? '```' : '~~~';
      inFence = true;
      fenceMarker = marker;
      fenceStart = lineStart;
    } else if (inFence && trimmed.startsWith(fenceMarker)) {
      ranges.push([fenceStart, lineEnd]);
      inFence = false;
      fenceMarker = '';
    } else if (!inFence) {
      const inlineRanges = collectInlineCodeRanges(line, lineStart);
      ranges.push(...inlineRanges);
    }

    offset = lineEnd + 1;
  }

  if (inFence) {
    ranges.push([fenceStart, source.length]);
  }

  return mergeRanges(ranges);
}

function collectInlineCodeRanges(
  line: string,
  lineOffset: number,
): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  let i = 0;
  while (i < line.length) {
    if (line[i] !== '`') {
      i += 1;
      continue;
    }
    let tickCount = 1;
    while (i + tickCount < line.length && line[i + tickCount] === '`') {
      tickCount += 1;
    }
    const marker = '`'.repeat(tickCount);
    const start = i;
    const end = line.indexOf(marker, i + tickCount);
    if (end < 0) {
      break;
    }
    ranges.push([lineOffset + start, lineOffset + end + tickCount]);
    i = end + tickCount;
  }
  return ranges;
}

function mergeRanges(ranges: Array<[number, number]>): Array<[number, number]> {
  if (ranges.length === 0) {
    return ranges;
  }
  const sorted = [...ranges].sort((a, b) => a[0] - b[0]);
  const merged: Array<[number, number]> = [];
  let [currentStart, currentEnd] = sorted[0];
  for (let i = 1; i < sorted.length; i += 1) {
    const [start, end] = sorted[i];
    if (start <= currentEnd) {
      currentEnd = Math.max(currentEnd, end);
    } else {
      merged.push([currentStart, currentEnd]);
      currentStart = start;
      currentEnd = end;
    }
  }
  merged.push([currentStart, currentEnd]);
  return merged;
}

function expandSpecialMacros(source: string): string {
  let result = source;

  result = result.replace(
    /!\[\[asset:([^\]\n]+)\]\]/g,
    (_whole, rawAssetPath: string) => {
      const path = rawAssetPath.trim();
      const fileName = extractAssetName(path);
      return `![${escapeMarkdownText(fileName)}](asset:${path})`;
    },
  );

  result = result.replace(
    /\[\[([^\]|#]+(?:#[^\]|]+)?)\|([^\]]+)\]\]/g,
    (_whole, rawPath: string, rawAlias: string) => {
      const path = rawPath.trim();
      const alias = rawAlias.trim();
      if (!path || !alias) {
        return _whole;
      }
      return `[${escapeMarkdownText(alias)}](${path})`;
    },
  );

  result = result.replace(
    /\[\[([^\]|#]+(?:#[^\]|]+)?)\]\]/g,
    (_whole, rawPath: string) => {
      const path = rawPath.trim();
      if (!path) {
        return _whole;
      }
      const pageName = extractPageName(path);
      return `[${escapeMarkdownText(pageName)}](${path})`;
    },
  );

  return result;
}

async function listDescendantPages(prefix: string): Promise<Array<{ path: string }>> {
  const result: Array<{ path: string }> = [];
  let forward: string | undefined = prefix;
  let guard = 0;

  while (guard < 100) {
    guard += 1;
    const response = await fetchPageList({
      prefix,
      forward,
      limit: 100,
      withDeleted: false,
    });
    result.push(...response.items.map((item) => ({ path: item.path })));
    if (!response.has_more || !response.anchor) {
      break;
    }
    forward = response.anchor;
  }

  return result;
}

function countPathSegments(path: string): number {
  if (path === '/') {
    return 0;
  }
  return path.split('/').filter((segment) => segment.length > 0).length;
}

function extractPageName(path: string): string {
  const split = path.split('#')[0];
  if (split === '/') {
    return '/';
  }
  const parts = split.split('/').filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : split;
}

function extractAssetName(path: string): string {
  const slashIndex = path.lastIndexOf('/');
  const colonIndex = path.lastIndexOf(':');
  const splitIndex = Math.max(slashIndex, colonIndex);
  return splitIndex >= 0 ? path.slice(splitIndex + 1) : path;
}

function escapeMarkdownText(text: string): string {
  return text
    .replace(/\\/g, '\\\\')
    .replace(/\[/g, '\\[')
    .replace(/\]/g, '\\]')
    .replace(/\|/g, '\\|');
}

function hasFlag(args: MacroArgs, keys: string[]): boolean {
  return keys.some((key) => args.flags.has(key));
}

function getValue(args: MacroArgs, keys: string[]): string | null {
  for (const key of keys) {
    if (Object.prototype.hasOwnProperty.call(args.values, key)) {
      return args.values[key];
    }
  }
  return null;
}

function pad2(value: number): string {
  return value.toString().padStart(2, '0');
}

function pad4(value: number): string {
  return value.toString().padStart(4, '0');
}

function createUniqueSlug(
  baseSlug: string,
  used: Record<string, true>,
): string {
  let slug = baseSlug;
  let index = 1;
  while (Object.prototype.hasOwnProperty.call(used, slug)) {
    slug = `${baseSlug}-${index}`;
    index += 1;
  }
  used[slug] = true;
  return slug;
}

function normalizeRowWidths(rows: string[][]): string[][] {
  const width = rows.reduce((max, row) => Math.max(max, row.length), 0);
  return rows.map((row) => {
    const padded = [...row];
    while (padded.length < width) {
      padded.push('');
    }
    return padded;
  });
}

function toMarkdownTableRow(cells: string[]): string {
  const escaped = cells.map((cell) => escapeMarkdownText(cell));
  return `| ${escaped.join(' | ')} |`;
}

function parseCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = '';
  let i = 0;
  let inQuote = false;

  while (i < text.length) {
    const ch = text[i];
    if (inQuote) {
      if (ch === '"') {
        if (text[i + 1] === '"') {
          cell += '"';
          i += 2;
          continue;
        }
        inQuote = false;
        i += 1;
        continue;
      }
      cell += ch;
      i += 1;
      continue;
    }

    if (ch === '"') {
      inQuote = true;
      i += 1;
      continue;
    }
    if (ch === ',') {
      row.push(cell);
      cell = '';
      i += 1;
      continue;
    }
    if (ch === '\r') {
      row.push(cell);
      rows.push(row);
      row = [];
      cell = '';
      if (text[i + 1] === '\n') {
        i += 2;
      } else {
        i += 1;
      }
      continue;
    }
    if (ch === '\n') {
      row.push(cell);
      rows.push(row);
      row = [];
      cell = '';
      i += 1;
      continue;
    }
    cell += ch;
    i += 1;
  }

  if (cell.length > 0 || row.length > 0) {
    row.push(cell);
    rows.push(row);
  }

  return rows;
}

function languageFromMime(mimeType: string): string | null {
  const plain = mimeType.split(';')[0].trim().toLowerCase();
  const map: Record<string, string> = {
    'text/plain': 'text',
    'text/markdown': 'markdown',
    'text/x-markdown': 'markdown',
    'application/json': 'json',
    'text/yaml': 'yaml',
    'application/x-yaml': 'yaml',
    'text/x-rust': 'rust',
    'text/rust': 'rust',
    'text/javascript': 'javascript',
    'application/javascript': 'javascript',
    'text/typescript': 'typescript',
    'text/x-shellscript': 'bash',
    'text/x-python': 'python',
    'text/x-go': 'go',
    'text/x-csharp': 'csharp',
    'text/x-sql': 'sql',
  };
  return map[plain] ?? null;
}

async function loadAssetText(
  context: MacroContext,
  src: string,
): Promise<{ text: string; mimeType: string } | null> {
  const direct = await loadAssetTextDirect(context, src);
  if (direct) {
    return direct;
  }

  const pagePath = context.pagePath;
  const rawSpec = src.startsWith('asset:') ? src : `asset:${src}`;
  const url = resolveAssetUrl(pagePath, rawSpec);
  if (!url) {
    return null;
  }

  let response: Response | null = null;
  try {
    const first = await fetch(url, {
      method: 'GET',
      credentials: 'same-origin',
      redirect: 'manual',
    });
    if (first.status >= 300 && first.status < 400) {
      const location = first.headers.get('location');
      if (location) {
        response = await fetch(location, {
          method: 'GET',
          credentials: 'same-origin',
          redirect: 'follow',
        });
      } else {
        const redirected = await fetchAssetByRedirectBody(first);
        if (!redirected) {
          return null;
        }
        response = redirected;
      }
    } else if (first.status === 0) {
      response = await fetch(url, {
        method: 'GET',
        credentials: 'same-origin',
        redirect: 'follow',
      });
    } else {
      response = first;
    }
  } catch {
    return null;
  }

  if (!response || !response.ok) {
    return null;
  }

  const mimeType = (response.headers.get('content-type') ?? '')
    .split(';')[0]
    .trim()
    .toLowerCase();
  const text = await response.text();

  return { text, mimeType };
}

async function loadAssetTextDirect(
  context: MacroContext,
  src: string,
): Promise<{ text: string; mimeType: string } | null> {
  if (!context.pageId) {
    return null;
  }

  const spec = parseAssetSpec(src.startsWith('asset:') ? src : `asset:${src}`);
  if (!spec) {
    return null;
  }
  const resolvedPath = resolvePagePath(context.pagePath, spec.path);
  if (!resolvedPath || resolvedPath !== context.pagePath) {
    return null;
  }

  const fileName = decodeURIComponentSafe(spec.file);
  const assets = await fetchPageAssets(context.pageId, Date.now());
  const target = assets.find((asset) => asset.file_name === fileName)
    ?? assets.find((asset) => asset.file_name.toLowerCase() === fileName.toLowerCase());
  if (!target) {
    return null;
  }

  const response = await fetch(`/api/assets/${encodeURIComponent(target.id)}/data`, {
    method: 'GET',
    credentials: 'same-origin',
    redirect: 'follow',
  });
  if (!response.ok) {
    return null;
  }

  const mimeType = (response.headers.get('content-type') ?? '')
    .split(';')[0]
    .trim()
    .toLowerCase();
  const text = await response.text();
  return { text, mimeType };
}

async function fetchAssetByRedirectBody(
  redirectResponse: Response,
): Promise<Response | null> {
  try {
    const body = await redirectResponse.json() as { id?: string };
    const id = typeof body.id === 'string' ? body.id.trim() : '';
    if (!id) {
      return null;
    }
    return fetch(`/api/assets/${encodeURIComponent(id)}/data`, {
      method: 'GET',
      credentials: 'same-origin',
      redirect: 'follow',
    });
  } catch {
    return null;
  }
}

function decodeURIComponentSafe(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}
