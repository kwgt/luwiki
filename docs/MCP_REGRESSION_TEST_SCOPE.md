# MCP関連 回帰確認範囲一覧

本書は、MCP実装に伴う既存機能への回帰確認範囲を整理した独立した回帰確認文書である。

位置付けとしては、`docs/REQUIREMENTS.md`と
現行MCP設計書群に対応する
実装・テスト実装向け文書として扱う。

MCP 追加は新規 endpoint 追加に留まらず、
Bearer 認証基盤、トークン管理情報、ユーザ属性、CLI 出力、`run` 起動経路、
監査ログ基盤、HTTP サーバ初期化に影響する。
そのため本書では、MCP そのもののテストではなく、
「既存機能が壊れていないこと」を確認すべき範囲を整理する。

## 1. 参照仕様

- `docs/REQUIREMENTS.md`
  - MCPサーバ機能
  - 認証・認可、監査ログ、アセット管理
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
  - `derived rebuild --target resources`
  - `derived rebuild --target all`
- `docs/REST_API_SPECS.md`
  - 認証
  - 認可
  - Bearer スコープ
- `docs/FRONT_MATTER_SPECS.md`
  - `mcp.primitive = resource`
  - resource front matter の値制約
- `docs/MCP_RESOURCE_SPECS.md`
  - resources 外部契約
  - 公開条件
  - capability
  - 通知非対応
  - 監査
  - 再構成導線
- `docs/MCP_EDIT_TEST_VIEWPOINTS.md`
  - `replace_section` 拡張後の edit operation 回帰観点
- `docs/MCP_ASSET_TOOL_TEST_VIEWPOINTS.md`
  - MCP アセット Tools導入時の固有テスト観点

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
5. MCP 側で `instance_id` を read / write 応答へ追加しても、REST API 側の既存レスポンス項目と互換性が変化しないこと

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

## 9. MCP 応答モデル変更の回帰範囲

### 9.1 `instance_id` 追加影響

1. `get_page`、`get_page_toc`、`create_page`、`update_page` の MCP 応答に `instance_id` を追加しても、既存 REST API や CLI の出力に影響しないこと
2. MCP クライアント向けには `instance_id` が必須応答として安定して返ること
3. `edit_page` 追加に伴う `instance_id` 応答の必須化が、他の MCP read / write ツール間で不整合を起こしていないこと

## 10. 監査ログ基盤の回帰範囲

### 10.1 既存ログ系

1. 既存 `AccessLogger` の出力が維持されること
2. 認証失敗が AccessLogger / tracing 側で従来どおり扱われること
3. 監査ログ追加によって通常ログ出力設定が壊れていないこと

### 10.2 MCP 非有効時

1. MCP 無効時に監査ログ基盤が単独起動しないこと
2. MCP 無効時に retention タスクが起動しないこと

## 11. 優先度の高い回帰セット

実装後の最小回帰セットとして、少なくとも以下を優先確認対象とする。

1. REST API の Basic / Bearer 認証と `write` 互換
2. 旧 Bearer トークン管理情報の読取互換
3. 旧 `UserInfo` の読取互換
4. `token create --scope write`、`token revoke`、`token purge`
5. `user add` / `user edit` / `user list` の既存運用
6. `luwiki run` の MCP 無効起動
7. MCP 無効時に `/mcp` 非公開で既存サーバ機能が維持されること
8. resources capability が readiness に応じて公開・非公開になること
9. `resources/list` / `resources/read` と既存 tools / prompts が同じ session 上で共存すること
10. 固定組み込み resource とページ由来 resource の path prefix 非適用境界
11. `derived rebuild --target resources` / `all` 後に resources 候補と URI 索引を復元できること
12. `resources.listChanged` を宣言せず通知しないこと
13. active SSE stream 生存中の session 維持と `last_access_at` 更新
14. SSE keep-alive、JSON-RPC message、SSE event payload、MCP handshake の互換維持
15. stream 終了、DELETE close、sweep、LRU eviction と Heartbeat tick の競合
16. `replace_section` 拡張後の既存 edit operation と既存 MCP tools / prompts / resources の代表回帰

## 12. 4.9.5 の整理結果

4.9.5 の完了条件に対する整理結果は以下の通り。

