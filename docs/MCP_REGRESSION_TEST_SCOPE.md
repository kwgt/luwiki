# MCP関連 回帰確認範囲一覧

本書は、MCP実装に伴う既存機能への回帰確認範囲を整理した独立した回帰確認文書である。

位置付けとしては、`docs/MCP_IMPLEMENTATION_DESIGN_TASKS.md` の 4.9.5 に対応する成果物であり、
MCP 設計書群から参照される実装・テスト実装向け文書として扱う。

MCP 追加は新規 endpoint 追加に留まらず、
Bearer 認証基盤、トークン管理情報、ユーザ属性、CLI 出力、`run` 起動経路、
監査ログ基盤、HTTP サーバ初期化に影響する。
そのため本書では、MCP そのもののテストではなく、
「既存機能が壊れていないこと」を確認すべき範囲を整理する。

## 1. 参照仕様

- `docs/MCP_IMPLEMENTATION_DESIGN_TASKS.md`
  - 4.8 CLI / 設定 / 起動経路の設計
  - 4.9 テスト設計
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - 2.3.39 から 2.3.43
  - 2.4 HTTP サーバへの反映
  - 2.5 起動失敗条件
- `docs/BEARER_AUTH_DESIGN.md`
  - 認証フロー
  - スコープ判定
  - ユーザ属性
- `docs/CLI_SPECS.md`
  - `run`
  - `token` 系
  - `user` 系
- `docs/REST_API_SPECS.md`
  - 認証
  - 認可
  - Bearer スコープ

## 2. 回帰確認の使い方

- 本書は既存機能の回帰確認範囲を整理するための文書であり、新規機能の詳細テストケース一覧ではない
- 変更箇所と同一モジュールにある既存機能、共通基盤へ引き上げた処理、永続化モデル拡張で影響を受ける既存データ経路を優先して確認する
- すべてを同一レイヤで再テストする前提にはせず、単体、結合、CLI、HTTP 統合、既存データ読取確認へ分配する

## 3. REST API 既存機能の回帰範囲

### 3.1 認証入口

1. `/api` 配下で Basic / Bearer の両方式が従来どおり受理されること
2. Bearer 認証失敗が 401、認可失敗が 403 となる責務分担が維持されること
3. Bearer 認証成功時の TTL 延長が従来どおり動作すること
4. `NoBasicAuth` ユーザに対する Basic 認証拒否が REST API で維持されること

### 3.2 既存 read/write API

1. 既存 read 系 API が Bearer `read` または `write` で従来どおり利用できること
2. 既存 write 系 API が Bearer `write` で従来どおり利用できること
3. Bearer 分解スコープ導入後も、REST API 側の `write` 互換動作が崩れていないこと
4. 既存 page_id ベース API のレスポンス形式が MCP 追加で変化していないこと

### 3.3 ロック・更新系

1. 既存ロック API の Bearer / `X-Lock-Authentication` 判定順序が維持されること
2. 既存更新 API の amend 条件が MCP `append` 追加で壊れていないこと
3. 既存 423 Locked / 403 Forbidden の責務分担が維持されること

## 4. Bearer 認証既存挙動の回帰範囲

### 4.1 スコープ判定

1. 旧 `read` / `write` トークンの認可が従来どおり成立すること
2. `write` が `read` / `create` / `update` / `append` / `delete` を包含する判定が維持されること
3. 分解済みスコープ導入後も、保存時に `write` 展開を行わない方針が維持されること
4. path prefix 制約判定が Bearer 認証成功後に適用される流れが維持されること

### 4.2 既存トークン管理情報

1. 旧 Bearer トークン管理情報をマイグレーションなしで読み取れること
2. `path_prefixes` 欠落旧データを全領域アクセス可として扱えること
3. 旧 `write` 保存値が認証時・CLI 表示時ともに正しく扱われること
4. 既存トークンが再発行なしで利用可能であること

## 5. ユーザ管理の回帰範囲

### 5.1 既存 `user add` / `user edit` / `user list`

