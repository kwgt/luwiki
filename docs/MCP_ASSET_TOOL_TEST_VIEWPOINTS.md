# MCP アセット Tools テスト観点一覧

本書は、MCP アセット Tools 6件のテスト観点を整理した独立したテスト観点書である。

MCP アセット Toolsは、ページに紐付く通常アセットをMCP経由で扱うための
pathベースtoolである。初期版では、`path + file_name` を入力識別子の基本とし、
`asset_id` は出力上の識別情報としてのみ扱う。

## 1. 参照仕様

- `docs/REQUIREMENTS.md`
  - 12. MCPサーバ機能
  - 13. 監査ログ
  - 15. アセット管理
- `docs/MCP_TOOL_SPECS.md`
  - 2.11 `list_page_assets`
  - 2.12 `resolve_page_asset`
  - 2.13 `get_page_asset_metadata`
  - 2.14 `get_page_asset_data`
  - 2.15 `upload_page_asset`
  - 2.16 `delete_page_asset`
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 3.4.11 アセット tool
  - 3.5.11 アセット tool
  - 4. MCP エラー応答と内部エラー分類の対応
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - 2.3.8 アセット共通 helper
  - 2.3.9 `list_page_assets`
  - 2.3.10 `resolve_page_asset`
  - 2.3.11 `get_page_asset_metadata`
  - 2.3.12 `get_page_asset_data`
  - 2.3.13 `upload_page_asset`
  - 2.3.14 `delete_page_asset`