- REST API 既存機能の回帰範囲を、認証入口、既存 read/write API、ロック系まで含めて定義した
- Bearer 認証既存挙動の回帰範囲を、`write` 互換、旧トークン読取互換、path prefix 制約適用順まで含めて定義した
- ユーザ管理の回帰範囲を、旧 `UserInfo`、`NoBasicAuth` 非導入運用、`user list` 維持まで含めて定義した
- トークン管理の回帰範囲を、`token create --scope write`、既存 token 系コマンド、旧データ表示導出まで含めて定義した
- `run` 起動経路と HTTP サーバ統合の回帰範囲を、MCP 無効時の非公開維持と既存ルーティング維持まで含めて定義した
- MCP 応答モデル変更の回帰範囲を、`instance_id` 追加の影響と REST API 非影響まで含めて定義した

## 13. MCP prompts導入時の回帰範囲

### 13.1 capabilityとrouting

1. primitive名前索引readinessの真偽にかかわらずtools capabilityを維持すること
2. readiness version 1ではtoolsとprompts capabilityが共存すること
3. readinessなし・未知versionではpromptsだけを非公開とすること
4. 同じhandshake済みsession上で`tools/list`、`prompts/list`、
   `tools/call(get_page)`、`prompts/get`を交互に利用できること
5. tools標準method、prompts標準method、LuWiki tool callのroutingが
   混線しないこと
6. `get_page`のtool resultと`prompts/get`の標準`GetPromptResult`を
   混同しないこと

### 13.2 認証とsession

1. prompts追加後もAuthorization headerをrmcpへ転送しないこと
2. request bodyとAuthorization headerを通常ログへ出力しないこと
3. session期限切れPOSTがHTTP 401となること
4. session上限超過時に最古sessionをevictionすること
5. DELETEによるsession終了が成功すること
6. readinessなしでも既存tools、Bearer認証、session管理を利用できること

### 13.3 front matter・template・FTS

1. prompt追加後も既存front matter検証を維持すること
2. template候補の保存後同期を維持すること
3. template単独再構成、legacy fallback、front matter優先規則を維持すること
4. `derived rebuild --target all`のprompt側失敗時に、
   template候補を含む全対象の既存状態を維持すること
5. prompt再構成後に候補、名前索引、readinessを復元し、
   同じ`prompts/list`経路から利用できること
6. 既存FTSとfront matter検索の対象・結果を変更しないこと

### 13.4 prompts公開面

1. read scopeとReadOnly利用を維持すること
2. promptsへページ用path prefix制約を適用しないこと
3. pathベースtoolsには従来のprefix制約を維持すること
4. draft、soft delete、hard delete、orphan候補の公開制御を維持すること
5. case-sensitive順序、50件、cursor、空一覧を維持すること
6. 固定protocol errorと秘匿情報非公開を維持すること
7. `list_prompts`、`get_prompt`の監査ログを維持すること
8. `prompts.listChanged`を宣言せず、保存・削除・再構成から通知しないこと

### 13.5 REST API

1. prompt定義不正を既存front matter HTTP 400構造で返すこと
2. primitive名前重複時にページ正本を変更しないこと
3. primitive名前重複応答へprompt名、path、page IDを公開しないこと
4. 保存後候補同期失敗時にページ正本と名前索引を維持すること
5. 保存後候補同期失敗から共通再構成で復旧できること
6. REST APIの既存レスポンス形式とMCP標準応答を混同しないこと

## 14. MCP resources導入時の回帰範囲

### 14.1 capabilityとrouting

1. resource URI索引readinessの真偽にかかわらずtools capabilityを維持すること
2. resources readiness version 1ではtools、prompts、resources capabilityが共存すること
3. readinessなし・未知versionではresourcesだけを非公開とし、
   tools / promptsを巻き込まないこと
4. 同じhandshake済みsession上で`tools/list`、`tools/call(get_page)`、
   `prompts/list`、`prompts/get`、`resources/list`、`resources/read`を
   交互に利用できること
5. tools標準method、prompts標準method、resources標準method、
   LuWiki tool callのroutingが混線しないこと
6. `Resource` / `ResourceContents`とLuWiki tool result / prompt resultを
   混同しないこと

### 14.2 認証とsession

1. resources追加後もAuthorization headerをrmcpへ転送しないこと
2. request bodyとAuthorization headerを通常ログへ出力しないこと
3. resources標準methodでも既存session管理を維持すること
4. session期限切れPOSTがHTTP 401となること
5. session上限超過時に最古sessionをevictionすること
6. DELETEによるsession終了が成功すること
7. resources readinessなしでも既存tools、prompts、Bearer認証、
   session管理を利用できること

### 14.3 front matter・template・prompt・FTS

