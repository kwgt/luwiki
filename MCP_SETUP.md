# MCPサーバ機能のセットアップ例

MCPサーバ機能を有効にすると、AIエージェント（Codex CLI / Claude Code など）からWikiのページ操作（取得・更新・検索など）を行うことができます。

本書ではMCPサーバ機能を有効にする場合の設定例を記述します。

## 手順
以下の手順で準備。

  1. エージェント用アカウントの作成
  2. アクセストークンの発行
  3. エージェントの設定追加
  4. サーバの起動

以下に個々の手順を記述します。

### エージェント用アカウントの発行
`user add`コマンドでAIエージェント用のアカウントを作成します。このとき、属性に"no_basic_auth"を付与し、ブラウザからログインできないようにしておいてください。

```sh
luwiki user add --attribute no_basic_auth -d "AI-Agent" agent
```

### アクセストークンの発行
`token create`コマンドでAIエージェント用のアクセストークンを生成します。以下の様にアクセストークンの生成を行います。

```sh
luwiki token create --scope read,create,update --path-prefix "/JailPage" agent
```

オプションの意味は以下のとおりです。

- `--scope`: 許可する権限を指定。read, create, delete, update, appendが指定可能。writeはすべてを指定する場合のエイリアス。指定しなかった場合はwriteを指定した場合と同じ。可能な限り最小限の権限で運用してください。
- `--path-prefix` : アクセス可能なページのパスプレフィクスを指定。複数指定可。指定しなかった場合は全ページにアクセス可能。
以下の例では、/JailPage以下のページに対し読み書き自由な権限を持つアクセストークンを生成します。

上記を実行しアクセストークンが生成されると以下のような表示が行われます。

```text
TOKEN ID:     01KN1649ABNQ9HCSNYW01F8RNT
TOKEN NAME:   -
USERNAME:     agent
SCOPES:       read, create, delete, update
PERMISSIONS:  read, create, delete, update
TTL:          30d
PATH PREFIXES:
    - /JailPage
TIMESTAMPS:
    create: 2026-03-31T14:33:41
    expire: 2026-04-30T14:33:41

TOKEN VALUE:
    9AVqOTCQ/h4igewTZE5HhZ5K0eFwGRPCwkr0fXBInro=
```

上記の"TOKEN VALUE"の欄に表示された文字列がアクセストークンになります(上記の例であれば`9AVqOTCQ/h4igewTZE5HhZ5K0eFwGRPCwkr0fXBInro=`)。このアクセストークンは生成時にしか表示されないので注意してください。

### エージェントの設定追加
#### Codex CLIの場合
`~/.codex/config.toml`を編集し以下のようなエントリを追加します。

```toml
[mcp_servers.local_wiki]
url = "https://${サーバのアドレス}:8080/mcp"
http_headers = { "Authorization" = "Bearer ${アクセストークン}" }
```

#### Claude Codeの場合
##### claudeコマンドを使用する場合
以下の様に`mcp add`サブコマンドを用いて登録を行います(※動作確認は行っていません)。

```sh
claude mcp add local_wiki --transport http --header "Authorization: Bearer ${アクセストークン}" "https://${サーバのアドレス}:8080/mcp"
```

##### settings.jsonを編集する場合
`~/.claude.json`を編集し、"mcpServers"に以下のようなエントリを追加します

```json
"mcpServers": {
  "local_wiki": {
    "type": "http",
    "url": "https://${サーバのアドレス}:8080/mcp",
    "headers": {
      "Authorization": "Bearer ${アクセストークン}"
    }
  }
}
```

#### Gemini CLIの場合
`~/.gemini/settings.json`を編集し、"mcp_servers"に以下のようなエントリを追加します(※動作確認は行っていません)。

```json
"mcp_servers": {
  "local_wiki": {
    "httpUrl": "https://${サーバのアドレス}:8080/mcp",
    "headers": {
      "Authorization": "Bearer ${アクセストークン}"
    }
  }
}
```

### サーバの起動
以下の様に`run`コマンドに`--mcp`オプションをつけてサーバを起動すると、MCPサーバ機能が有効になります。

```sh
luwiki run --mcp
```

## その他
- Wikiサーバを自己署名証書で運用している場合、Codex CLIなどの一般的なエージェントの場合署名検証で弾かれます(アクセスしてくれない)。この場合、Codex CLIでは環境変数SSL_CERT_FILEに、Claude Codeでは環境変数NODE_EXTRA_CA_CERTSに証明書ファイルへのパスを設定してサーバを起動してください。

