# luwiki

ローカル運用を目的とした、Rust製の軽量Wikiシステムです。Markdownでページを記述し、
ブラウザから閲覧・編集できます。データはredbに保存し、フロントエンドは実行ファイルに埋め込みます。

## 主な機能

- Markdownページの作成・編集、リビジョン管理、差分表示、移動、削除・復元
- YAML front matter、テンプレート、マクロ
- Mermaid、数式、GitHub風チェックボックス
- 本文・見出し・コード・front matterの全文検索
- 画像や添付ファイルなどのアセット管理
- バックアップ／マイグレート用のexport/import
- Basic認証、Bearer認証、トークンスコープとpath prefixによるアクセス制限
- MCP tools、prompts、resourcesによる外部クライアントとの連携
- TLS、監査ログ

## ビルド方法

### 必要な環境

- Rust 2024 editionを利用できるRust toolchain
- Node.jsとnpm（`frontend/package-lock.json`の依存パッケージを利用できる環境）
- WindowsではMSVC toolchainとVisual Studio 2022のC/C++ビルド環境

WindowsのCargoコマンドは、Visual Studio 2022のDeveloper Command Promptで実行してください。
`where cl`と`cl`で、コンパイラのパスとバージョンがVisual Studio 2022のものであることを確認します。
Visual Studio 2026付属のコンパイラは使用しません。
詳しい環境条件は[プロジェクト固有の制約事項](docs/PROJECT_CONSTRAINTS.md)を参照してください。

以下のコマンドはリポジトリのルートで実行します。フロントエンドの成果物を
`frontend/dist`から実行ファイルへ埋め込むため、npmによるビルドを先に行ってください。

### リリースビルド

```sh
npm --prefix frontend ci
npm --prefix frontend run build
cargo build --release --locked
```

実行ファイルは`target/release/luwiki`に生成されます（Windowsでは`target/release/luwiki.exe`）。

### デバッグビルド

```sh
npm --prefix frontend ci
npm --prefix frontend run build:debug
cargo build --locked
```

実行ファイルは`target/debug/luwiki`に生成されます（Windowsでは`target/debug/luwiki.exe`）。
依存パッケージが導入済みで`frontend/package-lock.json`に変更がない場合は、
2回目以降の`npm ci`を省略できます。

## 初回起動

以下では実行ファイルを`luwiki`として記述します。ビルド直後は、使用するシェルに応じて
`./target/release/luwiki`や`.\target\release\luwiki.exe`に読み替えてください。

1. ブラウザで利用するユーザを登録します。

   ```sh
   luwiki user add --display-name "My Name" myuser
   ```

   プロンプトに従ってパスワードを登録します。ユーザ未登録ではサーバを起動できません。

2. ローカルマシンから接続できるアドレスでサーバを起動します。

   ```sh
   luwiki run 127.0.0.1:8080
   ```

3. ブラウザで<http://127.0.0.1:8080/wiki>を開き、登録したユーザ名とパスワードでBasic認証を行います。

初回起動時には埋め込みの雛形からルートページが作成されます。
以降はブラウザからページを作成・編集できます。

設定ファイルは省略できます。デフォルトではOSごとのユーザ用ディレクトリにデータを保存します。
保存先を変更する場合は、ユーザ登録時とサーバ起動時に同じ設定ファイルまたは保存先オプションを指定してください。

## CLIの概要

```text
luwiki [OPTIONS] [COMMAND]
luwiki [OPTIONS] run [COMMAND-OPTIONS] [BIND-ADDR[:PORT]]
```

共通オプションはサブコマンドの前に、サブコマンド固有のオプションはその後に指定します。

### 共通オプション