1. resource追加後も既存front matter検証を維持すること
2. template候補の保存後同期を維持すること
3. prompt候補の保存後同期を維持すること
4. template単独再構成、legacy fallback、front matter優先規則を維持すること
5. prompt単独再構成、名前索引、readinessを維持すること
6. resource再構成後に候補、URI逆引き索引、readinessを復元し、
   同じ`resources/list` / `resources/read`経路から利用できること
7. `derived rebuild --target all`のtemplates / prompts / resourcesの
   いずれかが失敗した場合に、全対象の既存状態を維持すること
8. promptとresourcesの索引責務が独立していること
9. 既存FTSとfront matter検索の対象・結果を変更しないこと

### 14.4 resources公開面

1. 固定組み込みresourceとページ由来resourceを同じ`resources/list`へ合流できること
2. 固定組み込みresourceにページ用path prefix制約を適用しないこと
3. ページ由来resourceにもページ用path prefix制約を適用しないこと
4. ページ由来resourceにread scopeとresource ACLを適用し、
   ACL非許可では一覧から除外し、取得ではnot foundとして秘匿すること
5. draft、soft delete、hard delete、orphan候補、
   URI索引不整合の公開制御を維持すること
6. URI昇順、50件、cursor、空一覧を維持すること
7. 固定protocol errorと秘匿情報非公開を維持すること
8. `list_resources`、`read_resource`の監査ログを維持すること
9. `resources.listChanged`を宣言せず、保存・削除・rename・import・
   rollback・amend・再構成から通知しないこと

### 14.5 REST APIと既存MCP tools/prompts

1. resource定義不正を既存front matter HTTP 400構造で返すこと
2. resource URI重複時にページ正本を変更しないこと
3. resource URI重複応答へpage ID、DB内部エラー、Bearer tokenを公開しないこと
4. 保存後候補同期失敗時にページ正本とURI逆引き索引を維持すること
5. 保存後候補同期失敗から共通再構成で復旧できること
6. pathベースtoolsには従来のprefix制約を維持すること
7. promptsには引き続きページ用path prefix制約を適用しないこと
8. REST APIの既存レスポンス形式とMCP標準resources応答を混同しないこと

## 15. session Heartbeat 変更後の回帰範囲

### 15.1 session維持

1. active SSE stream 生存中に session metadata の `last_access_at` が更新されること
2. active SSE stream 生存中、idle TTL 相当時間を超えても対象 session が維持されること
3. 既存 `sse_keep_alive(Duration::from_secs(30))` による SSE response-level keep-alive が維持されること
4. Heartbeat が JSON-RPC message、SSE event payload、MCP handshake を変更しないこと
5. Heartbeat が Bearer 認証境界や Authorization header の扱いを変更しないこと

### 15.2 session失効と非延命境界

1. active SSE stream が存在しない session は従来どおり idle TTL で失効すること
2. stream 終了、drop、error、close 後は Heartbeat が停止し、通常の idle TTL 判定へ戻ること
3. `has_session()` 呼び出しだけでは `last_access_at` が更新されないこと
4. 認証失敗、session ID 欠落、session not found request で session が延命されないこと
5. DELETE `/mcp` 後に Heartbeat が session を復活させないこと
6. session 期限切れ POST の HTTP 401 が維持されること

### 15.3 上限制御と競合

1. session 上限超過時の LRU eviction が引き続き機能すること
2. active stream 中でも、上限超過時は設計どおり eviction され得ること
3. sweep、LRU eviction、明示 close と Heartbeat tick が競合しても panic が発生しないこと
4. sweep、LRU eviction、明示 close と Heartbeat tick が競合しても session metadata が不正に残留しないこと

## 16. `replace_section` 拡張後の代表回帰範囲

### 16.1 edit operation

1. `replace_section` の `title` 未指定時に既存見出しが維持されること
2. `replace_section` の `title` 指定時に見出しレベルが維持され、見出し文字列だけが置換されること
3. `include_subsection` 未指定時が `false` と同じ挙動になること
4. `include_subsection: false` で既存サブセクションが保持されること
5. `include_subsection: true` で現行相当の子セクション込み置換になること
6. `content` に許可されない Markdown 見出し行を含めた場合に `invalid_input` になること
7. revision 不一致、instance_id 不一致が operation 内容検証より先に返ること

### 16.2 既存 edit operation

1. `insert_section` の before / after 挿入と見出し保持が壊れていないこと
2. `delete_section` が対象セクション範囲だけを削除すること
3. `replace_text` の first / all occurrence が壊れていないこと
4. section selector の文字列指定、`by=id`、`by=title` が引き続き動作すること
5. selector 不正、同名 title 複数、section not found の分類が既存どおりであること
6. `edit_page` が `update` scope で扱われ、`append` scope では許可されないこと

