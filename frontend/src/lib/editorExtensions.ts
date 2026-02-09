import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import {
  autocompletion,
  completeFromList,
  type CompletionSource,
  snippetCompletion,
} from '@codemirror/autocomplete';
import { HighlightStyle, LanguageDescription, syntaxHighlighting } from '@codemirror/language';
import { markdown } from '@codemirror/lang-markdown';
import { python } from '@codemirror/lang-python';
import { rust } from '@codemirror/lang-rust';
import { search, searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { type EditorState, type Extension, RangeSetBuilder } from '@codemirror/state';
import { tags as t } from '@lezer/highlight';
import {
  Decoration,
  EditorView,
  type DecorationSet,
  keymap,
  lineNumbers,
  ViewPlugin,
  type ViewUpdate,
} from '@codemirror/view';
import { oneDark, oneDarkHighlightStyle } from '@codemirror/theme-one-dark';
import { emacs } from '@replit/codemirror-emacs';
import { csharp } from '@replit/codemirror-lang-csharp';
import { vim } from '@replit/codemirror-vim';
import { vscodeKeymap } from '@replit/codemirror-vscode-keymap';
import { wikiLinkExtension } from './markdown/wikiLinkExtension';

export type EditorKeymap = 'default' | 'vim' | 'emacs' | 'vscode';
export type EditorTheme = 'light' | 'dark';

const baseTheme = EditorView.theme(
  {
    '&': {
      height: '100%',
    },
    '.cm-scroller': {
      fontFamily: 'var(--cm-font-family)',
      fontSize: 'var(--cm-font-size)',
    },
    '.cm-content': {
      fontFamily: 'var(--cm-font-family)',
      padding: '0.75rem',
    },
    '.cm-mermaid-keyword': {
      color: '#005cc5',
      fontWeight: '600',
    },
    '.cm-mermaid-arrow': {
      color: '#6f42c1',
      fontWeight: '600',
    },
    '.cm-mermaid-comment': {
      color: '#6a737d',
      fontStyle: 'italic',
    },
  },
  { dark: false },
);

const lightHeadingTheme = EditorView.theme(
  {
    '.cm-heading': {
      borderBottom: '1px solid #d0d7de',
      paddingBottom: '0.2rem',
    },
  },
  { dark: false },
);

const oneLightHighlightStyle = HighlightStyle.define([
  { tag: t.comment, color: '#6a737d' },
  { tag: t.keyword, color: '#a626a4' },
  { tag: [t.name, t.deleted, t.character, t.propertyName, t.macroName], color: '#e45649' },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: '#4078f2' },
  { tag: [t.definition(t.variableName), t.definition(t.propertyName)], color: '#4078f2' },
  { tag: [t.labelName, t.definition(t.labelName)], color: '#4078f2' },
  { tag: [t.color, t.constant(t.name), t.standard(t.name)], color: '#986801' },
  { tag: [t.number, t.changed, t.annotation, t.modifier, t.self, t.namespace], color: '#986801' },
  { tag: [t.typeName, t.className], color: '#c18401' },
  { tag: [t.operator, t.operatorKeyword], color: '#0184bc' },
  { tag: [t.string, t.special(t.string)], color: '#50a14f' },
  { tag: [t.meta, t.comment], color: '#6a737d' },
  { tag: t.strong, fontWeight: 'bold' },
  { tag: t.emphasis, fontStyle: 'italic' },
  { tag: t.link, color: '#0184bc', textDecoration: 'underline' },
  { tag: t.heading, fontWeight: 'bold', color: '#4078f2', textDecoration: 'underline' },
  { tag: t.heading1, fontWeight: 'bold', color: '#4078f2', textDecoration: 'underline' },
  { tag: t.heading2, fontWeight: 'bold', color: '#4078f2', textDecoration: 'underline' },
  { tag: t.heading3, fontWeight: 'bold', color: '#4078f2', textDecoration: 'underline' },
  { tag: t.heading4, fontWeight: 'bold', color: '#4078f2', textDecoration: 'underline' },
  { tag: t.heading5, fontWeight: 'bold', color: '#4078f2', textDecoration: 'underline' },
  { tag: t.heading6, fontWeight: 'bold', color: '#4078f2', textDecoration: 'underline' },
  { tag: t.atom, color: '#d19a66' },
  { tag: t.invalid, color: '#ffffff', backgroundColor: '#e45649' },
]);

const codeLanguages = [
  LanguageDescription.of({
    name: 'python',
    alias: ['py'],
    support: python(),
  }),
  LanguageDescription.of({
    name: 'rust',
    alias: ['rs'],
    support: rust(),
  }),
  LanguageDescription.of({
    name: 'csharp',
    alias: ['cs', 'c#'],
    support: csharp(),
  }),
];

const mermaidKeywordDecoration = Decoration.mark({ class: 'cm-mermaid-keyword' });
const mermaidArrowDecoration = Decoration.mark({ class: 'cm-mermaid-arrow' });
const mermaidCommentDecoration = Decoration.mark({ class: 'cm-mermaid-comment' });

