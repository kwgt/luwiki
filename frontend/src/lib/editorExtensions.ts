import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { HighlightStyle, LanguageDescription, syntaxHighlighting } from '@codemirror/language';
import { markdown } from '@codemirror/lang-markdown';
import { python } from '@codemirror/lang-python';
import { rust } from '@codemirror/lang-rust';
import { search, searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { type Extension } from '@codemirror/state';
import { tags as t } from '@lezer/highlight';
import { EditorView, keymap, lineNumbers } from '@codemirror/view';
import { oneDark, oneDarkHighlightStyle } from '@codemirror/theme-one-dark';
import { emacs } from '@replit/codemirror-emacs';
import { csharp } from '@replit/codemirror-lang-csharp';
import { vim } from '@replit/codemirror-vim';
import { vscodeKeymap } from '@replit/codemirror-vscode-keymap';

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

export function buildBaseExtensions(): Extension[] {
  return [
    EditorView.lineWrapping,
    history(),
    search(),
    highlightSelectionMatches(),
    markdown({ codeLanguages }),
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
