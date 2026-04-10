# MCP実装タスクリスト

本書は、MCP サーバ機能の内部設計を踏まえて、
実装作業を複数セッションへ分割して進捗管理できる粒度へ
分解したタスクリストである。

対象は初期版 MCP サーバ機能と、それに付随する監査ログ、認証・認可、
CLI / 設定 / 起動経路、トークン / ユーザ管理拡張、およびテストである。

## 1. 参照文書

- `docs/REQUIREMENTS.md`
- `docs/CLI_SPECS.md`
- `docs/REST_API_SPECS.md`
- `docs/BEARER_AUTH_DESIGN.md`
- `docs/MCP_INTERNAL_DESIGN.md`
- `docs/MCP_ARCHITECTURE_DESIGN.md`
- `docs/MCP_TOOL_SPECS.md`
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
- `docs/MCP_AUDIT_LOG_DESIGN.md`
- `docs/MCP_AUTHORIZATION_TEST_VIEWPOINTS.md`
- `docs/MCP_APPEND_TEST_VIEWPOINTS.md`
- `docs/MCP_AUDIT_LOG_TEST_VIEWPOINTS.md`
- `docs/MCP_CLI_TEST_VIEWPOINTS.md`
- `docs/MCP_REGRESSION_TEST_SCOPE.md`

## 2. 実装方針

- MCP は `run` で明示有効化された場合のみ公開する
- MCP は REST API の内部呼び出しではなく、`src/mcp/` の専用公開層と共通サービス層で実装する
- path ベース要求から page_id ベース内部処理への橋渡しは `src/mcp/service.rs` へ集約する
- Bearer 認証コアは既存設計と整合させ、MCP 固有の入口だけを薄く追加する
- 監査ログ基盤は `src/audit/` の独立モジュールとして実装し、MCP 公開層と共通サービス層から接続する
- 初期実装の対象外である restore、削除済みページ参照、アセット操作、ロック操作は公開しない

## 3. マイルストーン

- M1: モジュール骨格と保存モデル拡張の整備
- M2: MCP 認証・認可と path ベースサービス層の整備
- M3: MCP transport / ツール公開面の整備
- M4: 監査ログ基盤と `append` 集約の整備
- M5: CLI / 設定 / 起動経路と token / user 拡張の整備
- M6: テストと回帰確認

## 4. 実装タスク

### 4.1 MCP / audit モジュール骨格

- [x] 4.1.1 `src/mcp/` を新設し `mod.rs` / `transport.rs` / `tools.rs` / `handler.rs` / `service.rs` / `auth.rs` / `model.rs` / `errors.rs` の骨格を追加する  
  完了条件: HTTP サーバ統合層から MCP 公開入口を参照できる
- [x] 4.1.2 `src/audit/` を新設し `mod.rs` / `model.rs` / `sink.rs` / `buffer.rs` / `writer.rs` / `rotation.rs` / `retention.rs` の骨格を追加する  
  完了条件: 監査イベント投入と JSONL 書込の責務分割をコード上に表現できる
- [x] 4.1.3 `src/lib.rs` / `src/main.rs` / 既存 `mod.rs` 群へ MCP / audit モジュールの配線を追加する  
  完了条件: 新規モジュールがビルド対象へ組み込まれる

### 4.2 保存モデル・DB API 拡張

- [x] 4.2.1 `src/database/types.rs` の Bearer 管理情報を分解スコープ、path prefix 制約、`token_id` 表示要件へ追従させる  
  完了条件: `read` / `write` / `create` / `update` / `append` / `delete`、複数 path prefix、互換読込を型で扱える
- [x] 4.2.2 `src/database/types.rs` の `UserInfo` を属性集合対応へ拡張する  
  完了条件: `NoBasicAuth` を含む将来拡張可能な属性集合を保持できる
- [x] 4.2.3 `src/database/manager/` 配下に path 解決補助 API を追加する  
  完了条件: current path からページ実体を解決し、list / search / rename / append に必要な状態を取得できる