1. 属性未指定の `user add` が従来どおりパスワード入力ありで動作すること
2. `user edit --display-name` が従来どおり動作すること
3. `user edit --password` が通常ユーザに対して従来どおり動作すること
4. `user list` が一覧責務を維持し、属性列追加で崩れていないこと

### 5.2 既存ユーザデータ

1. `attributes` を持たない旧 `UserInfo` を空集合として読めること
2. 旧ユーザが `NoBasicAuth` 未設定ユーザとして継続利用できること
3. `NoBasicAuth` を導入しない既存運用が変わらないこと

## 6. トークン管理 CLI の回帰範囲

### 6.1 既存コマンド

1. `token create --scope write` が従来どおり受理されること
2. `token revoke` が従来どおり動作すること
3. `token purge` が従来どおり動作すること
4. `token add_path` / `token remove_path` が path prefix 導入後も整合して動作すること

### 6.2 既存一覧運用

1. `token list` の主用途である人手による一覧確認が維持されること
2. `token list` の列構成変更により、少なくとも人手運用で誤認しないこと
3. 旧データの `write` / `path_prefixes` 欠落が一覧表示で正しく導出されること

## 7. `run` 起動経路の回帰範囲

### 7.1 MCP 無効時

1. `luwiki run` で既定どおり MCP 無効起動となること
2. MCP 無効時に `/mcp` endpoint が登録されないこと
3. MCP 無効時に既存 REST API / UI ルートだけが公開されること
4. MCP 無効時に MCP 固有依存の初期化失敗を評価しないこと

### 7.2 既存 `run` 設定との整合

1. `run.use_mcp` 未設定時の既定値が `false` であること
2. `--mcp` が config より優先されること
3. `--save-config` が既存 `run` 設定保存挙動を壊していないこと
4. 既存 `run` の TLS や通常ログ設定が MCP 追加で崩れていないこと

## 8. HTTP サーバ統合の回帰範囲

### 8.1 既存ルーティング

1. MCP endpoint 追加後も既存 `/api` ルーティングが変わらないこと
2. 既存 UI / 静的配信ルーティングが変わらないこと
3. MCP endpoint が `rest_api::create_api_scope(...)` 配下へ誤って混入していないこと

### 8.2 起動失敗条件

1. MCP 有効時のみ MCP 固有依存初期化失敗が起動失敗になること
2. MCP 無効時に監査ログ出力先の問題などで起動失敗しないこと

## 9. 監査ログ基盤の回帰範囲

### 9.1 既存ログ系

1. 既存 `AccessLogger` の出力が維持されること
2. 認証失敗が AccessLogger / tracing 側で従来どおり扱われること
3. 監査ログ追加によって通常ログ出力設定が壊れていないこと

### 9.2 MCP 非有効時

1. MCP 無効時に監査ログ基盤が単独起動しないこと
2. MCP 無効時に retention タスクが起動しないこと

## 10. 優先度の高い回帰セット

実装後の最小回帰セットとして、少なくとも以下を優先確認対象とする。

1. REST API の Basic / Bearer 認証と `write` 互換
2. 旧 Bearer トークン管理情報の読取互換
3. 旧 `UserInfo` の読取互換
4. `token create --scope write`、`token revoke`、`token purge`
5. `user add` / `user edit` / `user list` の既存運用
6. `luwiki run` の MCP 無効起動
7. MCP 無効時に `/mcp` 非公開で既存サーバ機能が維持されること

## 11. 4.9.5 の整理結果

4.9.5 の完了条件に対する整理結果は以下の通り。

- REST API 既存機能の回帰範囲を、認証入口、既存 read/write API、ロック系まで含めて定義した
- Bearer 認証既存挙動の回帰範囲を、`write` 互換、旧トークン読取互換、path prefix 制約適用順まで含めて定義した
- ユーザ管理の回帰範囲を、旧 `UserInfo`、`NoBasicAuth` 非導入運用、`user list` 維持まで含めて定義した
- トークン管理の回帰範囲を、`token create --scope write`、既存 token 系コマンド、旧データ表示導出まで含めて定義した
- `run` 起動経路と HTTP サーバ統合の回帰範囲を、MCP 無効時の非公開維持と既存ルーティング維持まで含めて定義した
