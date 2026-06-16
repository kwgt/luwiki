# luwiki

ローカル運用を目的とした、Rust製の軽量Wikiシステムです。

## v0.9.25以前からの既存データ利用時の注意

v0.9.25以前のバージョンで作成・運用していた既存データを利用する場合は、
front matter対応およびfront matter由来派生データ対応後の状態に合わせるため、
アップデート後に次の再構成コマンドを実行してください。

```sh
luwiki fts rebuild
luwiki derived rebuild --target all
```

`fts rebuild`は、全文検索インデックスを現行スキーマで作り直します。
特に、front matter対応前に作成された全文検索インデックスには
`front_matter`フィールドが存在しないため、再構築しないまま検索やページ更新を行うと
FTS関連エラーになる場合があります。

`derived rebuild --target all`は、ページ正本からテンプレート、MCP prompts、
MCP resourcesなどのfront matter由来派生データを再構成します。

安全のため、これらのコマンドはサーバを停止するか、少なくともページ更新などの
書き込み操作が行われない状態で実行してください。

## 主な機能

- Markdownページの作成、編集、履歴管理、rename、削除・復元
- YAML front matterの編集、検証、表示時の除外
- 本文、見出し、コード、front matterを対象とした全文検索
- front matterおよびlegacy設定に対応したテンプレート機能
- マクロ、Mermaid、数式、GitHub風チェックボックス
- アセット管理
- バックアップ／マイグレート用のexport/import
- Basic認証、Bearer認証、スコープ、path prefix制約
- MCP toolsによるページ操作
- MCP標準`prompts/list`・`prompts/get`
- MCP標準`resources/list`・`resources/read`
- front matter由来派生データの再構成

要求仕様と設計の詳細は[`docs`](docs/)を参照してください。

## ビルド方法

### 前提

- Rust 2024 editionを利用できるRust toolchain
- Node.js
- npm

フロントエンドの生成物は`frontend/dist`へ出力され、Rust側の`rust-embed`によって
実行ファイルへ埋め込まれます。そのため、フロントエンドを先にビルドしてください。

### リリースビルド

```sh
npm --prefix frontend ci
npm --prefix frontend run build
cargo build --release
```

生成される実行ファイル:

```text
target/release/luwiki
```

### デバッグビルド

```sh
npm --prefix frontend ci
npm --prefix frontend run build:debug
cargo build
```

生成される実行ファイル:

```text
target/debug/luwiki
```

`frontend`の依存パッケージが導入済みで、`package-lock.json`に変更がない場合は、
2回目以降の`npm ci`を省略できます。

## 使用方法

```text
luwiki [OPTIONS] [COMMAND]
```

主なコマンド:

| コマンド | 用途 |
|:--|:--|
| `run` | サーバの起動 |
| `derived` | front matter由来派生データの管理 |
| `user` | ユーザ管理 |
| `page` | ページ管理 |
| `lock` | ロック管理 |
| `asset` | アセット管理 |
| `fts` | 全文検索の管理 |
| `token` | Bearerトークン管理 |
| `export` | バックアップ／マイグレート用データのexport |
| `import` | エクスポートデータのimport |
| `commands` | サブコマンド一覧の表示 |
| `help-all` | 全サブコマンドのヘルプ表示 |

front matter由来の派生データは、次のコマンドで再構成できます。

```sh
luwiki derived rebuild --target templates
luwiki derived rebuild --target prompts
luwiki derived rebuild --target resources
luwiki derived rebuild --target all
```

`--template-root`は、通常運用のテンプレート判定条件ではなく、
テンプレート再構成時にlegacy候補を取り込むための入力元です。
`prompts` / `resources` targetでは使用せず、`all` targetではtemplates側だけに適用します。

利用可能なオプションと各サブコマンドの詳細は、次を参照してください。

```sh
luwiki --help
luwiki <COMMAND> --help
```

CLI仕様は[`docs/CLI_SPECS.md`](docs/CLI_SPECS.md)に記載しています。

## ブートストラップ

リリースビルドした実行ファイルを使う場合:

1. ユーザを追加します。

   ```sh
   target/release/luwiki user add <USER-NAME>
   ```

2. サーバを起動します。

   ```sh
   target/release/luwiki run
   ```

3. ブラウザで<http://127.0.0.1:8080/wiki>を開きます。

## MCPサーバ機能

MCPサーバ機能を有効にする場合は、`run`へ`--mcp`を指定します。

```sh
luwiki run --mcp
```

MCP endpointはBearer認証を必要とし、ページ操作用tools、
MCP標準`prompts/list`・`prompts/get`、
MCP標準`resources/list`・`resources/read`を提供します。

セットアップ例は[`MCP_SETUP.md`](MCP_SETUP.md)を参照してください。
promptsの外部仕様は
[`docs/MCP_PROMPT_SPECS.md`](docs/MCP_PROMPT_SPECS.md)に記載しています。
resourcesの外部仕様は
[`docs/MCP_RESOURCE_SPECS.md`](docs/MCP_RESOURCE_SPECS.md)に記載しています。

## Todo
  - [x] TLS対応
  - [x] 全文検索の実装
  - [x] 新規ページ作成インタフェースの追加(URL直打ち or リンク埋め込みでの新規ページ作成は可能)
  - [x] ページ移動(リネーム)実装
  - [x] ページ削除の実装
  - [x] amend更新の対応
  - [x] ゾンビページ(削除済みページ)の管理画面
  - [x] リビジョン管理画面
  - [x] エディタ導入(CodeMirror)
  - config.toml
      - [X] Wiki名の表示名設定
      - [X] アセットアップロードの上限サイズ指定
      - [ ] アイコン設定
  - [x] バックアップ(インポート／エクスポート)機能の実装
  - Markdown追加機能
      - [x] Github風チェックボックス
      - [x] Mermaid対応 (Mermaid.js組み込み)
      - [x] 数式対応 (@mdit/plugin-katexを使用)
  - [x] テンプレート機能
  - マクロ機能
      - [x] コメント記述マクロ
      - [x] 子ページリスト生成マクロ
      - [x] 単純リンク生成
      - [x] アセットのコードブロック展開マクロ
      - [x] ページ名展開
  - [x] Bearer認証のサポート
  - [x] YAML front matter対応
  - [x] front matter由来派生データの再構成
  - MCPサーバ機能
      - tools
          - ページ本文の操作
              - [x] get_page
              - [x] get_page_toc
              - [x] list_pages
              - [x] search_pages
              - [x] create_page
              - [x] update_page
              - [x] append_page
              - [x] rename_page
              - [x] get_page_section
              - [x] edit_page
          - [ ] アセットの操作
      - resources (未テスト)
          - [x] 固定組み込みresource
              - [x] front matter詳細仕様
          - [x] ページ由来resource
              - [x] resources/list
              - [x] resources/read
      - prompts (未テスト)
          - [x] prompts/list
          - [x] prompts/get

## ライセンス
このソフトウェアは[MITライセンス](https://opensource.org/licenses/MIT)の条件下でオープンソースとして利用可能です。
