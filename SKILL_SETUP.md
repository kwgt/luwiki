# スキルの設定

LuwikiをAIエージェントに効率よく使わせるためのスキルを以下のエージェントに対し設定する方法を記述します。

- Codex CLI
- Claude Code

なお、事前にエージェントへのMCPサービス登録を行っておく必要があります。

## Codex CLIの場合
以下の手順で登録を行います。

1. スキル設定格納用のディレクトリの作成
2. SKILL.mdの格納
3. openai.yamlの格納

### スキル設定格納用のディレクトリの作成
以下のディレクトリを作成してください。

- `~/.codex/skills/luwiki` : `SKILL.md` を格納するディレクトリ
- `~/.codex/skills//luwiki/agents` : `openai.yaml` を格納するディレクトリ。

### SKILL.mdの格納

本書の末尾に記載のSKILL.mdの先頭に以下のfront matterを追加し `~/.codex/skills/luwiki/SKILL.md` を作成します。

```yaml
---
name: luwiki
description: Use an MCP-enabled Wiki to find, read, create, edit, append, or rename Markdown pages through get_page, get_page_toc, get_page_section, list_pages, search_pages, create_page, update_page, edit_page, append_page, and rename_page. Use whenever a request involves this Wiki's pages, Wiki paths or links, page sections, revisions, or Wiki content maintenance. Apply the Wiki-specific relative-path rule in which the current page itself—not its parent—is the resolution base.
---
```

### openai.yamlの格納

以下の内容で `~/.codex/skills/luwiki/agents/openai.yaml` を作成します。

```yaml
interface:
  display_name: "Luwiki MCP"
  short_description: "Luwikiをエージェントの作業メモとして使用します"
  default_prompt: "Use $luwiki to read and maintain the project scratchpad."

dependencies:
  tools:
    - type: "mcp"
      value: "local_wiki"
      description: "Local Wiki and agent scratchpad"
      transport: "streamable_http"
      url: "https://localhost:8080/mcp"

policy:
  allow_implicit_invocation: true
```

`dependencies.tools[].value`には登録したMCPサービス名を設定してください。また、Luwikiを使用したWikiサービスをMCPサーバとして追加した場合は同じフォーマットで `dependencies.tools` にエントリを追加することにより対応できます。

## Claude Codeの場合

以下の手順で登録を行います。

1. スキル設定格納用のディレクトリの作成
2. SKILL.mdの格納

### スキル設定格納用のディレクトリの作成
以下のディレクトリを作成してください。

- `~/.claude/skills/luwiki` : `SKILL.md` を格納するディレクトリ

### SKILL.mdの格納

本書の末尾に記載のSKILL.mdの先頭に以下のfront matterを追加し `~/.claude/skills/luwiki/SKILL.md` を作成します。


```yaml
---
name: luwiki
description: Use Luwiki to find, read, create, edit, append, or rename Wiki pages, and to maintain project or agent scratchpad content.
allowed-tools:
  - mcp__local_wiki__get_page
  - mcp__local_wiki__get_page_toc
  - mcp__local_wiki__get_page_section
  - mcp__local_wiki__list_pages
  - mcp__local_wiki__search_pages
  - mcp__local_wiki__create_page
  - mcp__local_wiki__update_page
  - mcp__local_wiki__edit_page
  - mcp__local_wiki__append_page
  - mcp__local_wiki__rename_page
---
```

なお、`allowed-tools` に記述するツール名の `local_wiki` となっている箇所を登録したMCPサービス名に置き換えてください。

## SKILL.md本文

````markdown

# Wiki MCP

Use the Wiki's MCP tools as the source of truth. Pass absolute Wiki paths to tool arguments. Do not expose or invent internal page IDs.

## Resolve Wiki paths

Treat every Wiki page as both content and a container. Resolve a relative path from the current page path itself, not from the page's parent.

- Current page `/a/b` + relative path `c/d` => `/a/b/c/d`
- Do **not** resolve that example to `/a/c/d`; that is filesystem file semantics.
- Keep an absolute path unchanged.
- Establish the current page before resolving a relative reference. If the request does not identify it and context does not make it unambiguous, ask for the current page path.
- Resolve relative references to absolute Wiki paths before calling a page tool.
- Apply the server's documented path normalization and validation after selecting the current page as the base. Do not guess behavior for ambiguous dot segments or other syntax not defined by the Wiki contract.

Use the same rule for links found in page content, user-supplied relative page references, and destination paths derived from a current page. A page can therefore have child pages even though it also contains Markdown content.

## Choose the narrowest tool