- `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 10. MCP アセット Toolsの監査設計

## 2. 観点の使い方

- 本書はアセット6 tool固有の正常系、失敗系、payload制約、DB接続、
  監査ログ接続を実装・テストへ配分するための別紙である
- 認証・認可の詳細組合せは `MCP_AUTHORIZATION_TEST_VIEWPOINTS.md`、
  監査ログ基盤の詳細は `MCP_AUDIT_LOG_TEST_VIEWPOINTS.md`、
  既存機能回帰は `MCP_REGRESSION_TEST_SCOPE.md` と接続して扱う
- REST APIのredirect、ETag、条件付きGET、HTTP status codeを
  MCP tool仕様へ持ち込まないことを確認する

## 3. 共通入力・出力の観点

### 3.1 入力識別子

1. アセット指定が `path + file_name` で行われること
2. `asset_id` 単独入力を初期版で受け付けないこと
3. `path` が正規化済み絶対pathとして扱われること
4. `file_name` の空文字、`/`、バックスラッシュを拒否すること
5. `file_name` を相対パスまたは絶対パスとして解釈しないこと

### 3.2 metadata

1. metadataに `asset_id`、`path`、`file_name`、`mime_type`、
   `size`、`timestamp`、`username` が含まれること
2. `instance_id` が存在する場合だけmetadataへ含まれること
3. `instance_id` 欠損時にJSON項目を省略すること
4. 内部保存パス、page ID、DB内部詳細がmetadataへ含まれないこと

### 3.3 `data_base64`

1. `data_base64` は `get_page_asset_data` の出力と
   `upload_page_asset` の入力に限定されること
2. `list_page_assets`、`resolve_page_asset`、`get_page_asset_metadata`、
   `delete_page_asset` が `data_base64` を返さないこと
3. `data_base64` とアセット本体bytesが監査ログ、tracing、
   error summaryへ流れないこと

## 4. 参照系toolの観点

### 4.1 `list_page_assets`

1. 指定ページに紐付く通常アセットmetadata一覧を返すこと
2. 削除済み、ゾンビ、所属ページ不一致のアセットを返さないこと
3. アセットが0件の場合に空一覧を返せること
4. page不存在を `not_found` として扱うこと
5. DB失敗を `internal_error` として扱うこと

### 4.2 `resolve_page_asset`

1. `path + file_name` から対象アセットを解決できること
2. 成功時に `asset_id`、`path`、`file_name` の最小識別情報を返すこと
3. 詳細metadataと本体を返さないこと
4. 対象asset不存在、削除済み、外部到達不能を `not_found` として扱うこと
5. 所属ページ不一致を外部へ詳細公開しないこと

### 4.3 `get_page_asset_metadata`

1. `path + file_name` から詳細metadataを返すこと
2. `data_base64` を返さないこと
3. `instance_id` の有無に応じてJSON表現が変わること
4. 削除済み、ゾンビ、所属ページ不一致assetを通常解決対象にしないこと

### 4.4 `get_page_asset_data`

1. metadataと `data_base64` を返すこと
2. raw bytesが10MiB以下の保存済みアセットを返せること
3. raw bytesが10MiBを超える保存済みアセットを `conflict` として扱うこと
4. 事前検出できるresult / transport payload上限超過を `conflict` として扱うこと
5. transport送信時またはserialize後に初めて失敗した場合は
   protocol `internal_error` として扱うこと

## 5. 更新系toolの観点

### 5.1 `upload_page_asset`

1. `path`、`file_name`、`mime_type`、`data_base64` を入力として新規uploadすること
2. ローカルファイルパス入力を受け付けないこと
3. 既存アセットの上書きuploadを受け付けないこと
4. `mime_type` の空文字を拒否すること
5. `data_base64` の空文字を拒否すること
6. base64 decode失敗を `invalid_input` として扱うこと
7. decode前byte長が13,981,016 bytesを超える場合に `invalid_input` とすること
8. decode後raw bytesが10MiBを超える場合に `invalid_input` とすること
9. 同名未削除アセットが存在する場合に `conflict` とすること
10. 削除済み同名アセットが存在する場合はDB APIの既存挙動に従うこと
11. ロック中ページではlock token入力を受け付けず `conflict` とすること
12. 成功時にmetadataとsummaryを返し、アセット本体を返さないこと

### 5.2 `delete_page_asset`

1. `path + file_name` で対象アセットを削除すること
2. `asset_id` 単独入力を受け付けないこと
3. ロック中ページではlock token入力を受け付けず `conflict` とすること
4. 対象asset不存在、削除済み、外部到達不能を `not_found` として扱うこと
5. 成功時に `asset_id`、`path`、`file_name`、summaryを返すこと
6. 成功時にmetadata詳細とアセット本体を返さないこと

## 6. 認可とロックの観点

1. 参照系4 toolがread scopeを要求すること
2. `upload_page_asset` がupdate scopeを要求すること
3. `delete_page_asset` がdelete scopeを要求すること
4. legacy write scopeが6 toolのrequired scopeを満たすこと
5. path prefix制約を正規化済み入力 `path` に対して判定すること
6. `ReadOnly`ユーザによるupload / deleteを拒否すること
7. 参照系4 toolがページロックの影響を受けないこと
8. upload / deleteがロック中ページを `conflict` として扱うこと

## 7. DB接続と状態秘匿の観点

1. MCP service層がREST API内部HTTP呼び出しを行わないこと
2. 既存 `DatabaseManager` asset APIを直接利用すること
3. DB層へrmcp公開型やMCP固有error codeを持ち込まないこと
4. 削除済みassetを通常解決対象から除外すること
5. ゾンビassetを通常解決対象から除外すること
6. 所属ページ不一致assetを通常解決対象から除外すること
7. 除外したassetの一覧やfile_nameを外部応答や監査summaryへ含めないこと

## 8. 実施レイヤ分割の目安

- 単体テスト向き
  - file_name、mime_type、data_base64、size制約、error写像
- service層結合テスト向き
  - path解決、scope、ReadOnly、path prefix、DB API接続、状態秘匿
- serverテスト向き
  - tool schema、入力モデル、出力モデル、protocol error変換
- MCP統合テスト向き
  - handshake済みsession上の6 tool代表呼び出し、監査ログ接続
- REST回帰テスト向き
  - 既存RESTアセットAPIのredirect、ETag、条件付きGET、upload、delete維持
