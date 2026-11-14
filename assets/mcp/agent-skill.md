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
| Read lightweight current state | `get_page_state` | Get an opaque `state_token` without the page body. |
| Wait for a known page to change | `wait_page_change` | Pass the exact token as `expected_state`; timeout is a normal result. |
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
| Upload a page asset | `upload_page_asset` | Supply `path`, `file_name`, `mime_type`, and exactly one of `text` or `data_base64`; local file paths are not accepted. |
| Delete a page asset | `delete_page_asset` | Delete a normal page asset identified by `path + file_name`. |

## Read efficiently

1. Resolve the requested Wiki page path.
2. For a small or necessarily complete page, call `get_page`.
3. For a large page or a request about one topic, call `get_page_toc`, choose the section, then call `get_page_section` using `{ "by": "id", "value": "..." }` when possible.
4. Treat section IDs as dynamic and valid only within the corresponding revision. Do not reuse one across revisions.
5. If a title is duplicated, do not use a title selector; obtain the TOC and use the section ID.

`get_page_section.content` excludes the selected heading line and includes its descendants until the next heading at the same or higher level.

## Wait for page state changes

Use this workflow when the user asks you to wait for a human to save a known
page. A waiting call does not start an inactive agent on its own.

1. Resolve the absolute Wiki path and call `get_page_state` once. It returns
   `path`, `revision`, nullable `instance_id`, and opaque `state_token`.
2. Call `wait_page_change` with that `path`, the token as `expected_state`,
   and optional integer `timeout_seconds` from 1 to 30 (default 30).
3. If `status` is `timeout`, continue only while waiting is still requested.
   Repeat with the same token; do not fetch a new baseline between calls,
   since that would hide a change occurring between waits.
4. If `status` is `changed`, waiting is complete. `change` is `updated`,
   `replaced`, or `unavailable`. For available pages, retrieve the body with
   `get_page` if needed and continue the requested work. The retrieved body
   may already be newer than the change result.

For a non-interactive client, prepare permission for the intended read-back
tool (such as `get_page`) before waiting. Approval for the wait tool does not
grant approval for another tool. Keep any temporary tool approval within the
user-authorized task and the server's read scope and path restrictions.

The wait input is shaped like this; replace the example value with the exact
token returned by the server:

```json
{"path":"/example","expected_state":"<state_token from get_page_state>","timeout_seconds":30}
```

`unavailable` has `state: null`: the path no longer resolves to a normal page.
Stop waiting; do not infer whether it was deleted, renamed, or became a draft,
and do not follow an undisclosed destination. Initial `get_page_state` on an
unavailable page returns `not_found`; waiting for a missing page to be created
is not supported. A disappearance between baseline retrieval and the first
wait is still reported as `changed`/`unavailable`.

These tools compare snapshots, not an event history. Changes that occur and
revert between checks may be unobserved. `state_token` remains usable when
legacy `instance_id` is null; it is not an authentication credential. Every
call still requires Bearer authentication, read scope, and path authorization.
Authorization is rechecked during waiting; treat `forbidden` as an error,
not a page change. Cancel when waiting is no longer requested. For waits that
span minutes, repeat bounded calls and ensure the client's tool timeout leaves
room for the server's response. Server tool availability remains authoritative.

## Wait for answers on a questionnaire

When the user asks you to respond or resume work automatically after a known
Wiki page is answered:

1. Call `get_page_state` for the page, then read the answer section. If the
   requested answers are already present, proceed with the requested follow-up.
2. Otherwise call `wait_page_change` on the same path with the baseline
   `state_token` as `expected_state`. On a normal `timeout`, keep the same
   token while the user still wants you to wait.
3. On `changed/updated`, retrieve the page or answer section again and check
   for the requested answers. If an edit was unrelated or the answers are
   incomplete, use the state token returned by that update for the next wait.
   When the answers are present, respond or resume the authorized task.
4. On `replaced`, `unavailable`, or an authorization or other tool error, stop
   this workflow and report the status. Stop if the user cancels.

Each call requires the connection's read scope and authorization for the path.

## Browse and search pages

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
- Do not include `text`, `data_base64`, asset bytes, Bearer tokens, Authorization headers, lock tokens, internal storage paths, or DB internals in logs, summaries, or Wiki notes.

### Focused edits

When a localized edit is requested, do not conclude that `edit_page` is unavailable merely because it was omitted from a previously exposed or discovered subset of MCP tools. Some hosts expose tools lazily or return only a filtered subset during tool discovery. Do not treat permission denial, authorization failure, or `forbidden` as tool unavailability. Never use `update_page` or another broader write tool to work around such a restriction.

