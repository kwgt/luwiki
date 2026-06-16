declare module 'markdown-it-task-lists' {
  import type MarkdownIt = require('markdown-it');

  interface MarkdownItTaskListsOptions {
    enabled?: boolean;
    label?: boolean;
    labelAfter?: boolean;
  }

  const markdownItTaskLists: MarkdownIt.PluginWithOptions<MarkdownItTaskListsOptions>;
  export = markdownItTaskLists;
}
