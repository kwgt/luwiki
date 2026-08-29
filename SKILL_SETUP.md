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
description: Use an MCP-enabled Wiki to find, read, create, edit, append, rename Markdown pages, and manage page assets through get_page, get_page_toc, get_page_section, list_pages, search_pages, create_page, update_page, edit_page, append_page, rename_page, list_page_assets, resolve_page_asset, get_page_asset_metadata, get_page_asset_data, upload_page_asset, and delete_page_asset. Use whenever a request involves this Wiki's pages, assets, Wiki paths or links, page sections, revisions, or Wiki content maintenance. Apply the Wiki-specific relative-path rule in which the current page itself—not its parent—is the resolution base.
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

環境に合わせて以下の変更を行って下さい。

- `dependencies.tools[].value`には登録したMCPサービス名を設定してください。
- `dependencies.tools[].url`には登録したMCPサービスのURLを設定してください。

なお、Luwikiを使用したWikiサービスをMCPサーバとして追加した場合は同じフォーマットで `dependencies.tools` にエントリを追加することにより対応できます。

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
description: Use Luwiki whenever a request involves Wiki pages, page assets, Wiki content, Wiki paths or links, page sections, or project/agent scratchpad content. Use it for finding, reading, creating, localized section or text editing, full-page replacement, appending, renaming Wiki pages, and listing, resolving, retrieving, uploading, or deleting page assets. In particular, treat requests to modify part of an existing page, update a section, replace specific text, or manage assets attached to a page as Luwiki tasks.
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
  - mcp__local_wiki__list_page_assets
  - mcp__local_wiki__resolve_page_asset
  - mcp__local_wiki__get_page_asset_metadata
  - mcp__local_wiki__get_page_asset_data
  - mcp__local_wiki__upload_page_asset
  - mcp__local_wiki__delete_page_asset
