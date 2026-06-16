# MCPアーキテクチャ設計

本書は、MCP内部設計のうち、
責務配置、モジュール構成、既存 REST API 層との共通化方針を整理するための文書である。

本書は、共通部である `docs/MCP_INTERNAL_DESIGN.md` を前提とし、
現行 `docs/MCP_INTERNAL_DESIGN.md` の以下の章を移設する受け皿として用いる。

- 6. 責務配置に関する設計判断
- 7. モジュール構成案
- 8. 既存 REST API 層との共通化方針

関連する設計文書は以下の通り。

- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - path ベースサービス層、永続化モデル、DB API の詳細を確認する場合に参照する
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - HTTPサーバ統合、`run` コマンド、transport / endpoint の組み込み方針を確認する場合に参照する
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 公開ツール、入出力モデル、エラー応答を確認する場合に参照する
- `docs/MCP_RESOURCE_SPECS.md`
  - resourcesの公開URI、field、cursor、contentsの外部契約を確認する場合に参照する
- `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 監査ログ基盤の独立モジュール構成と記録経路の詳細を確認する場合に参照する

---

## 1. 対象範囲

本書では以下を対象とする。

- 責務配置に関する設計判断
- `src/` 配下のモジュール構成案
- `src/mcp/` 配下の初期構成案
- 既存モジュールへの変更点
- 既存 REST API 層との共通化方針

## 2. 責務配置に関する設計判断

本章の責務配置設計では、以下を基本方針として採用する。

- MCP は「公開面」と「path ベース共通処理」を分離する
- 既存 REST API をそのまま再利用するのではなく、DBアクセス層の上に共通サービス層を置く
- サーバ統合、公開面、共通サービス、DBアクセスの 4 層で責務を分割する
- 監査ログは独立サブシステムとして扱うのではなく、公開面および共通サービスに横断接続する

## 3. モジュール構成案

MCP モジュール構成の初期案として、
既存の `rest_api` / `http_server` / `database` の分割方針に合わせ、
MCP 専用のトップレベルモジュール `src/mcp/` を新設する方針を採る。

### 3.1 トップレベル配置

トップレベルでは以下の責務分割を想定する。

- `src/http_server/`
  - Actix サーバへの組み込み
  - MCP 公開口の有効化判断
  - 共有状態および補助タスクとの統合
- `src/mcp/`
  - MCP 公開面
  - path ベース共通サービス
  - MCP標準prompts / resourcesの公開面
  - MCP 固有の認可接続
  - MCP 応答モデル
- `src/database/`
  - 永続化と低水準 DB API
- `src/rest_api/`
  - 既存 REST API の公開面

MCP は REST API の下位モジュールに入れず、
外部公開面として独立した兄弟モジュールに置く。
これにより、REST API 内部 HTTP 呼び出しではなく、
同一の低水準基盤を共有する別公開面として位置付ける。

### 3.2 `src/mcp/` 配下の初期構成案

`src/mcp/` 配下は、少なくとも以下のモジュール分割を想定する。

- `src/mcp/mod.rs`
  - MCP モジュールの公開入口
  - HTTP サーバから利用する組み込み関数の提供
- `src/mcp/transport.rs`
  - MCP transport / endpoint の Actix 接続
  - リクエスト受理から公開層への橋渡し
- `src/mcp/tools.rs`
  - 公開するツール一覧とツール定義
  - ツール名、入力スキーマ、出力スキーマの定義
- `src/mcp/handler.rs`
  - ツール呼び出しおよび標準primitive操作単位の入口
  - 入力検証、サービス呼び出し、応答整形、監査ログ記録
- `src/mcp/service.rs`
  - path ベース共通サービス層の公開入口
  - read / list / search / create / update / append / rename の業務呼び出し
  - prompts / resources の一覧、取得、URI解決、公開状態判定
- `src/mcp/auth.rs`
  - Bearer 認証文脈との接続
  - MCP 向け認可補助
- `src/mcp/model.rs`
  - MCP 内部の要求・応答モデル
  - path ベースの共通入出力型
  - prompts / resources 用の内部サービス結果型
- `src/mcp/errors.rs`
  - MCP 固有エラー分類
  - 内部エラーから公開エラーへの写像

### 3.3 `src/mcp/` 直下へ置かない責務

以下の責務は、`src/mcp/` 直下へ閉じ込めず、
既存モジュールまたは別モジュールとして分離する。

- 監査ログ基盤
  - 将来的に REST API 側や CLI 側でも利用し得るため、
    `src/mcp/` 内部専用実装として固定しない
  - イベント生成入口、集約・バッファ管理、writer、rotation、retention を独立責務として分ける
- Bearer トークン保存モデル
  - 既存 `src/database/` の拡張として扱う
- `run` コマンドの引数・設定解決
  - 既存 `src/cmd_args/` および `src/command/` の責務とする

### 3.4 既存モジュールへの変更点

MCP モジュール追加に伴い、少なくとも以下の既存モジュールへの追記を想定する。

- `src/main.rs`
  - `mod mcp;` の追加
- `src/http_server/mod.rs`
  - MCP endpoint の組み込み
  - MCP 有効化条件の反映
- `src/http_server/app_state.rs`
  - 必要に応じて MCP 設定または監査ログ依存を保持
- `src/cmd_args/run.rs`
  - MCP 有効化オプションの追加
- `src/command/run.rs`
  - MCP 有効化設定を HTTP サーバ起動へ受け渡す
- `src/database/`
  - Bearer トークン管理情報、ユーザ属性、MCP 用補助 API の拡張
  - prompt候補、resource候補、primitive名前索引、resource URI逆引き索引、readiness状態の追加

### 3.5 モジュール構成に関する設計判断

本章のモジュール構成設計では、以下を基本方針として採用する。

- MCP は `src/mcp/` を新設して独立モジュール化する
- transport / handler / service / auth / model / errors を分ける
- REST API 配下へ取り込まず、HTTP サーバから並列に組み込む
- 監査ログ基盤は MCP 専用閉域にせず、横断利用可能な位置付けで後続設計する

## 4. 既存 REST API 層との共通化方針

本章では、既存 REST API 層との共通化方針として、
「何を再利用し、何を MCP 専用に分けるか」を明確にする。

基本方針は、HTTP 依存の入出力処理は `rest_api` / `mcp` の各公開層に残し、
path ベース業務処理と認可補助は共通サービス層へ引き上げる、という分離とする。

### 4.1 そのまま再利用するもの

以下は既存実装を第一候補として再利用する。

- `AppState`
  - 共有状態の保持
  - DB、FTS、設定値へのアクセス
- `DatabaseManager`
  - 既存のページ読取・更新・削除・リネーム系 API
  - Bearer トークン照合と TTL 延長
- 既存の path 妥当性チェック規則
  - 現行 `rest_api::pages::validate_page_path` が持つ絶対パス前提と禁止文字規則
- FTS 検索基盤
  - 既存 `fts` モジュールによる検索実行

これらは低水準基盤または既存サーバ基盤であり、
MCP のために HTTP 経由で呼び直すのではなく、
同一プロセス内で直接利用する。

### 4.2 共通サービス層へ引き上げるもの

以下は REST API と MCP の双方で意味を共有し得るため、
必要に応じて共通サービス層へ引き上げる対象とする。

- path 正規化と path ベースの存在確認
- path からページ実体や内部 ID を解決する処理
- list / search における prefix 妥当性検証
- rename 時の移動元・移動先整合性検証
- create / update / append / rename の業務フロー制御
- path prefix 制約を含む認可補助

ただし、既存 REST API が page_id ベース公開である点を踏まえ、
REST API 側まで直ちに全面移行する前提は置かない。
まずは MCP のために共通サービス層を設け、
REST API 側で将来的に流用価値が高いものだけを段階的に共有化する。

### 4.3 MCP 専用に残すもの

以下は MCP 公開面に固有であり、REST API とは共通化しない。

- MCP transport / endpoint の処理
- MCP ツール定義
- MCP 入出力スキーマ
- MCP 応答形式への整形
- MCP 特有のエラー分類とクライアント向け表現
- `append` の公開インタフェース

特に `append` は初期実装で REST API へ露出しないため、
公開面としては MCP 専用に扱う。
ただし内部の保存フローや競合制御の一部は、共通サービス層または DB 層へ置く。

### 4.4 REST API 側へ残すもの

以下は既存 REST API の責務として維持し、
MCP とは共通化しない。

- HTTP クエリ・ヘッダ・Content-Type の詳細検証
- HTTP ステータスコードと JSON エラーレスポンスの組み立て
- ETag や Cache-Control など HTTP 応答メタ情報
- page_id ベースで公開される既存外部仕様
- 既存 UI / フロントエンドとの契約を前提としたレスポンス形式

### 4.5 共通化の進め方

共通化は以下の順で進める。

1. MCP 実装に必要な共通処理を `src/mcp/service.rs` 側へ定義する
2. その中で既存 `DatabaseManager` や `fts` を直接利用する
3. 既存 REST API から流用価値の高い純粋ロジックのみを切り出す
4. HTTP 依存処理は `rest_api` 側へ残す

この方針により、MCP の設計を優先しつつ、
既存 REST API の外部仕様や構造を不必要に崩さずに済む。

### 4.6 REST API との共通化に関する設計判断

本章の共通化設計では、以下を基本方針として採用する。

- `rest_api` と `mcp` は公開面を分離したまま維持する
- 再利用対象は HTTP 非依存の基盤処理に限定する
- path ベース業務処理と認可補助は MCP 側の共通サービス層へ集約する
- REST API 側の HTTP 入出力処理、page_id ベース公開仕様、レスポンス形式は維持する
- `append` は公開面としては MCP 専用、内部処理としては共通サービスまたは DB 層へ配置する

## 5. MCP promptsの垂直接続

MCP promptsはtools用`tools/call`とは別のrmcp標準routingへ接続する。

### 5.1 `prompts/list`

実装済みの接続順は次のとおり。

1. `ServerHandler::list_prompts()`
2. `LuwikiMcpServer::list_prompts_for_auth_at()`
3. `McpHandler::handle_list_prompts()`
4. `McpService::list_prompts()`
5. `DatabaseManager::list_prompt_candidates()`
6. `PromptListArgument`、`PromptListItem`、`ListPromptsServiceResult`
7. rmcp `Prompt`、`ListPromptsResult`

database層は候補と最新ページ状態を合流し、公開可能な候補を返す。
service層はread scope、cursor、名前順、50件上限を処理する。
handler層はprompts専用監査ログを生成し、server層がrmcp型へ最終変換する。

### 5.2 `prompts/get`

実装済みの接続順は次のとおり。

1. `ServerHandler::get_prompt()`
2. `LuwikiMcpServer::get_prompt_for_auth_at()`
3. `McpHandler::handle_get_prompt()`
4. `McpService::get_prompt()`
5. `DatabaseManager::get_prompt_source_by_name()`
6. `GetPromptServiceResult`
7. rmcp `GetPromptResult`、`PromptMessage`

database層はprimitive名前索引、ページ状態、latest revision、latest sourceを
単一redb read transaction内で解決する。prompt候補テーブルは名前解決と
本文取得に使用しない。

service層はlatest sourceのfront matter再検証、引数検証、placeholder展開、
message文字列生成を担当する。handler層は監査ログを生成し、server層は
単一User text messageをrmcp型へ変換する。

### 5.3 request contextと共有状態

標準handlerは`RequestContext<RoleServer>`から既存`AuthContext`と
入力元IP addressを取得する。`LuwikiMcpServer`は`AppState`のread lock内で
databaseへアクセスし、lock失敗を内部エラーへ変換する。

promptsはread scopeを要求するがページ用path prefix制約を適用しない。
同じ認証文脈を使うpathベースtoolsには従来のprefix制約を維持する。

### 5.4 型とroutingの分離

- database永続化型をMCP公開層へ直接露出しない
- service層とserver層の境界にはLuWiki内部モデルを使用する
- rmcp型への変換をserver公開層に限定する
- tools標準method、prompts標準method、LuWiki tool callのroutingを混在させない

## 6. MCP resourcesの垂直接続

MCP resourcesはtools用`tools/call`とは別のrmcp標準routingへ接続する。

### 6.1 `resources/list`

実装済みの接続順は次のとおり。

1. `ServerHandler::list_resources()`
2. `LuwikiMcpServer::list_resources_for_auth_at()`
3. `McpHandler::handle_list_resources()`
4. `McpService::list_resources()`
5. `DatabaseManager::list_resource_candidates()`
6. `ResourceListItem`、`ListResourcesServiceResult`
7. rmcp `Resource`、`ListResourcesResult`

database層はresource候補と最新ページ状態を合流し、公開可能なページ由来候補を返す。
service層は固定組み込みresourceとの合流、read scope、path prefix、cursor、
URI順、50件上限を処理する。handler層はresources専用監査ログを生成し、
server層がrmcp型へ最終変換する。

### 6.2 `resources/read`

実装済みの接続順は次のとおり。

1. `ServerHandler::read_resource()`
2. `LuwikiMcpServer::read_resource_for_auth_at()`
3. `McpHandler::handle_read_resource()`
4. `McpService::read_resource()`
5. 固定組み込みresource本文、または`DatabaseManager::get_resource_source_by_id()`
6. `ReadResourceServiceResult`
7. rmcp `ResourceContents`、`ReadResourceResult`

固定組み込みresourceはdatabase層のページ正本を経由せず、実装に埋め込んだ文書本文を返す。
ページ由来resourceはresource URI逆引き索引、ページ状態、latest revision、
latest sourceを単一redb read transaction内で解決する。
resource候補テーブルは一覧用であり、本文取得の正本として使用しない。

service層はURI検証、固定組み込みresourceまたはページ由来resourceへの分岐、
latest sourceのfront matter再検証、front matter除去後本文の取得、
MIME type既定値補完を担当する。handler層は監査ログを生成し、
server層はtext contentsとしてrmcp型へ変換する。

### 6.3 request contextと共有状態

標準handlerは`RequestContext<RoleServer>`から既存`AuthContext`と
入力元IP addressを取得する。`LuwikiMcpServer`は`AppState`のread lock内で
databaseへアクセスし、lock失敗を内部エラーへ変換する。

resourcesはread scopeを要求する。固定組み込みresourceにはページ用path prefix制約を
適用しない。ページ由来resourceにはページ用path prefix制約を適用する。
同じ認証文脈を使うpathベースtoolsには従来のprefix制約を維持する。

### 6.4 型とroutingの分離

- database永続化型をMCP公開層へ直接露出しない
- service層とserver層の境界にはLuWiki内部モデルを使用する
- rmcp型への変換をserver公開層に限定する
- tools標準method、prompts標準method、resources標準method、LuWiki tool callのroutingを混在させない
- resource URI、field、cursor、contents、protocol errorの外部契約は`MCP_RESOURCE_SPECS.md`を正本とする