- [x] 4.2.4 `src/database/manager/` 配下に `append` 保存補助 API と競合確認 API を追加する  
  完了条件: amend 判定、最新 revision 再確認、必要なロック待機判定を支援できる
- [x] 4.2.5 旧 Bearer 管理情報および旧 `UserInfo` の互換デシリアライズを実装する  
  完了条件: 既存データを破壊せずに新モデルへ移行できる

### 4.3 MCP 認証・認可

- [x] 4.3.1 既存 Bearer 認証コアを MCP 入口から利用できる共通形へ整理する  
  完了条件: REST API と MCP の両方から、トークン照合・TTL 延長・認証文脈生成を共有利用できる
- [x] 4.3.2 `src/mcp/auth.rs` に MCP 用 Bearer 認証入口を実装する  
  完了条件: Bearer 抽出、Basic 非受理、認証成功時の共通認証文脈受け渡しができる
- [x] 4.3.3 `src/mcp/service.rs` に required scope 判定と path prefix 制約判定を実装する  
  完了条件: read / create / update / append / delete / write 互換を操作単位で判定できる
- [x] 4.3.4 rename、list、search の複合認可判定を実装する  
  完了条件: rename の移動元 / 移動先、list / search の要求 prefix を個別に判定できる

### 4.4 path ベースサービス層

- [x] 4.4.1 `validate_and_normalize_path`、`resolve_page_by_path`、`resolve_prefix_request` を実装する  
  完了条件: path 妥当性検証、正規化、ページ解決、prefix 判定の共通入口が揃う
- [x] 4.4.2 read / get_toc / list / search / get_section のサービス API を実装する  
  完了条件: path ベース入力から既存 DB API / FTS を呼び出し、`get_page` / `get_page_toc` / `list_pages` / `search_pages` / `get_page_section` に対応する結果モデルを返せる
- [x] 4.4.3 create / update / rename のサービス API を実装する  
  完了条件: 認可、path 解決、保存、結果整形を一貫して処理できる
- [x] 4.4.4 `append` のサービス API を実装する  
  完了条件: 末尾追記、amend 判定、競合待機、timeout、監査ログ向け結果生成を処理できる
- [x] 4.4.5 対象外機能の拒否方針をコードへ反映する  
  完了条件: restore、削除済みページ参照、アセット操作、ロック操作を `unsupported` / `not_found` の設計どおり扱える

### 4.5 MCP 公開面・transport

- [x] 4.5.1 `src/mcp/tools.rs` に初期版ツール定義を実装する  
  完了条件: `get_page`、`get_page_toc`、`list_pages`、`search_pages`、`create_page`、`update_page`、`edit_page`、`append_page`、`rename_page`、`get_page_section` を公開できる
- [x] 4.5.2 `src/mcp/model.rs` に共通入出力モデルとツール別モデルを実装する  
  完了条件: path ベースの公開データモデル、`instance_id` を含む read / write 応答モデル、`edit_page.operation`、`get_page_toc.sections`、`get_page_section.section` selector、`list_pages` の cursor ページング、`search_pages` の top-N 取得モデルがコード化される
- [x] 4.5.3 `src/mcp/errors.rs` と `src/mcp/handler.rs` に内部エラーから公開エラーへの写像を実装する  
  完了条件: transport エラー、認証失敗、認可失敗、入力不正、競合、`not_latest_revision`、`instance_id_not_match`、未対応を設計どおり返せる
- [x] 4.5.4 `src/mcp/transport.rs` に Streamable HTTP 前提の Actix adapter を実装する  
  完了条件: `/mcp` で `POST` を受理し、`GET` / `DELETE` を `405` として扱える
- [x] 4.5.5 `src/http_server/mod.rs` と `src/http_server/app_state.rs` に MCP endpoint 組み込みを追加する  
  完了条件: MCP 有効時のみ `/mcp` が登録され、必要依存が注入される

### 4.6 監査ログ基盤

