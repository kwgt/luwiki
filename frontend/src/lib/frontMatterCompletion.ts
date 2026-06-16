import {
  snippetCompletion,
  type Completion,
  type CompletionResult,
  type CompletionSource,
  type CompletionContext,
} from '@codemirror/autocomplete';
import { syntaxTree } from '@codemirror/language';

interface YamlPathEntry {
  indent: number;
  key: string;
}

interface FrontMatterCompletionTargetKey {
  type: 'key';
  from: number;
  to: number;
  partial: string;
  parentPath: string[];
}

interface FrontMatterCompletionTargetValue {
  type: 'value';
  from: number;
  to: number;
  partial: string;
  path: string[];
}

type FrontMatterCompletionTarget =
  | FrontMatterCompletionTargetKey
  | FrontMatterCompletionTargetValue;

const FRONT_MATTER_KEYWORD_SECTION = { name: 'Front Matter', rank: 1 } as const;
const FRONT_MATTER_VALUE_SECTION = { name: 'Front Matter Values', rank: 2 } as const;
const FRONT_MATTER_SNIPPET_SECTION = { name: 'Front Matter Snippets', rank: 3 } as const;

const topLevelKeyOptions: Completion[] = [
  { label: 'wiki', type: 'namespace', detail: 'LuWiki metadata', section: FRONT_MATTER_KEYWORD_SECTION },
  { label: 'mcp', type: 'namespace', detail: 'MCP metadata', section: FRONT_MATTER_KEYWORD_SECTION },
  { label: 'custom_meta', type: 'namespace', detail: 'User defined metadata', section: FRONT_MATTER_KEYWORD_SECTION },
  snippetCompletion('wiki:\n  template:\n    name: ${name}\n    description: ${description}\n    macro_expand: true\n  tags:\n    - ${tag}', {
    label: 'wiki block',
    type: 'snippet',
    detail: 'wiki template and tags',
    section: FRONT_MATTER_SNIPPET_SECTION,
  }),
  snippetCompletion('mcp:\n  primitive: prompt\n  name: ${name}\n  description: ${description}\n  system: ${system}\n  arguments:\n    - name: ${argument}\n      description: ${argumentDescription}\n      required: true', {
    label: 'mcp prompt block',
    type: 'snippet',
    detail: 'prompt primitive template',
    section: FRONT_MATTER_SNIPPET_SECTION,
  }),
  snippetCompletion('mcp:\n  primitive: resource\n  name: ${name}\n  description: ${description}', {
    label: 'mcp resource block',
    type: 'snippet',
    detail: 'resource primitive template',
    section: FRONT_MATTER_SNIPPET_SECTION,
  }),
];

const nestedKeyOptions = new Map<string, Completion[]>([
  ['wiki', [
    { label: 'template', type: 'property', detail: 'template metadata', section: FRONT_MATTER_KEYWORD_SECTION },
    { label: 'tags', type: 'property', detail: 'tag list', section: FRONT_MATTER_KEYWORD_SECTION },
  ]],
  ['wiki.template', [
    { label: 'name', type: 'property', detail: 'template display name', section: FRONT_MATTER_KEYWORD_SECTION },
    { label: 'description', type: 'property', detail: 'template description', section: FRONT_MATTER_KEYWORD_SECTION },
    { label: 'macro_expand', type: 'property', detail: 'macro expansion flag', section: FRONT_MATTER_KEYWORD_SECTION },
  ]],
  ['mcp', [
    { label: 'primitive', type: 'property', detail: 'prompt or resource', section: FRONT_MATTER_KEYWORD_SECTION },
    { label: 'name', type: 'property', detail: 'display name', section: FRONT_MATTER_KEYWORD_SECTION },
    { label: 'description', type: 'property', detail: 'description', section: FRONT_MATTER_KEYWORD_SECTION },
    { label: 'system', type: 'property', detail: 'prompt-only system text', section: FRONT_MATTER_KEYWORD_SECTION },
    { label: 'arguments', type: 'property', detail: 'prompt argument list', section: FRONT_MATTER_KEYWORD_SECTION },
  ]],
  ['mcp.arguments.[]', [
    { label: 'name', type: 'property', detail: 'argument name', section: FRONT_MATTER_KEYWORD_SECTION },
    { label: 'description', type: 'property', detail: 'argument description', section: FRONT_MATTER_KEYWORD_SECTION },
    { label: 'required', type: 'property', detail: 'required flag', section: FRONT_MATTER_KEYWORD_SECTION },
  ]],
]);

const valueOptions = new Map<string, Completion[]>([
  ['mcp.primitive', [
    { label: 'prompt', type: 'constant', detail: 'MCP prompt primitive', section: FRONT_MATTER_VALUE_SECTION },
    { label: 'resource', type: 'constant', detail: 'MCP resource primitive', section: FRONT_MATTER_VALUE_SECTION },
  ]],
  ['wiki.template.macro_expand', [
    { label: 'true', type: 'constant', detail: 'enable macro expansion', section: FRONT_MATTER_VALUE_SECTION },
    { label: 'false', type: 'constant', detail: 'disable macro expansion', section: FRONT_MATTER_VALUE_SECTION },
  ]],
  ['mcp.arguments.[].required', [
    { label: 'true', type: 'constant', detail: 'required argument', section: FRONT_MATTER_VALUE_SECTION },
    { label: 'false', type: 'constant', detail: 'optional argument', section: FRONT_MATTER_VALUE_SECTION },
  ]],
]);

function countLeadingSpaces(text: string): number {
  let count = 0;
  while (count < text.length && text[count] === ' ') {
    count += 1;
  }
  return count;
}

