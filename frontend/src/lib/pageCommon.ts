import axios from 'axios';
import MarkdownIt from 'markdown-it';
import taskLists from 'markdown-it-task-lists';
import Prism from 'prismjs';
import 'prismjs/components/prism-markup';
import 'prismjs/components/prism-markdown';
import 'prismjs/components/prism-json';
import 'prismjs/components/prism-yaml';
import 'prismjs/components/prism-bash';
import 'prismjs/components/prism-javascript';
import 'prismjs/components/prism-typescript';
import 'prismjs/components/prism-jsx';
import 'prismjs/components/prism-tsx';
import 'prismjs/components/prism-python';
import 'prismjs/components/prism-rust';
import 'prismjs/components/prism-go';
import 'prismjs/components/prism-csharp';
import 'prismjs/components/prism-toml';
import 'prismjs/components/prism-diff';
import 'prismjs/components/prism-css';
import 'prismjs/components/prism-sql';

export interface TocEntry {
  level: number;
  text: string;
  anchor: string;
}

export interface MarkdownRendererOptions {
  plugins?: {
    taskList?: boolean;
  };
}

const ASSET_PREFIX = 'asset:';

export function getMetaContent(name: string): string | null {
  const tag = document.querySelector(`meta[name="${name}"]`);
  return tag?.getAttribute('content') ?? null;
}

export function normalizeWikiPath(rawPath: string): string {
  const trimmed = rawPath.replace(/^\/+/, '');
  if (trimmed.length === 0) {
    return '/';
  }
  return `/${trimmed}`;
}

export function resolvePagePath(basePath: string, targetPath: string): string | null {
  if (!targetPath || targetPath.trim().length === 0) {
    return null;
  }

  if (targetPath.startsWith('/')) {
    return cleanPath(targetPath);
  }

  if (targetPath.startsWith('#')) {
    return null;
  }

  const normalizedBase = basePath.trim().length > 0 ? basePath : '/';
  if (targetPath === '.') {
    return cleanPath(normalizedBase);
  }
  const base = normalizedBase.endsWith('/')
    ? normalizedBase
    : `${normalizedBase}/`;
  return cleanPath(`${base}${targetPath}`);
}

export function cleanPath(pathValue: string): string {
  const parts = pathValue.split('/');
  const stack: string[] = [];
  for (const part of parts) {
    if (!part || part === '.') {
      continue;
    }
    if (part === '..') {
      stack.pop();
      continue;
    }
    stack.push(part);
  }
  return `/${stack.join('/')}`;
}

export function parseAssetSpec(rawSpec: string): { path: string; file: string } | null {
  if (!rawSpec.startsWith(ASSET_PREFIX)) {
    return null;
  }
  const rest = rawSpec.slice(ASSET_PREFIX.length);
  if (!rest) {
    return null;
  }

  const splitIndex = rest.lastIndexOf(':');
  if (splitIndex < 0) {
    const slashIndex = rest.lastIndexOf('/');
    if (slashIndex < 0) {
      return { path: '.', file: rest };
    }
    const path = rest.slice(0, slashIndex) || '.';
    const file = rest.slice(slashIndex + 1);
    if (!file) {
      return null;
    }
    return { path, file };
  }

  const path = rest.slice(0, splitIndex);
  const file = rest.slice(splitIndex + 1);
  if (!file) {
    return null;
  }

  return { path, file };
}

export function resolveAssetUrl(pagePath: string, rawSpec: string): string | null {
  const parsed = parseAssetSpec(rawSpec);
  if (!parsed) {
    return null;
  }

  const resolvedPath = resolvePagePath(pagePath, parsed.path);
  if (!resolvedPath) {
    return null;
  }

  const params = new URLSearchParams({
    path: resolvedPath,
    file: parsed.file,
  });
  return `/api/assets?${params.toString()}`;
}

export function slugifyHeading(text: string): string {
  const trimmed = text.trim();
  if (!trimmed) {
    return 'section';
  }
  return trimmed
    .toLowerCase()
    .replace(/\s+/g, '-')
    .replace(/[^\w\-\u3040-\u30ff\u3400-\u9fff]/g, '');
}

