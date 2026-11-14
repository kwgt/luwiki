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
- `~/.codex/skills/luwiki/agents` : `openai.yaml` を格納するディレクトリ。

### SKILL.mdの格納

以下の front matter と末尾の[共通本文](#ローカル-skillmd-の共通本文)を連結し `~/.codex/skills/luwiki/SKILL.md` を作成します。

```yaml
---
name: luwiki
description: Use an MCP-enabled Wiki to find, read, create, edit, append, rename Markdown pages, wait for page state changes with get_page_state and wait_page_change, and manage page assets. Use whenever a request involves this Wiki's pages, assets, Wiki paths or links, page sections, revisions, or Wiki content maintenance. Apply the Wiki-specific relative-path rule in which the current page itself—not its parent—is the resolution base.
---
```

### openai.yamlの格納

以下の内容で `~/.codex/skills/luwiki/agents/openai.yaml` を作成します。

文中のプレースホルダは以下のように置き換えてください。

- `{{mcp-service-name}}` : エージェントに登録したMCPサービス名に置き換えてください(`local_wiki`など)。
- `{{mcp-host}}` : エージェントに設定したMCPサーバのホスト部(`localhost`など)。


```yaml
interface:
  display_name: "Luwiki MCP"
  short_description: "Luwikiをエージェントの作業メモとして使用します"
  default_prompt: "Use $luwiki to read and maintain the project scratchpad."

dependencies:
  tools:
    - type: "mcp"
      value: "{{mcp-service-name}}
      description: "Local Wiki and agent scratchpad"
      transport: "streamable_http"
      url: "https://{{mcp-host}}:8080/mcp"

policy:
  allow_implicit_invocation: true
```

なお、Luwikiを使用したWikiサービスをMCPサーバとして追加した場合は同じフォーマットで `dependencies.tools` にエントリを追加することにより対応できます。

## Claude Codeの場合

以下の手順で登録を行います。

1. スキル設定格納用のディレクトリの作成
2. SKILL.mdの格納

### スキル設定格納用のディレクトリの作成
以下のディレクトリを作成してください。

- `~/.claude/skills/luwiki` : `SKILL.md` を格納するディレクトリ

### SKILL.mdの格納

以下の front matter と末尾の[共通本文](#ローカル-skillmd-の共通本文)を連結し `~/.claude/skills/luwiki/SKILL.md` を作成します。

文中のプレースホルダは以下のように置き換えてください。

- `{{mcp-service-name}}` : エージェントに登録したMCPサービス名に置き換えてください(`local_wiki`など)。

```yaml
---
name: luwiki
description: Use Luwiki whenever a request involves Wiki pages, page assets, Wiki content, Wiki paths or links, page sections, or project/agent scratchpad content. Use it for finding, reading, creating, localized section or text editing, full-page replacement, appending, renaming Wiki pages, waiting for known pages to change with get_page_state and wait_page_change, and listing, resolving, retrieving, uploading, or deleting page assets. In particular, treat requests to modify part of an existing page, update a section, replace specific text, or manage assets attached to a page as Luwiki tasks.
allowed-tools:
  - mcp__{{mcp-service-name}}
---
```

## ローカル SKILL.md の共通本文

次の Markdown を各クライアントの front matter の直後に置きます。

文中のプレースホルダは以下のように置き換えてください。

- `{{mcp-service-name}}` : エージェントに登録したMCPサービス名に置き換えてください(`local_wiki`など)。
- `{{mcp-authority}}` : サーバに設定したMCPオーソリティ名に置き換えてください(既定値のまま実行しているのであれば`local.luwiki`に置き換えてください)。

取得には接続先の Bearer トークンに `read` スコープが必要です。

```markdown
# Luwiki

When LuWiki is first needed, load the server's operational instructions in this order:

1. First, call the host's generic MCP resource reader with 
   `server: "{{mcp-service-name}}"` and `uri: "luwiki://{{mcp-authority}}/builtin/agent-skill"`.
   In Codex, the call is:  
   `read_mcp_resource({"server":"{{mcp-service-name}}","uri":"luwiki://{{mcp-authority}}/builtin/agent-skill"})`.
   The reader may appear among generic MCP tools, outside the `{{mcp-service-name}}`
   namespace. If needed, use tool discovery to look for `read_mcp_resource`
   by name. On success, use the returned `contents[].text`.
2. Call `get_agent_instructions` on the same MCP server only if the host
   provides no resource reader or the direct read fails. Use the returned
   `content` field.
3. If neither method returns the instructions, report the failure and stop
   LuWiki operations for that server.

Reuse the instructions in the current context for the same server. Fetch
them again when the server changes or the instructions have been lost from
the conversation context.
```

既存の全文版を利用している場合は、サーバを更新した後、ローカルの
`SKILL.md` を上記の front matter と共通本文へ差し替えます。
MCP 接続を再初期化し、設定した URI から運用指示を取得できることを確認します。
