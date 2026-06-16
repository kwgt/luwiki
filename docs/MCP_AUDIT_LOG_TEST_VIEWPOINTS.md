# MCP監査ログ テスト観点一覧

本書は、MCPサーバ機能における監査ログのテスト観点を整理した独立したテスト観点書である。

位置付けとしては、`docs/MCP_IMPLEMENTATION_DESIGN_TASKS.md` の 4.9.3 に対応する成果物であり、
MCP 設計書群から参照される実装・テスト実装向け文書として扱う。

監査ログは MCP 起点で設計されたが、実装上は `src/audit/` の横断基盤として扱われる。
そのため本書では、MCP handler / service からのイベント投入だけでなく、
集約、writer、rotation、retention、終了時 flush を含む基盤全体の観点を整理する。

## 1. 参照仕様

- `docs/REQUIREMENTS.md`
  - 13. 監査ログ
- `docs/MCP_INTERNAL_DESIGN.md`
  - 5.1 認証・認可
- `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 2. 責務配置
  - 3. 監査ログレコードモデル
  - 4. `append` 集約ロジックの状態管理
  - 5. JSONL 保存形式とファイル分割ルール
  - 6. 保持期間超過ログの自動削除
  - 7. `tracing` と既存アクセスログとの役割分担
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 4.5 認可失敗の対応
  - 4.6 競合系の対応
- `docs/MCP_RESOURCE_SPECS.md`
  - MCP resourcesの監査対象操作、summary、秘匿情報

## 2. 観点の使い方

- 本書は監査ログ基盤の単体テスト、サービス層結合テスト、ファイルI/O 統合テスト、起動時保守テストへ観点を配分するための別紙である
- 成功系と失敗系を同じ粒度で扱わず、「どの操作をどう記録するか」と「どう保存・保守するか」を分けて具体化する
- 認証失敗は監査ログ対象外であるため、本書では「記録されないこと」も重要な観点として扱う
- `append` の保存挙動そのものは 4.9.2 側で扱い、本書では監査ログとしての集約・flush・確定記録を中心に扱う

## 3. 監査レコードモデルの観点

### 3.1 必須項目と欠損条件

1. `operation`、`user_id`、`result`、`timestamp` が常に記録されること
2. Bearer 認証成功後の正常系・認可失敗系で `token_id` が記録されること
3. 接続情報が取得できた場合に `address` が記録されること
4. path が特定できる操作で `target_path` が記録されること
5. revision を持つ write 系成功操作で `revision` が記録されること
6. `summary` が不要な単純成功系では `null` を許容すること

### 3.2 操作種別ごとの記録粒度

1. `get`、`get_section`、`list`、`list_prompts`、`get_prompt`、`list_resources`、`read_resource`、`search`、`create`、`update`、`append`、`rename` が `AuditOperation` として区別されること
2. 認可失敗が独立 operation ではなく、元の `operation` と `result` の組で表現されること
3. `append` が独立 operation として記録されること
4. `rename` 成功時に `target_path` へ移動元 path、`summary` へ移動先 path が入ること

### 3.3 `result` の詳細分類

1. 成功系が `Success` で記録されること
2. スコープ不足が `ScopeDenied` で記録されること
3. path prefix 制約違反が `PathPrefixDenied` で記録されること
4. 競合系が `Conflict` として記録されること
5. 入力不正が `InvalidInput` として記録されること
6. 対象外操作が `Unsupported` として記録されること
7. 内部異常が `InternalError` として記録されること

## 4. 成功系記録の観点

### 4.1 read系操作

1. `get` / `get_section` 成功時に `operation` と対象 path が記録されること
2. `list` / `search` で prefix 指定ありの場合、その要求 prefix が `target_path` に記録されること
3. `list` / `search` で prefix 指定なしの場合、`target_path = null` を許容すること
4. read 系単純成功で `summary = null` を許容すること

### 4.2 write系操作

1. `create` 成功時に path、revision、成功結果が記録されること
2. `update` 成功時に path、revision、成功結果が記録されること
3. `append` 成功時に path、revision、成功結果が記録されること
4. `rename` 成功時に移動元 path と移動先補足が記録されること
5. write 系成功時に `token_id` が記録されること

## 5. 認可失敗記録の観点

### 5.1 スコープ不足

1. Bearer 認証成功後の required scope 不足が監査ログ対象になること
2. スコープ不足時に `operation` は元の要求操作を維持すること
3. スコープ不足時に `result = ScopeDenied` になること
4. スコープ不足時に `token_id` が記録されること
5. スコープ不足時に `revision = null` になること
6. `summary` に不足スコープ等の補足を入れられること

### 5.2 path prefix 制約違反

1. path prefix 制約違反が監査ログ対象になること
2. path prefix 制約違反時に `result = PathPrefixDenied` になること
3. 判定対象 path が定まる場合は `target_path` にその path が記録されること
4. rename の認可失敗時に、失敗した側の path が `target_path` に記録されること
5. rename の認可失敗時に、移動元 / 移動先のどちらで失敗したかを `summary` に含められること

### 5.3 非対象系との境界

1. 認証失敗が監査ログ JSONL に記録されないこと
2. list / search の結果後段フィルタ除外を認可失敗として起票しないこと
3. path 不正や対象不存在が `ScopeDenied` / `PathPrefixDenied` と混同されないこと

## 6. `append` 集約の観点

### 6.1 集約キー

1. 集約キーが `user_id`、`token_id`、`target_path` の組であること
2. 同一ユーザ・同一 path でも `token_id` が変われば別集約になること
3. path が異なれば別集約になること

### 6.2 保留状態の更新

1. 初回 `append` 成功時に保留状態が新規作成されること
2. 同一キーの追加 `append` で `append_count` が加算されること
3. 同一キーの追加 `append` で `last_timestamp` が更新されること
4. `first_timestamp` が初回値を維持すること
5. `revision` が初回イベント値を維持すること
6. amend 相当件数 / 新規 revision 件数などの `summary_seed` が集約できること

### 6.3 集約対象外

1. `append` 成功イベントだけが集約対象になること
2. `append` の認可失敗が保留状態へ入らず即時記録されること
3. `append` の競合失敗が保留状態へ入らず即時記録されること
4. `append` の入力不正が保留状態へ入らず即時記録されること

### 6.4 集約確定

1. 時間窓満了で保留状態が確定出力されること
2. キー切替で異なるキーの保留状態が確定出力されること
3. 明示 flush で保留中の全 `append` が確定対象へ移ること
4. プロセス終了時 flush で保留中の全 `append` が確定出力されること
5. 集約確定時に `summary` へ件数と期間が含まれること
6. 集約確定時に `timestamp` が確定時刻になること
7. 集約確定時に `revision` は初回 revision を使うこと

## 7. flush と writer の観点

### 7.1 通常書込

1. 1 レコードが 1 行の JSON object として書き込まれること
2. 各レコード末尾に LF が付くこと
3. 文字コードが UTF-8 BOM なしであること
4. `operation` と `result` が安定した小文字スネークケースで出力されること
5. `timestamp` が UTC の ISO8601 / RFC3339 形式で出力されること

### 7.2 明示 flush / 終了時 flush

1. 明示 flush 時に OS バッファまで反映されること
2. 終了時 flush が writer の終了シーケンスに含まれること
3. flush 実行後に保留中 `append` が書き残されないこと
4. flush 失敗時に保留状態を安易に破棄しないこと
5. flush 失敗が内部失敗として上位へ通知できること

## 8. rotation の観点

### 8.1 切替条件

1. レコード追加後に上限を超える場合、先にローテーションすること
2. 行の途中で分割せず、レコード単位で切り替えること
3. ローテーション済みファイルへ追記を戻さないこと
4. 新しいアクティブファイルが空から開始すること

### 8.2 命名規則

1. アクティブファイルが `audit.current.jsonl` であること
2. ローテーション済みファイルが `audit-YYYYMMDDTHHMMSSZ-NNNNNN.jsonl` 形式であること
3. 同一秒内の複数切替でも連番により一意になること
4. ディレクトリ列挙時に順序が安定すること

### 8.3 集約との連携

1. `append` 集約の保留状態がある場合、ローテーション前に明示 flush できること
2. ローテーション前 flush により集約レコードがファイル境界を跨がないこと

## 9. retention の観点

### 9.1 削除対象の選定

1. 判定対象がローテーション済みファイルのみであること
2. `audit.current.jsonl` が削除対象に含まれないこと
3. 保持期間超過判定が「現在時刻 - 保持期間」で行われること
4. ファイル名埋め込み UTC 時刻で削除可否を判定すること
5. 命名規則外ファイルを削除対象に含めず警告に留めること

### 9.2 実行契機

1. 起動時 retention が 1 回実行されること
2. 稼働中 retention が定期タスクとして実行されること
3. 監査ログ基盤無効時に retention を起動しないこと

### 9.3 失敗時の扱い

1. 一部ファイル削除失敗時に他候補の処理を継続すること
2. 個別削除失敗が警告ログへ記録されること
3. 監査ログディレクトリ自体へアクセスできない場合に起動失敗として扱えること

## 10. 役割分担の観点

### 10.1 AccessLogger / tracing / 監査ログ

1. 認証失敗が AccessLogger / tracing 側で扱われ、JSONL に入らないこと
2. 認可失敗が JSONL 監査ログへ入ること
3. Referer、User-Agent、レスポンスサイズ等が監査ログへ重複保存されないこと
4. `token_id`、`target_path`、`revision`、認可失敗詳細が監査ログ側で保持されること
5. `tracing` が監査証跡の正本ではなく内部診断補助として扱われること

## 11. 実施レイヤ分割の目安

- 単体テスト向き
  - `AuditRecord` の項目欠損条件
  - `AuditOperation` / `AuditResult` の写像
  - `append` 集約キーと保留状態更新
- サービス層結合テスト向き
  - 認可失敗時のイベント投入
  - rename / list / search の `target_path` / `summary` 組み立て
  - `append` 成功 / 失敗の集約対象判定
- ファイルI/O 統合テスト向き
  - JSONL 出力
  - rotation
  - 明示 flush / 終了時 flush
  - retention 削除
- HTTP / 起動統合テスト向き
  - 認証失敗非記録
  - 起動時 retention
  - サーバ終了時 flush

## 12. 4.9.3 の整理結果

4.9.3 の完了条件に対する整理結果は以下の通り。

- 成功系の観点を、read 系 / write 系ごとの `AuditRecord` 記録内容まで含めて定義した
- 認可失敗の観点を、スコープ不足、path prefix 制約違反、認証失敗非対象の境界まで含めて定義した
- `append` 集約の観点を、集約キー、保留状態、時間窓、キー切替、終了時 flush まで含めて定義した
- flush の観点を、明示 flush、終了時 flush、writer 失敗時の保留状態維持まで含めて定義した
- 保持削除の観点を、対象選定、起動時 / 定期実行、個別削除失敗時の継続まで含めて定義した

## 13. MCP prompts監査の観点

### 13.1 操作名

1. `prompts/list`を`list_prompts`として永続化すること
2. `list_prompts`を既存ページ一覧の`list`と区別すること
3. `prompts/get`を`get_prompt`として永続化すること
4. `get_prompt`を既存ページ取得の`get`と区別すること

### 13.2 `list_prompts`

1. 成功時に`target_path = null`、`revision = null`となること
2. 成功summaryが`count=<件数> has_more=<true|false>`だけであること
3. scope不足を`scope_denied`として記録すること
4. cursor不正を`invalid_input`として記録すること
5. DB失敗・候補不整合を`internal_error`として記録すること
6. 失敗summaryに固定公開messageだけを記録すること
7. cursor値、prompt名、prompt一覧を記録しないこと

### 13.3 `get_prompt`

1. 成功時に`target_path = null`、`revision = latest revision`となること
2. 値制約を満たす要求名だけを`name=<prompt名>`としてsummaryへ記録すること
3. 不正な要求名では`summary = null`となること
4. 不存在・非公開を`not_found`として記録すること
5. 引数不正を`invalid_input`として記録すること
6. scope不足を`scope_denied`として記録すること
7. front matter・名前索引不整合を`internal_error`として記録すること
8. 失敗時に`target_path = null`、`revision = null`となること

### 13.4 秘匿情報とbest-effort

1. prompt本文、system、展開後message、引数名・引数値を記録しないこと
2. front matter、ページpath、page ID、DB内部エラーを記録しないこと
3. Bearer token平文、Authorization header、request bodyを記録しないこと
4. user ID、token ID、取得可能なIP address、timestampを記録すること
5. 監査sink lock・書込み・ユーザID解決失敗でprompts応答を変更しないこと
6. transport認証失敗を監査JSONLへ記録しないこと

### 13.5 実施レイヤ分割

- handler・service結合テスト
  - 操作名、result、summary、revision、best-effort
- JSONL検査
  - 永続化名、null項目、秘匿情報非記録
- transport統合テスト
  - 入力元IP address、認証失敗非記録、request body非記録

## 14. MCP resources監査の観点

### 14.1 操作名

1. `resources/list`を`list_resources`として永続化すること
2. `list_resources`を既存ページ一覧の`list`および`list_prompts`と区別すること
3. `resources/read`を`read_resource`として永続化すること
4. `read_resource`を既存ページ取得の`get`および`get_prompt`と区別すること

### 14.2 `list_resources`

1. 成功時に`target_path = null`、`revision = null`となること
2. 成功summaryが`count=<件数> has_more=<true|false>`だけであること
3. scope不足を`scope_denied`として記録すること
4. cursor不正を`invalid_input`として記録すること
5. DB失敗・候補不整合を`internal_error`として記録すること
6. 失敗summaryに固定公開messageだけを記録すること
7. cursor値、resource URI一覧、resource名一覧を記録しないこと
8. path prefix範囲外により除外されたページ由来resourceを、
   個別の認可失敗監査レコードとして記録しないこと

### 14.3 `read_resource`

1. 固定組み込みresource成功時に`target_path = null`、`revision = null`となること
2. ページ由来resource成功時に`target_path = null`、
   `revision = latest revision`となること
3. 値制約を満たす要求URIだけを`uri=<resource URI>`としてsummaryへ記録すること
4. 成功時にresource種別を`kind=builtin`または`kind=page`としてsummaryへ記録すること
5. 不正な要求URIでは`summary = null`となること
6. 不存在・非公開を`not_found`として記録すること
7. URI不正を`invalid_input`として記録すること
8. scope不足を`scope_denied`として記録すること
9. URI索引不整合、latest source欠落、front matter再検証失敗を
   `internal_error`として記録すること
10. 失敗時に`target_path = null`、`revision = null`となること

### 14.4 秘匿情報とbest-effort

1. resource本文、front matter本文を記録しないこと
2. ページpath、page ID、DB内部エラーを記録しないこと
3. cursor値、resource URI一覧、resource名一覧を記録しないこと
4. path prefix範囲外であることを示す詳細を記録しないこと
5. Bearer token平文、Authorization header、request bodyを記録しないこと
6. user ID、token ID、取得可能なIP address、timestampを記録すること
7. 監査sink lock・書込み・ユーザID解決失敗でresources応答を変更しないこと
8. transport認証失敗を監査JSONLへ記録しないこと

### 14.5 実施レイヤ分割

- handler・service結合テスト
  - 操作名、result、summary、revision、best-effort
- JSONL検査
  - 永続化名、null項目、秘匿情報非記録
- transport統合テスト
  - 入力元IP address、認証失敗非記録、request body非記録