| オプション | 内容 |
|:--|:--|
| `-c, --config-path FILE` | 使用するTOML設定ファイル |
| `-d, --db-path FILE` | redbデータベースの保存先 |
| `-I, --fts-index DIR` | 全文検索インデックスの保存先 |
| `-a, --assets-path DIR` | アセットの保存先 |
| `-l, --log-level LEVEL` | ログレベル（`none`、`error`、`warn`、`info`、`debug`、`trace`。既定は`info`） |
| `-L, --log-output PATH` | ログの出力先ファイルまたはディレクトリ |
| `--log-tee` | ログを標準出力にも出力 |
| `-T, --wiki-title TITLE` | Wikiの表示名（既定は`LUWIKI`） |
| `-S, --asset-limit-size SIZE` | アップロード上限（既定は`10M`、最大`100M`。`K`、`M`はKiB、MiB） |
| `-t, --template-root PATH` | テンプレート再構成時に取り込むlegacy候補のWikiパス |
| `--audit-log-dir DIR` | 監査ログの出力先 |
| `--audit-log-retention DURATION` | 監査ログの保持期間（既定は`90d`。`d`、`h`、`m`を使用可能） |
| `--audit-log-rotate-size SIZE` | 監査ログのローテーション閾値（既定は`2M`） |
| `--show-options` | 解決済みの設定を表示して終了 |
| `--save-config` | 設定を保存して終了 |
| `--help` / `--version` | ヘルプ／バージョンの表示 |

### サーバ起動オプション

| 引数・オプション | 内容 |
|:--|:--|
| `BIND-ADDR[:PORT]` | 待ち受け先。設定未指定時は`0.0.0.0:8080` |
| `-b, --open-browser` | 起動時にブラウザを開く |
| `--mcp` | MCPサーバ機能を有効化 |
| `-T, --tls` | HTTPSを有効化 |
| `-C, --cert FILE` | PEM形式のサーバ証明書 |
| `-K, --key FILE` | PEM形式の秘密鍵（証明書の指定も必要） |
| `--win-service` | Windowsサービスとして起動（Windowsのみ） |

```sh
luwiki run --open-browser 127.0.0.1:8080
luwiki run --tls --cert server.pem --key server-key.pem 127.0.0.1:8443
```

`--key`を省略する場合は、証明書のPEMに秘密鍵も含めてください。
`--tls`指定時に証明書パスを明示しなければ、デフォルトの`server.pem`を使用し、
存在しない場合は自己署名証明書を自動生成します。明示した証明書ファイルが存在しない場合はエラーです。

### 管理コマンドとヘルプ

| コマンド | 用途 |
|:--|:--|
| `user` | ユーザの登録・一覧・変更・削除 |
| `page` | ページの追加・一覧・移動・削除・復元 |
| `lock` | 編集ロックの一覧・解除 |
| `asset` | アセットの追加・一覧・移動・削除・復元 |
| `fts` | 全文検索、インデックスの再構築・マージ |
| `derived` | front matter由来の派生データの再構成 |
| `token` | Bearerトークンの発行・確認・無効化、path prefixの管理 |
| `export` / `import` | バックアップ／マイグレート用データの出力・取り込み |
| `commands` | サブコマンド一覧の表示 |
| `help-all` | 全サブコマンドのヘルプ表示 |

```sh
luwiki --help
luwiki run --help
luwiki user add --help
luwiki help-all
```

各コマンドの引数や動作の詳細は[CLI仕様](docs/CLI_SPECS.md)を参照してください。

## 設定ファイル

設定ファイルはTOML形式です。CLIで明示した値が設定ファイルより優先され、
未指定の項目には組み込みのデフォルト値を使用します。
デフォルト位置に設定ファイルがなくても起動できますが、`--config-path`で明示する場合は
そのファイルをあらかじめ作成してください。

### デフォルトの保存先

| 対象 | Linux | Windows |
|:--|:--|:--|
| 設定ファイル | `$XDG_CONFIG_HOME/luwiki/config.toml`（未設定時は`~/.config/luwiki/config.toml`） | `%LOCALAPPDATA%\luwiki\config.toml` |
| データディレクトリ | `$XDG_DATA_HOME/luwiki`（未設定時は`~/.local/share/luwiki`） | `%LOCALAPPDATA%\luwiki` |

データディレクトリ配下には、`database.redb`、`index/`、`assets/`、`log/`、`audit/`、
TLS用の`server.pem`を配置します。実際の設定・保存先は次のコマンドで確認できます。