function prunePathStack(
  stack: YamlPathEntry[],
  indent: number,
): void {
  while (stack.length > 0 && stack[stack.length - 1].indent >= indent) {
    stack.pop();
  }
}

function applyYamlLine(
  stack: YamlPathEntry[],
  rawText: string,
): void {
  const indent = countLeadingSpaces(rawText);
  const trimmed = rawText.trim();
  if (trimmed.length === 0 || trimmed === '---' || trimmed.startsWith('#')) {
    return;
  }

  const content = rawText.slice(indent);
  if (content.startsWith('- ')) {
    prunePathStack(stack, indent);
    stack.push({ indent, key: '[]' });
    const inlineContent = content.slice(2);
    const inlineKey = inlineContent.match(/^([A-Za-z_][\w.-]*)\s*:/);
    if (inlineKey) {
      stack.push({ indent: indent + 2, key: inlineKey[1] });
    }
    return;
  }

  const keyMatch = content.match(/^([A-Za-z_][\w.-]*)\s*:/);
  if (!keyMatch) {
    return;
  }

  prunePathStack(stack, indent);
  stack.push({ indent, key: keyMatch[1] });
}

function buildPathStackBeforeLine(
  docText: string,
  currentLineNumber: number,
): YamlPathEntry[] {
  const lines = docText.split('\n');
  const stack: YamlPathEntry[] = [];

  for (let index = 1; index < currentLineNumber - 1 && index < lines.length; index += 1) {
    applyYamlLine(stack, lines[index]);
  }

  return stack;
}

function isInFrontMatter(state: CompletionContext['state'], pos: number): boolean {
  const node = syntaxTree(state).resolveInner(pos, 1);
  for (let current: typeof node | null = node; current; current = current.parent) {
    if (current.name === 'Frontmatter') {
      return true;
    }
  }
  return false;
}

function buildParentPath(
  stack: YamlPathEntry[],
  indent: number,
  isListItem: boolean,
): string[] {
  const base = stack
    .filter((entry) => entry.indent < indent)
    .map((entry) => entry.key);

  if (isListItem) {
    const last = stack[stack.length - 1];
    if (last && last.indent < indent && last.key !== '[]') {
      return [...base, '[]'];
    }
    if (last && last.indent === indent && last.key === '[]') {
      return stack
        .filter((entry) => entry.indent < indent)
        .map((entry) => entry.key)
        .concat('[]');
    }
    return [...base, '[]'];
  }

  return base;
}

function detectCompletionTarget(
  context: CompletionContext,
): FrontMatterCompletionTarget | null {
  if (!isInFrontMatter(context.state, context.pos)) {
    return null;
  }

  const line = context.state.doc.lineAt(context.pos);
  const beforeCursor = line.text.slice(0, context.pos - line.from);
  const trimmed = beforeCursor.trim();
  if (trimmed === '---') {
    return null;
  }

  const stack = buildPathStackBeforeLine(
    context.state.doc.toString(),
    line.number,
  );

  const keyMatch = beforeCursor.match(/^(\s*)(-\s*)?([A-Za-z_][\w.-]*)?$/);
  if (keyMatch) {
    const indent = keyMatch[1].length;
    const isListItem = typeof keyMatch[2] === 'string';
    const partial = keyMatch[3] ?? '';
    const prefixLength = indent + (keyMatch[2]?.length ?? 0);
    return {
      type: 'key',
      from: line.from + prefixLength,
      to: context.pos,
      partial,
      parentPath: buildParentPath(stack, indent, isListItem),
    };
  }

  const valueMatch = beforeCursor.match(/^(\s*)(-\s*)?([A-Za-z_][\w.-]*)\s*:\s*([A-Za-z_-]*)$/);
  if (valueMatch) {
    const indent = valueMatch[1].length;
    const isListItem = typeof valueMatch[2] === 'string';
    const key = valueMatch[3];
    const partial = valueMatch[4] ?? '';
    return {
      type: 'value',
      from: context.pos - partial.length,
      to: context.pos,
      partial,
      path: [...buildParentPath(stack, indent, isListItem), key],
    };
  }

  return null;
}

function resolveKeyOptions(parentPath: string[]): Completion[] {
  if (parentPath.length === 0) {
    return topLevelKeyOptions;
  }
  return nestedKeyOptions.get(parentPath.join('.')) ?? [];
}

function resolveValueOptions(path: string[]): Completion[] {
  return valueOptions.get(path.join('.')) ?? [];
}

function buildCompletionResult(
  target: FrontMatterCompletionTarget,
  options: Completion[],
): CompletionResult | null {
  if (options.length === 0) {
    return null;
  }

  if (target.partial.length === 0) {
    return {
      from: target.from,
      to: target.to,
      options,
      validFor: /^[A-Za-z_][\w.-]*$/,
    };
  }

  return {
    from: target.from,
    to: target.to,
    options,
    validFor: /^[A-Za-z_][\w.-]*$/,
  };
}

export const frontMatterCompletionSource: CompletionSource = (context) => {
  const target = detectCompletionTarget(context);
  if (!target) {
    return null;
  }

  if (!context.explicit && target.partial.length === 0) {
    return null;
  }

  const options = target.type === 'key'
    ? resolveKeyOptions(target.parentPath)
    : resolveValueOptions(target.path);

  return buildCompletionResult(target, options);
};

export function debugFrontMatterCompletionTarget(
  state: CompletionContext['state'],
  pos: number,
): FrontMatterCompletionTarget | null {
  return detectCompletionTarget({
    state,
    pos,
    explicit: true,
  } as CompletionContext);
}