### 16.3 既存MCP公開面

1. `tools/list` で既存 tools と `edit_page` schema が取得できること
2. `edit_page` schema に `title` / `include_subsection` と説明文が反映されていること
3. `create_page`、`update_page`、`append_page`、`rename_page`、
   `get_page`、`get_page_toc`、`get_page_section`、`list_pages`、
   `search_pages` の代表呼び出しが壊れていないこと
4. write 系 tool の Bearer scope 判定が壊れていないこと
5. `prompts/list`、`prompts/get`、`resources/list`、`resources/read` が
   同じ handshake 済み session 上で tools と共存すること
6. `initialize.instructions` が Bearer 認証注意を維持し、
   `replace_section` 注意文を含むこと

## 17. MCP アセット Tools導入時の回帰範囲

### 17.1 capabilityとrouting

1. tools capabilityが既存toolとアセット6 toolを同じ `tools/list` で公開できること
2. `list_page_assets`、`resolve_page_asset`、`get_page_asset_metadata`、
   `get_page_asset_data`、`upload_page_asset`、`delete_page_asset` のschemaが
   既存tool schemaを壊さないこと
3. 同じhandshake済みsession上で既存tools、prompts、resources、
   アセット6 toolを交互に利用できること
4. tools標準method、prompts標準method、resources標準method、
   LuWiki tool callのroutingが混線しないこと
5. `get_page_asset_data` のtool resultとresources標準methodの
   `ResourceContents`を混同しないこと

### 17.2 認証・認可

1. アセット参照系4 tool追加後も既存pathベースtoolsのread scope判定が
   維持されること
2. `upload_page_asset` のupdate scope判定追加後も `update_page` と
   `edit_page` のscope判定が維持されること
3. `delete_page_asset` のdelete scope判定追加後も、既存REST APIの
   Bearer認可が意図せず変化しないこと
4. ReadOnly拒否追加後も、ReadOnlyユーザによるMCP read系、
   prompts、resourcesの利用が維持されること
5. アセットtoolのpath prefix判定追加後も、prompts/resourcesの
   path prefix非適用境界が維持されること

### 17.3 RESTアセットAPI

1. RESTアセット一覧、メタ情報取得、本体取得、upload、deleteの既存endpointが
   MCP アセット Tools追加後も従来どおり動作すること
2. REST APIのredirect、ETag、条件付きGET、HTTP status codeが
   MCP `get_page_asset_data` の仕様に引きずられて変化しないこと
3. REST uploadのmultipart / body処理とMCP `data_base64` 入力処理が
   混同されないこと
4. REST APIの設定値
   （未指定時10MiB、設定可能な最大値100MiB）と、
   MCP固定10MiB / 13,981,016 bytes制限が
   それぞれ独立して評価されること
5. REST側のlock token headerや既存ロック応答が、
   MCP upload / deleteのlock token非公開方針で壊れないこと

### 17.4 DB・保存済みアセット

1. 既存 `DatabaseManager` asset APIをMCP service層から直接利用しても、
   REST handler経由の既存利用が維持されること
2. 削除済みasset、ゾンビasset、所属ページ不一致assetの扱いが、
   既存DB整合性やREST側の公開制御を壊さないこと
3. 同名未削除asset重複の拒否が既存upload挙動と矛盾しないこと
4. 削除済み同名assetが存在する場合の新規upload許可可否が、
   DB APIの既存挙動に従うこと

### 17.5 監査ログと秘匿

1. アセット6 toolの監査operation追加後も、既存tools、prompts、
   resourcesの監査operationが維持されること
2. `data_base64`、アセット本体bytes、内部保存パス、DB内部詳細を
   監査ログ、tracing、error summaryへ出力しないこと
3. アセット6 toolがappend集約状態へ混入しないこと
4. 監査ログbest-effort方針がアセットtool追加後も維持されること

### 17.6 優先度の高い追加回帰セット

1. `tools/list` にアセット6 toolが追加され、既存tool schemaが維持されること
2. read / update / delete / write scopeとReadOnlyの代表ケースを確認すること
3. path prefix範囲内外の代表ケースを確認すること
4. RESTアセットAPIの代表read / upload / deleteが維持されること
5. `get_page_asset_data` の10MiB超過と `upload_page_asset` の
   decode前13,981,016 bytes超過が別入口で確認できること
6. アセット6 toolの監査ログと秘匿情報非記録を確認すること