- [x] 4.6.1 `src/audit/model.rs` に監査レコード、内部結果分類、`append` 集約キーを実装する  
  完了条件: `operation`、`user_id`、`token_id`、`address`、`target_path`、`result`、`timestamp`、`summary`、`revision` を保持できる
- [x] 4.6.2 `src/audit/sink.rs` と `src/audit/buffer.rs` に監査イベント投入と `append` 集約を実装する  
  完了条件: 通常イベント即時書込と、`append` の 1 分集約・終了時 flush を切り替えられる
- [x] 4.6.3 `src/audit/writer.rs` と `src/audit/rotation.rs` に JSONL 書込と固定サイズローテーションを実装する  
  完了条件: `audit.current.jsonl` 追記、閾値到達時のローテーション、flush ができる
- [x] 4.6.4 `src/audit/retention.rs` に保持期間超過ファイル削除を実装する  
  完了条件: 起動時または保守契機で、アクティブファイルを除外して削除できる
- [x] 4.6.5 MCP 公開層と共通サービス層から監査ログへ接続する  
  完了条件: 成功系、認可失敗、`append` 集約、認証失敗対象外の境界が設計どおり記録される

### 4.7 CLI / 設定 / 起動経路

- [x] 4.7.1 `src/cmd_args/run.rs` と `src/command/run.rs` に MCP / audit 関連オプションと設定解決を実装する  
  完了条件: `--mcp`、監査ログ出力先、保持期間、ローテーション閾値を解釈して内部設定へ変換できる
- [x] 4.7.2 `src/cmd_args/token.rs` と token 系 command に分解スコープ / path prefix 追従を実装する  
  完了条件: `token create`、`token list`、`token info` が新スコープ体系と path 制約表示に対応する
- [x] 4.7.3 `src/cmd_args/user.rs` と user 系 command に属性管理を実装する  
  完了条件: `user add`、`user edit`、`user info` で `NoBasicAuth` を扱える
- [x] 4.7.4 CLI 表示整形を実装する  
  完了条件: `token list` の `SCOPE` / `PATH` / `STATUS`、`token info` の完全表示、`user info` の属性表示が仕様どおりになる

### 4.8 テスト

- [x] 4.8.1 Bearer 管理情報 / `UserInfo` 互換読込の単体テストを追加する  
  完了条件: 旧データを新モデルで読める
- [x] 4.8.2 MCP 認証・認可の単体 / 統合テストを追加する  
  完了条件: Bearer 成功、スコープ不足、path prefix 制約違反、`write` 互換、`NoBasicAuth` 境界を確認できる
- [x] 4.8.3 path ベースサービス層の単体 / 統合テストを追加する  
  完了条件: read / get_toc / list / search / get_section / create / update / append / rename の各経路、および `list_pages` の cursor 境界と `search_pages` の top-N 取得を確認できる
- [x] 4.8.4 `append` 競合制御の統合テストを追加する  
  完了条件: 同一ユーザ amend、別ユーザ新規 revision、待機、timeout、競合失敗を確認できる
- [x] 4.8.5 監査ログ基盤の単体 / 統合テストを追加する  
  完了条件: 成功、認可失敗、`append` 集約、rotation、retention、終了時 flush を確認できる
- [x] 4.8.6 CLI 統合テストを追加する  
  完了条件: `run --mcp`、`token create/list/info`、`user add/edit/info` の正常系 / 異常系を確認できる
- [x] 4.8.7 回帰確認を追加する  
  完了条件: REST API 既存機能、MCP 無効時の `/mcp` 非公開、既存 token / user CLI、旧 Bearer / UserInfo 読取互換を確認できる

## 5. 優先度順の着手推奨

1. MCP / audit モジュール骨格
2. 保存モデル・DB API 拡張
3. MCP 認証・認可
4. path ベースサービス層
5. MCP 公開面・transport
6. 監査ログ基盤
7. CLI / 設定 / 起動経路
8. テスト