export function extractTitle(markdown: string, pagePath: string): string {
  const lines = markdown.split(/\r?\n/);
  for (const line of lines) {
    const match = line.match(/^#\s+(.+)$/);
    if (match) {
      return match[1].trim();
    }
  }

  if (pagePath === '/') {
    return '/';
  }

  const parts = pagePath.split('/').filter(Boolean);
  return parts[parts.length - 1] ?? pagePath;
}

export function extractToc(markdown: string): TocEntry[] {
  const entries: TocEntry[] = [];
  const lines = markdown.split(/\r?\n/);
  for (const line of lines) {
    const match = line.match(/^(#{2,4})\s+(.+)$/);
    if (!match) {
      continue;
    }
    const level = match[1].length;
    const text = match[2].trim();
    entries.push({
      level,
      text,
      anchor: slugifyHeading(text),
    });
  }
  return entries;
}

function buildWikiLink(base: string, resolvedPath: string, suffix: string): string {
  if (resolvedPath === '/') {
    return `${base}/` + suffix;
  }
  return `${base}${resolvedPath}${suffix}`;
}

function splitHref(href: string): { path: string; suffix: string } {
  const hashIndex = href.indexOf('#');
  const queryIndex = href.indexOf('?');
  let splitIndex = -1;
  if (hashIndex >= 0 && queryIndex >= 0) {
    splitIndex = Math.min(hashIndex, queryIndex);
  } else if (hashIndex >= 0) {
    splitIndex = hashIndex;
  } else if (queryIndex >= 0) {
    splitIndex = queryIndex;
  }
  if (splitIndex < 0) {
    return { path: href, suffix: '' };
  }
  return { path: href.slice(0, splitIndex), suffix: href.slice(splitIndex) };
}

function isExternalLink(href: string): boolean {
  return /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(href);
}

export function createMarkdownRenderer(
  pagePath: string,
  linkBase = '/wiki',
  options?: MarkdownRendererOptions,
): MarkdownIt {
  const md = new MarkdownIt({
    html: false,
    linkify: true,
    highlight(code, lang) {
      const normalized = normalizeLanguage(lang);
      if (!normalized) {
        const escaped = md.utils.escapeHtml(code);
        return `<pre class="language-text"><code class="language-text">${escaped}</code></pre>`;
      }

      const grammar = Prism.languages[normalized];
      if (!grammar) {
        const escaped = md.utils.escapeHtml(code);
        return `<pre class="language-text"><code class="language-text">${escaped}</code></pre>`;
      }

      const highlighted = Prism.highlight(code, grammar, normalized);
      return `<pre class="language-${normalized}"><code class="language-${normalized}">${highlighted}</code></pre>`;
    },
  });

  const taskListEnabled = options?.plugins?.taskList ?? true;
  if (taskListEnabled) {
    md.use(taskLists, { enabled: false });
  }

  const defaultHeadingOpen = md.renderer.rules.heading_open;
  md.renderer.rules.heading_open = (tokens, idx, options, env, self) => {
    const inline = tokens[idx + 1];
    const title = inline && inline.type === 'inline' ? inline.content : '';
    const anchor = slugifyHeading(title);
    tokens[idx].attrSet('id', anchor);
    if (defaultHeadingOpen) {
      return defaultHeadingOpen(tokens, idx, options, env, self);
    }
    return self.renderToken(tokens, idx, options);
  };

  const defaultLinkOpen = md.renderer.rules.link_open;
  md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
    const href = tokens[idx].attrGet('href');
    if (href && href.startsWith(ASSET_PREFIX)) {
      const resolved = resolveAssetUrl(pagePath, href);
      if (resolved) {
        tokens[idx].attrSet('href', resolved);
      }
    } else if (href && !isExternalLink(href)) {
      const { path, suffix } = splitHref(href);
      if (path && !path.startsWith('#')) {
        const resolved = resolvePagePath(pagePath, path);
        if (resolved) {
          tokens[idx].attrSet('href', buildWikiLink(linkBase, resolved, suffix));
        }
      }
    }
    if (defaultLinkOpen) {
      return defaultLinkOpen(tokens, idx, options, env, self);
    }
    return self.renderToken(tokens, idx, options);
  };

  const defaultImage = md.renderer.rules.image;
  md.renderer.rules.image = (tokens, idx, options, env, self) => {
    const src = tokens[idx].attrGet('src');
    if (src && src.startsWith(ASSET_PREFIX)) {
      const resolved = resolveAssetUrl(pagePath, src);
      if (resolved) {
        tokens[idx].attrSet('src', resolved);
      }
    }
    if (defaultImage) {
      return defaultImage(tokens, idx, options, env, self);
    }
    return self.renderToken(tokens, idx, options);
  };

  return md;
}

export function normalizeLanguage(lang?: string): string | null {
  if (!lang) {
    return null;
  }
  const normalized = lang.toLowerCase();
  const aliasMap: Record<string, string> = {
    sh: 'bash',
    shell: 'bash',
    yml: 'yaml',
    js: 'javascript',
    ts: 'typescript',
    md: 'markdown',
    rs: 'rust',
  };
  const resolved = aliasMap[normalized] ?? normalized;
  return Prism.languages[resolved] ? resolved : null;
}

export function toErrorMessage(err: unknown): string {
  if (err && typeof err === 'object' && 'status' in err) {
    const status = (err as { status?: unknown }).status;
    const reason = (err as { reason?: unknown }).reason;
    const statusMessageMap: Record<number, string> = {
      400: 'リクエストが不正です',
      401: '認証に失敗しました',
      403: '権限がありません',
      404: '対象が見つかりません',
      409: '同名のアセットが既に存在します',
      410: '対象は削除済みです',
      411: 'コンテンツ長が必要です',
      413: 'ファイルサイズが大きすぎます',
      423: 'ページがロックされています',
      500: 'サーバ内でエラーが発生しました',
    };
    if (typeof reason === 'string' && reason.trim().length > 0) {
      return reason;
    }
    if (typeof status === 'number' && statusMessageMap[status]) {
      return statusMessageMap[status];
    }
    if (typeof status === 'number') {
      return `通信エラー (HTTP ${status})`;
    }
  }
  if (axios.isAxiosError(err)) {
    if (err.code === 'ECONNABORTED') {
      return 'リクエストがタイムアウトしました';
    }
    const requestStatus = (() => {
      const req = err.request as { status?: number } | undefined;
      if (req && typeof req.status === 'number' && req.status > 0) {
        return req.status;
      }
      return undefined;
    })();
    const status = err.response?.status ?? requestStatus;
    const data = err.response?.data;
    if (data && typeof data === 'object' && 'reason' in data) {
      const reason = (data as { reason?: unknown }).reason;
      if (typeof reason === 'string' && reason.trim().length > 0) {
        return reason;
      }
    }
    const statusMessageMap: Record<number, string> = {
      400: 'リクエストが不正です',
      401: '認証に失敗しました',
      403: '権限がありません',
      404: '対象が見つかりません',
      409: '同名のアセットが既に存在します',
      410: '対象は削除済みです',
      411: 'コンテンツ長が必要です',
      413: 'ファイルサイズが大きすぎます',
      423: 'ページがロックされています',
      500: 'サーバ内でエラーが発生しました',
    };
    if (status && statusMessageMap[status]) {
      return statusMessageMap[status];
    }
    if (status) {
      return `通信エラー (HTTP ${status})`;
    }
    return 'ネットワークエラーが発生しました';
  }

  if (err instanceof Error) {
    return err.message;
  }

  return 'unknown error';
}

export function formatBytes(size: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB'];
  let value = size;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  const formatted = value.toLocaleString('en-US', {
    maximumFractionDigits: 1,
  });
  return `${formatted}${units[index]}`;
}

export function parseAssetMaxBytes(value: string | null): number | null {
  if (!value) {
    return null;
  }
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return null;
  }
  return Math.floor(parsed);
}