| Need | Tool | Key guidance |
| --- | --- | --- |
| Read a whole page | `get_page` | Use when the complete raw Markdown is needed. |
| Inspect page structure | `get_page_toc` | Get headings, dynamic section IDs, and approximate section sizes without loading the full page. |
| Read one section | `get_page_section` | Prefer after TOC inspection for focused retrieval. |
| Browse a subtree | `list_pages` | Page through path-sorted results with `next_cursor` while `has_more` is true. |
| Search by content | `search_pages` | Explicitly select one or more targets: `headings`, `body`, `code`, `front_matter`. This is top-N search, not cursor pagination. |
| Create a page | `create_page` | Supply the final absolute destination path and complete initial raw Markdown. |
| Replace a whole page | `update_page` | Use only when full-source replacement is intended. |
| Make a focused edit | `edit_page` | Prefer for section or text changes; first obtain current concurrency tokens. |
| Add content at the end | `append_page` | Supply only the exact suffix, including any needed newlines. |
| Change a page path | `rename_page` | Supply the final absolute destination in `rename_to`, not merely a parent path. |

## Read efficiently

1. Resolve the requested Wiki path.
2. For a small or necessarily complete page, call `get_page`.
3. For a large page or a request about one topic, call `get_page_toc`, choose the section, then call `get_page_section` using `{ "by": "id", "value": "..." }` when possible.
4. Treat section IDs as dynamic and valid only within the corresponding revision. Do not reuse one across revisions.
5. If a title is duplicated, do not use a title selector; obtain the TOC and use the section ID.

`get_page_section.content` excludes the selected heading line and includes its descendants until the next heading at the same or higher level.

## Discover pages

For hierarchical discovery, call `list_pages` with an absolute `prefix`. Use a limit from 1 to 100. When `has_more` is true, repeat with the same prefix and `cursor=next_cursor`; the cursor item is excluded from the next page.

For full-text discovery, call `search_pages` with a nonempty `query`, at least one explicit `target`, and an optional absolute `prefix`. Use a limit from 1 to 100. Results are score-ranked snapshots; there is no cursor or stable continuation.

An empty result is a valid result, not an error.

## Write safely

Before changing content, read enough of the current page to preserve unrelated Markdown and front matter.

- Prefer `edit_page` for localized changes.
- Use `update_page` only when replacing the complete raw source is deliberate.
- Use `append_page` only for literal end-of-page additions. Include `\n` in `content` when line separation is required; the server does not add it.
- Preserve existing front matter unless the request explicitly changes it. For `create_page` and `update_page`, `content` may be the complete raw source including front matter.
- Do not use `append_page` to edit front matter.
- Do not attempt restore, deleted-page access, asset operations, locks, templates, outgoing-link lists, or backlink lists; the initial tool set does not support them.

### Focused edits

Immediately before `edit_page`, obtain the latest `revision` and `instance_id` from `get_page` or `get_page_toc`. Pass both values back unchanged with exactly one operation:

- `replace_section`: replace a section body. Do not include the target heading line in `content`. Use `title` to rename the heading. Existing subsections remain by default; set `include_subsection: true` only when replacing them too.
- `insert_section`: include a complete section, including its heading, and place it `before` or `after` an anchor.
- `delete_section`: delete the selected section.
- `replace_text`: make `old_text` specific enough to identify the intended text. Use `occurrence: all` only when every match should change.

For section selectors, prefer `{ "by": "id", "value": "..." }` after a fresh TOC read. A string selector is interpreted as a heading title.

If `edit_page` returns `not_latest_revision` or `instance_id_not_match`, fetch the current page again, reassess the requested change against the new content, and retry only if the intent remains safe. Never reuse stale concurrency tokens.

## Rename safely

Resolve both source and destination as absolute Wiki paths. Verify that `rename_to` is the final target. Do not rename the root page, and do not move a page beneath its own current path. Both paths must be permitted by the active path-prefix authorization.

## Handle errors

- `not_found`: verify the resolved absolute path or revision; do not assume a deleted page can be read.
- `forbidden`: stop and report that the required scope or path-prefix permission is missing. Do not search around the restriction.
- `conflict`: re-read state when useful; do not overwrite or rename blindly.
- `invalid_input`: correct path, selector, limits, operation shape, Markdown heading constraints, or front matter before retrying.
- `unsupported`: explain that the requested operation is outside the available Wiki tools.
- `internal_error`: report the failure without claiming the change succeeded; retry only when safe.

After a write, use the returned `path`, `revision`, and `instance_id` as authoritative. Summarize what changed and clearly state any part that could not be completed.
````