---
```

なお、`allowed-tools` に記述するツール名の `local_wiki` となっている箇所を登録したMCPサービス名に置き換えてください。

## SKILL.md本文

````markdown
# Luwiki MCP

Luwiki is a Markdown-based Wiki system.

Use Luwiki's MCP tools and their runtime results as the source of truth for current state and capabilities. This skill defines Luwiki-specific workflows, conventions, and expected tool roles, but does not by itself prove that a named tool is currently available.

Prefer the least-destructive tool that directly matches the requested operation. Do not emulate a narrow operation with a broader write operation when the narrow tool is available.

## Resolve relative page paths

Luwiki treats every Wiki page as both content and a container. Resolve a relative path from the current page path itself, not from the page's parent.

- Current page `/a/b` + relative path `c/d` => `/a/b/c/d`
- Do **not** resolve that example to `/a/c/d`; that is filesystem file semantics.
- Keep an absolute path unchanged.
- Do not create or request Wiki page paths that contain whitespace characters. Ask the user for a whitespace-free path when the intended path contains spaces, tabs, or other whitespace.
- Establish the current page before resolving a relative reference. If the request does not identify it and context does not make it unambiguous, ask for the current page path.
- Resolve relative references to absolute Wiki paths before calling a page tool.
- Apply the server's documented path normalization and validation after selecting the current page as the base. Do not guess behavior for ambiguous dot segments or other syntax not defined by the Wiki contract.

Use the same rule for links found in page content, user-supplied relative page references, and destination paths derived from a current page. A page can therefore have child pages even though it also contains Markdown content.

## Resolve asset references

LuWiki asset references identify an owning page and a file name.

A complete asset reference has the form `asset:<asset-spec>`. In this section,
`<asset-spec>` does not include the leading `asset:` prefix.

Resolve the page portion using the same Wiki-specific rule as page links:
the current page itself is the base for relative paths.

For current page `/a/b`:

- `asset:image.png` or `asset:./image.png` resolves to `path=/a/b` and
  `file_name=image.png`.
- `asset:./child:image.png` resolves to `path=/a/b/child` and
  `file_name=image.png`.
- `asset:../sibling:image.png` resolves to `path=/a/sibling` and
  `file_name=image.png`.
- `asset:/shared/images:image.png` resolves to `path=/shared/images` and
  `file_name=image.png`.

When calling page-asset MCP tools, do not pass the combined `asset:`
reference. Pass the resolved absolute page `path` and `file_name` as
separate tool arguments.

Do not invent an asset page path or file name. When the target asset has not
already been established by the user or current page content, use
`list_page_assets` or `resolve_page_asset` to verify it before authoring a
reference.

## Author asset references and macros

When writing raw Wiki Markdown that refers to page assets, use LuWiki's asset
reference syntax instead of inventing an HTTP URL or local file path.

For an asset belonging to the current page:

- Embed it with Markdown: `![](asset:./image.png)`.
- Embed it with Obsidian-compatible syntax: `![[asset:./image.png]]`.
- Render a text asset as a code block:
  `{{include_code:src=./sample.rs:lang=rust}}`.
- Render a CSV asset as a table: `{{include_csv:src=./table.csv}}`.

For `include_code` and `include_csv`, pass the asset spec in `src` without
the leading `asset:` prefix.

- `include_code` requires a `text/*` asset. If `lang` is omitted, LuWiki
  infers it from the MIME type.
- `include_csv` requires a `text/csv` asset.

These constructs are stored as raw Markdown and expanded by LuWiki at
rendering time. Do not replace them with expanded output when creating or
editing page source.

Use `get_page` or the relevant page section tool to preserve existing macro
syntax during edits. Use page-asset tools only when an asset must be listed,
resolved, read, uploaded, or deleted.

## Choose the narrowest tool

| Need | Tool | Key guidance |
| --- | --- | --- |
| Read a whole page | `get_page` | Use when the complete raw Markdown is needed. |
| Inspect page structure | `get_page_toc` | Get headings, dynamic section IDs, and approximate section sizes without loading the full page. |
| Read one section | `get_page_section` | Prefer after TOC inspection for focused retrieval. |
| Browse a subtree | `list_pages` | Page through path-sorted results with `next_cursor` while `has_more` is true. |
| Search by content | `search_pages` | Explicitly select one or more targets: `headings`, `body`, `code`, `front_matter`. This is top-N search, not cursor pagination. |
| Create a page | `create_page` | Supply the final absolute destination path and complete initial raw Markdown. |
| Replace a whole page | `update_page` | Use only when complete raw-source replacement is the intended operation. For a localized change, use it only as a last resort after `edit_page` has been specifically sought through the host's available tool-discovery mechanism and confirmed unavailable for reasons other than authorization or permission restrictions. |
| Make a focused edit | `edit_page` | Prefer for section or text changes; first obtain current concurrency tokens. |
| Add content at the end | `append_page` | Supply only the exact suffix, including any needed newlines. |
| Change a page path | `rename_page` | Supply the final absolute destination in `rename_to`, not merely a parent path. |
| List page assets | `list_page_assets` | Return metadata for normal assets attached to the page. |
| Resolve a page asset | `resolve_page_asset` | Resolve `path + file_name` to the asset's minimal identity. |
| Read asset metadata | `get_page_asset_metadata` | Return metadata only; do not request the asset body. |
| Read asset data | `get_page_asset_data` | Return metadata plus `data_base64`; use only when the body is needed. |
| Upload a page asset | `upload_page_asset` | Supply `path`, `file_name`, `mime_type`, and `data_base64`; local file paths are not accepted. |
| Delete a page asset | `delete_page_asset` | Delete a normal page asset identified by `path + file_name`. |

## Read efficiently

1. Resolve the requested Wiki page path.
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
- Follow the tool-selection rules above. Never use a broader write tool to bypass an unavailable or unauthorized narrower operation.
- Use `append_page` only for literal end-of-page additions. Include `\n` in `content` when line separation is required; the server does not add it.
- Preserve existing front matter unless the request explicitly changes it. For `create_page` and `update_page`, `content` may be the complete raw source including front matter.
- Do not use `append_page` to edit front matter.
- Do not attempt restore, deleted-page access, locks, templates, outgoing-link lists, or backlink lists; the initial tool set does not support them.
- For assets, use only the initial page-asset tools listed above. Do not attempt `asset_id`-only operations, local-file-path uploads, overwrites, lock-token uploads/deletes, deleted asset access, asset restore, asset rename/copy/move, or MIME-only updates.
- Do not include `data_base64`, asset bytes, Bearer tokens, Authorization headers, lock tokens, internal storage paths, or DB internals in logs, summaries, or Wiki notes.

### Focused edits

When a localized edit is requested, do not conclude that `edit_page` is unavailable merely because it was omitted from a previously exposed or discovered subset of MCP tools. Some hosts expose tools lazily or return only a filtered subset during tool discovery. Do not treat permission denial, authorization failure, or `forbidden` as tool unavailability. Never use `update_page` or another broader write tool to work around such a restriction.

If `edit_page` is not currently available to call, use the host's tool discovery mechanism, when available, to look specifically for `edit_page` or localized Wiki editing before falling back to `update_page`.

Immediately before `edit_page`, obtain the latest `revision` and `instance_id` from `get_page` or `get_page_toc`. Pass both values back unchanged with exactly one operation:

- `replace_section`: replace a section body. Do not include the target heading line in `content`. Use `title` to rename the heading. Existing subsections remain by default; set `include_subsection: true` only when replacing them too.
- `insert_section`: include a complete section, including its heading, and place it `before` or `after` an anchor.
- `delete_section`: delete the selected section.
- `replace_text`: make `old_text` specific enough to identify the intended text. Use `occurrence: all` only when every match should change.

For section selectors, prefer `{ "by": "id", "value": "..." }` after a fresh TOC read. A string selector is interpreted as a heading title.

If `edit_page` returns `not_latest_revision` or `instance_id_not_match`, fetch the current page again, reassess the requested change against the new content, and retry only if the intent remains safe. Never reuse stale concurrency tokens.

## Rename safely

Resolve both source and destination as absolute Wiki page paths. Verify that `rename_to` is the final target. Do not rename the root page, and do not move a page beneath its own current path. Both paths must be permitted by the active path-prefix authorization.

## Handle errors

- `not_found`: verify the resolved absolute path or revision; do not assume a deleted page can be read.
- `forbidden`: stop and report that the required scope or path-prefix permission is missing. Do not search around the restriction.
- `conflict`: re-read state when useful; do not overwrite or rename blindly.
- `invalid_input`: correct path, selector, limits, operation shape, Markdown heading constraints, or front matter before retrying.
- `unsupported`: explain that the requested operation is outside the available Luwiki tools.
- `internal_error`: report the failure without claiming the change succeeded; retry only when safe.

After a write, use the returned `path`, `revision`, and `instance_id` as authoritative. Summarize what changed and clearly state any part that could not be completed.
````
