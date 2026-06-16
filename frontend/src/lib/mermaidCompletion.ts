import {
  completeFromList,
  snippetCompletion,
  type CompletionSource,
} from '@codemirror/autocomplete';
import { type EditorState } from '@codemirror/state';

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

export function isMermaidFenceOpen(state: EditorState, pos: number): boolean {
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

export const mermaidCompletionSource: CompletionSource = (context) => {
  if (!isMermaidFenceOpen(context.state, context.pos)) {
    return null;
  }
  return mermaidCompletionList(context);
};
