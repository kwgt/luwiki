import type { MarkdownExtension } from '@lezer/markdown';
import { tags as t } from '@lezer/highlight';

export const wikiLinkExtension: MarkdownExtension = {
  defineNodes: [
    { name: 'WikiLink', style: t.link },
  ],
  parseInline: [
    {
      name: 'WikiLink',
      before: 'Link',
      parse(cx, next, pos) {
        if (next !== 91 /* [ */ || cx.char(pos + 1) !== 91 /* [ */) {
          return -1;
        }

        let pipeAt = -1;
        for (let i = pos + 2; i < cx.end - 1; i += 1) {
          const ch = cx.char(i);
          if (ch === 10 /* \n */ || ch === 13 /* \r */) {
            return -1;
          }
          if (ch === 124 /* | */ && pipeAt < 0) {
            pipeAt = i;
          }
          if (ch === 93 /* ] */ && cx.char(i + 1) === 93 /* ] */) {
            const contentStart = pos + 2;
            const contentEnd = i;
            const content = cx.slice(contentStart, contentEnd);
            if (content.trim().length === 0) {
              return -1;
            }

            if (pipeAt >= 0) {
              const left = cx.slice(contentStart, pipeAt).trim();
              const right = cx.slice(pipeAt + 1, contentEnd).trim();
              if (!left || !right) {
                return -1;
              }
            }

            return cx.addElement(cx.elt('WikiLink', pos, i + 2));
          }
        }
        return -1;
      },
    },
  ],
};