function parseFenceInfo(text: string): { marker: string; lang: string } | null {
  const matched = text.match(/^(\s*)(`{3,}|~{3,})([^\n]*)$/);
  if (!matched) {
    return null;
  }

  const marker = matched[2];
  const rest = matched[3].trim();
  const lang = rest.split(/\s+/, 1)[0]?.toLowerCase() ?? '';
  return { marker, lang };
}

function isMermaidFenceOpen(state: EditorState, pos: number): boolean {
  let inMermaid = false;
  let fenceMarker = '';
  const targetLine = state.doc.lineAt(pos).number;
  for (let lineNo = 1; lineNo <= targetLine; lineNo += 1) {
    const line = state.doc.line(lineNo);
    const info = parseFenceInfo(line.text);
    if (!inMermaid) {
      if (info?.lang === 'mermaid') {
        inMermaid = true;
        fenceMarker = info.marker;
      }
      continue;
    }
    if (info && info.marker[0] === fenceMarker[0] && info.marker.length >= fenceMarker.length) {
      inMermaid = false;
      fenceMarker = '';
    }
  }
  return inMermaid;
}

function buildMermaidDecorations(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  let inMermaid = false;
  let fenceMarker = '';

  for (let lineNo = 1; lineNo <= view.state.doc.lines; lineNo += 1) {
    const line = view.state.doc.line(lineNo);
    const info = parseFenceInfo(line.text);

    if (!inMermaid) {
      if (info?.lang === 'mermaid') {
        inMermaid = true;
        fenceMarker = info.marker;
      }
      continue;
    }

    if (info && info.marker[0] === fenceMarker[0] && info.marker.length >= fenceMarker.length) {
      inMermaid = false;
      fenceMarker = '';
      continue;
    }

    const lineStart = line.from;
    const lineText = line.text;

    const commentMatch = lineText.match(/%%.*/);
    if (commentMatch && commentMatch.index !== undefined) {
      builder.add(
        lineStart + commentMatch.index,
        lineStart + commentMatch.index + commentMatch[0].length,
        mermaidCommentDecoration,
      );
    }

    const keywordRegex = /\b(flowchart|graph|sequenceDiagram|classDiagram|stateDiagram|stateDiagram-v2|erDiagram|journey|gantt|pie|mindmap|timeline|quadrantChart|gitGraph|subgraph|end|participant|actor|class|linkStyle|style)\b/g;
    for (;;) {
      const matched = keywordRegex.exec(lineText);
      if (!matched || matched.index === undefined) {
        break;
      }
      builder.add(
        lineStart + matched.index,
        lineStart + matched.index + matched[0].length,
        mermaidKeywordDecoration,
      );
    }

    const arrowRegex = /(<-->|-->|==>|-.->|---|--x|x--|o--|--o|<--|<->)/g;
    for (;;) {
      const matched = arrowRegex.exec(lineText);
      if (!matched || matched.index === undefined) {
        break;
      }
      builder.add(
        lineStart + matched.index,
        lineStart + matched.index + matched[0].length,
        mermaidArrowDecoration,
      );
    }
  }

  return builder.finish();
}

const mermaidHighlightPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildMermaidDecorations(view);
    }

    update(update: ViewUpdate): void {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildMermaidDecorations(update.view);
      }
    }
  },
  {
    decorations: (value) => value.decorations,
  },
);

const mermaidCompletionList = completeFromList([
  snippetCompletion('flowchart TD\n  A[Start] --> B[End]', {
    label: 'flowchart template',
    type: 'snippet',
  }),
  snippetCompletion('sequenceDiagram\n  participant A\n  participant B\n  A->>B: message', {
    label: 'sequence template',
    type: 'snippet',
  }),
  { label: 'flowchart', type: 'keyword' },
  { label: 'graph', type: 'keyword' },
  { label: 'sequenceDiagram', type: 'keyword' },
  { label: 'classDiagram', type: 'keyword' },
  { label: 'stateDiagram-v2', type: 'keyword' },
  { label: 'erDiagram', type: 'keyword' },
  { label: 'gantt', type: 'keyword' },
  { label: 'subgraph', type: 'keyword' },
  { label: 'participant', type: 'keyword' },
  { label: 'linkStyle', type: 'keyword' },
  { label: '-->', type: 'operator' },
  { label: '==>', type: 'operator' },
  { label: '-.->', type: 'operator' },
]);

const mermaidCompletionSource: CompletionSource = (context) => {
  if (!isMermaidFenceOpen(context.state, context.pos)) {
    return null;
  }
  return mermaidCompletionList(context);
};

export function buildBaseExtensions(): Extension[] {
  return [
    EditorView.lineWrapping,
    history(),
    search(),
    highlightSelectionMatches(),
    markdown({
      codeLanguages,
      extensions: [wikiLinkExtension],
    }),
    autocompletion({
      override: [mermaidCompletionSource],
    }),
    mermaidHighlightPlugin,
    baseTheme,
  ];
}

export function buildLineNumberExtension(enabled: boolean): Extension {
  return enabled ? lineNumbers() : [];
}

export function buildThemeExtension(theme: EditorTheme): Extension {
  if (theme === 'dark') {
    return [
      oneDark,
      syntaxHighlighting(oneDarkHighlightStyle),
    ];
  }
  return [
    lightHeadingTheme,
    syntaxHighlighting(oneLightHighlightStyle),
  ];
}

export function buildKeymapExtension(keymapName: EditorKeymap): Extension {
  const commonKeymaps = [indentWithTab, ...searchKeymap, ...historyKeymap];
  if (keymapName === 'vim') {
    return [vim(), keymap.of(commonKeymaps)];
  }
  if (keymapName === 'emacs') {
    return [emacs(), keymap.of(commonKeymaps)];
  }
  if (keymapName === 'vscode') {
    return keymap.of([...vscodeKeymap, ...commonKeymaps]);
  }
  return keymap.of([...commonKeymaps, ...defaultKeymap]);
}