```sh
luwiki --show-options
luwiki --show-options run
```

### 設定例

以下を`config.toml`として作成すると、設定ファイルと同じディレクトリの`storage/`配下にデータを保存し、
`127.0.0.1:8080`で待ち受けます。

```toml
[global]
wiki_title = "My Wiki"
db_path = "storage/database.redb"
fts_index = "storage/index"
assets_path = "storage/assets"
log_level = "INFO"
log_output = "storage/log"
asset_limit_size = "10M"
audit_path = "storage/audit"
audit_retention = "90d"
audit_rotate_size = "2M"

[run]
bind = "127.0.0.1"
port = 8080
use_tls = false
use_mcp = false
mcp_authority = "local.luwiki"
```

設定ファイル内の相対ファイルパスは、設定ファイルのあるディレクトリを基準に解決します。
設定を使用してユーザ登録・起動する例は次のとおりです。

```sh
luwiki --config-path ./config.toml user add myuser
luwiki --config-path ./config.toml run
```

主な設定テーブルは`[global]`（共通設定）と`[run]`（サーバ起動設定）です。
`[user.list]`、`[page.list]`、`[asset.list]`、`[fts.search]`などでは、
管理コマンドのソート順や検索対象のデフォルト値を設定できます。
Wikiアイコンは`[global]`の`wiki_icon`に画像ファイルのパスを指定します。
HTTPSの証明書・秘密鍵を設定ファイルに保存する場合は、`[run]`の`server_cert`・`server_key`を使用してください。

現在の設定をデフォルト位置の設定ファイルへ保存することもできます。
次の例は設定の保存だけを行い、サーバは起動しません。

```sh
luwiki --wiki-title "My Wiki" --save-config run 127.0.0.1:8080
```

## MCPサーバ機能

MCPを利用する場合は、通常のHTTP/HTTPSサーバ起動に`--mcp`を追加します。
設定ファイルの`[run]`で`use_mcp = true`を指定することもできます。

```sh
luwiki run --mcp 127.0.0.1:8080
```

MCPエンドポイントは`/mcp`です。接続にはBearerトークンを使用します。
エージェント用ユーザとトークンの作成、クライアント側の設定例は
[MCPセットアップ](MCP_SETUP.md)を参照してください。
MCP toolsに加え、標準の`prompts/list`・`prompts/get`、`resources/list`・`resources/read`を提供します。

## 既存データの更新時の注意

v0.9.25以前のバージョンで作成・運用していたデータを利用する場合は、
アップデート後に次のコマンドを実行してください。保存先を変更している場合は、
通常の起動時と同じ`--config-path`などの共通オプションを付けます。

```sh
luwiki fts rebuild
luwiki derived rebuild --target all
```

`fts rebuild`は全文検索インデックスを現行スキーマで作り直します。
front matter対応前のインデックスには`front_matter`フィールドがないため、
再構築せずに検索やページ更新を行うとFTS関連エラーになる場合があります。

`derived rebuild --target all`はページ正本からテンプレート、MCP prompts、MCP resourcesの
派生データを再構成します。`--target templates`、`--target prompts`、`--target resources`で個別に再構成できます。
`--template-root`はlegacyテンプレート候補の取り込み元で、通常運用のテンプレート判定には使用しません。
`all`ではテンプレートの再構成にだけ適用します。

安全のため、再構成コマンドはサーバを停止するか、少なくともページ更新などの
書き込み操作が行われない状態で実行してください。

## 関連ドキュメント

- [要求仕様](docs/REQUIREMENTS.md)
- [CLI・設定ファイル仕様](docs/CLI_SPECS.md)
- [REST API仕様](docs/REST_API_SPECS.md)
- [MCP tools仕様](docs/MCP_TOOL_SPECS.md)、[prompts仕様](docs/MCP_PROMPT_SPECS.md)、[resources仕様](docs/MCP_RESOURCE_SPECS.md)
- [エクスポートデータ仕様](docs/EXPORT_DATA_SPEC.md)

## ライセンス

このソフトウェアは[MITライセンス](LICENSE)の条件下で利用できます。