If `edit_page` is not currently available to call, use the host's tool discovery mechanism, when available, to look specifically for `edit_page` or localized Wiki editing before falling back to `update_page`.

Immediately before `edit_page`, obtain the latest `revision` and `instance_id`
from `get_page` or `get_page_toc`. Pass both values back unchanged and specify
`operation` as a nonempty array, including for a single edit. An operation
object without the array wrapper is invalid.

Resolve every `section` and `anchor` against that initial source. IDs and
title selectors retain their original target throughout the batch. Inserting
or deleting earlier headings must not redirect an ID to another section.
Operations run in array order, and `replace_text` searches the current
intermediate source. `replace_section.title` preserves the original target.

An original section deleted by an earlier operation cannot be referenced
later in the batch. Rewriting its heading with `replace_text` makes a later
section reference invalid. Replacing text with the identical text retains
the target. Use `replace_section.title` for heading renames.

Build a new section completely in `insert_section.content`. To reference
that new section or a changed title with a selector, save and retrieve a
fresh TOC in a subsequent request. The original ID or title can still refer
to a section renamed with `replace_section.title` within the batch.

All operations must succeed before the final source is saved once. On a
revision or instance ID conflict, fetch the latest source and reconsider
the entire array. On `internal_error`, fetch the current page before retrying
because a post-commit synchronization failure may leave the source saved.
Operation-application errors identify the failing operation with a
one-based number in the message.

Each array element uses one of these operations:

- `replace_section`: replace a section body. Do not include the target heading line in `content`. Use `title` to rename the heading. Existing subsections remain by default; set `include_subsection: true` only when replacing them too.
- `insert_section`: include a complete section, including its heading, and place it `before` or `after` an anchor.
- `delete_section`: delete the selected section.
- `replace_text`: make `old_text` specific enough to identify the intended text. Use `occurrence: all` only when every match should change.

Example for initial TOC IDs `s-001` and `s-002`:

```json
{
  "path": "/example",
  "revision": 7,
  "instance_id": "value-from-get_page_toc",
  "operation": [
    {
      "type": "insert_section",
      "anchor": {"by": "id", "value": "s-001"},
      "placement": "before",
      "content": "# Notice\nText\n"
    },
    {
      "type": "replace_section",
      "section": {"by": "id", "value": "s-002"},
      "content": "Updated body of the original second section"
    }
  ]
}
```

The second element edits the original `s-002`, even after the insertion.
For a single edit, use an array containing just that one element.

For section selectors, prefer `{ "by": "id", "value": "..." }` after a fresh TOC read. A string selector is interpreted as a heading title.

If `edit_page` returns `not_latest_revision` or `instance_id_not_match`, fetch the current page again, reassess the requested change against the new content, and retry only if the intent remains safe. Never reuse stale concurrency tokens.

## Upload text assets

Use `text` only for a concrete `text/*` MIME type with an absent or UTF-8
charset. Use `data_base64` for other bytes, including application/json.
Exactly one body field is required; null and specifying both are invalid.
Empty text is allowed; empty Base64 remains invalid. Preserve text exactly:
do not trim, normalize Unicode, remove BOM, or convert line endings.
The raw limit is 10MiB measured in UTF-8 bytes for text. Base64 has an
encoded limit of 13,981,016 bytes and the same raw limit. The HTTP body
limit is 64MiB including JSON escapes and envelope. Retrieval still uses
get_page_asset_data with Base64 output.

Text asset upload example (the target page must already exist):

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "upload_page_asset",
    "arguments": {
      "path": "/example",
      "file_name": "sample.md",
      "mime_type": "text/markdown; charset=utf-8",
      "text": "# Example\n\nHello.\n"
    }
  }
}
```


## Write portable resource references

Use `{{mcp:authority}}` in a resource reference, for example
`luwiki://{{mcp:authority}}/pages/manual`. Only this exact spelling expands;
the macro adds no scheme or path. It expands at browser rendering/preview
and page-resource or original prompt retrieval, including code.
Saved source and raw get_page stay unchanged. Inserted prompt argument values
are not reprocessed. An odd number of immediately preceding backslashes
suppresses expansion; source backslashes are retained.
Template-only `{{!mcp:authority}}` is unsupported and stays literal.
Reconnect/reload after a server restart to observe changed configuration.

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
