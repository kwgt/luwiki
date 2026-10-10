# 監査ログ設計

本書は、MCP 実装設計で具体化した監査ログ設計を、
横断基盤として独立に整理するための設計書である。

初期の設計起点は MCP 機能であるが、
監査ログ基盤そのものは MCP 専用モジュールとして閉じず、
RESTのBasic/Bearer双方からも利用する共通基盤として扱う。

そのため、実装上の責務配置は `src/mcp/` 配下ではなく、
独立した `src/audit/` モジュールを前提とする。

関連する設計文書は以下の通り。

- `docs/MCP_INTERNAL_DESIGN.md`
  - 監査ログを含む横断責務の前提と、分冊全体の参照入口を確認する場合に参照する
- `docs/MCP_ARCHITECTURE_DESIGN.md`
  - `src/audit/` を含む責務配置と既存モジュールへの接続位置を確認する場合に参照する
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - 認可失敗、`append`、更新系操作から監査イベントへ渡す業務情報を確認する場合に参照する
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - 監査ログ設定値、起動時初期化、保守タスク、終了時 flush の接続を確認する場合に参照する
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 公開ツール、エラー応答、監査対象操作の粒度を確認する場合に参照する
- `docs/MCP_AUDIT_LOG_TEST_VIEWPOINTS.md`
  - 監査ログ基盤のテスト観点を確認する場合に参照する
- `docs/MCP_RESOURCE_SPECS.md`
  - MCP resourcesの監査対象操作、summary、秘匿情報を確認する場合に参照する

本書は主として以下を定義する。

- 監査ログ基盤の責務配置
- 監査ログレコードモデル
- `append` 集約ロジックの状態管理
- JSONL 保存形式とローテーション規則
- 保持期間超過ログの自動削除
- `tracing` および既存アクセスログとの役割分担

## 1. 対象範囲

監査対象はMCPと定義済みREST公開操作（Basic/Bearer双方）とする。REST拡張の正本は第11節。

ただしモジュール設計上は、
以下のような将来拡張を阻害しないことを前提とする。

- CLI 管理操作の一部記録
- export / import や管理系機能からの監査イベント投入

要求仕様上の原則は以下の通りとする。

- 監査ログは Wiki ページ本体とは別の専用パスへ保存する
- 保存先ストレージは別物に分離しない
- 認証失敗は監査ログではなく HTTP ログで扱う
- 認可失敗は監査ログ対象に含める

## 2. 責務配置

監査ログ基盤は、
handler や service に処理を埋め込むのではなく、
「イベント生成」と「記録実行」を分離した専用モジュールとして設計する。

初期案の責務分割は、少なくとも以下の 5 単位とする。

1. 監査イベント生成入口
2. 集約・バッファ管理
3. 永続化 writer
4. ローテーション管理
5. 保持削除管理

各責務の配置方針は以下とする。

- 監査イベント生成入口
  - 公開層および共通サービス層から呼び出される薄い入口とする
  - 入力は `operation`、`user_id`、`token_id`、`address`、`target_path`、`result`、`timestamp`、`summary`、`revision` を基本とする
  - 認証失敗はこの入口へ渡さず、認可失敗および業務結果だけを受け付ける
  - handler 側で transport 文脈を補い、service 側で業務結果を確定したうえで入口へ渡す
- 集約・バッファ管理
  - `append` の集約条件判定、時間窓管理、未 flush レコード保持を担当する
  - 単純な成功・失敗イベントは即時書込対象へ流し、`append` だけを条件付きで保留できるようにする
  - writer に直接ファイル I/O を持ち込まず、メモリ上の保留状態と flush 判定に責務を限定する
- 永続化 writer
  - 確定済み監査レコードを JSONL へ追記する責務を持つ
  - 1 件単位または flush 単位の受け取りを担当し、レコード内容の業務判断は行わない
  - ファイルオープン、追記、flush、プロセス終了時の最終 flush を担当する
- ローテーション管理
  - 固定サイズ到達判定と出力先切替を担当する
  - writer から切り離し、どのファイルへ書くかを決める責務として扱う
  - JSONL のレコード内容や `append` 集約条件には関与しない
- 保持削除管理
  - 保持期間超過ファイルの列挙と削除実行を担当する
  - 起動時または定期契機で呼び出される保守責務として扱い、通常のレコード書込経路からは分離する
  - 現在アクティブな書込先ファイルは削除対象に含めない

`src/audit/` 配下の初期構成案は以下とする。

- `src/audit/mod.rs`
  - 外部公開入口
  - 依存組み立て
- `src/audit/model.rs`
  - 監査レコード
  - 集約キー
  - 内部結果分類
- `src/audit/sink.rs`
  - 監査イベント生成入口
  - handler / service から見える投入 API
- `src/audit/buffer.rs`
  - `append` 集約
  - 保留レコード管理
- `src/audit/writer.rs`
  - JSONL 追記
  - flush 実行
- `src/audit/rotation.rs`
  - 固定サイズローテーション
  - 出力ファイル切替
- `src/audit/retention.rs`
  - 保持期間超過ファイル削除
  - 起動時 / 定期保守補助

責務境界を明確にするため、各層との接続は以下のように制限する。

- 公開層 / service
  - 監査イベントを生成入口へ渡す
  - ファイルパス、ローテーション、保持削除を直接扱わない
- 監査ログ基盤
  - 監査レコードの集約、永続化、ローテーション、保持削除を担当する
  - 業務応答生成や認可判定には関与しない
- HTTP サーバ統合層
  - 監査ログ基盤の初期化、終了時 flush、保守タスク起動を担当する
  - 監査レコード内容の生成は担当しない

## 3. 監査ログレコードモデル

監査ログ 1 件は、JSONL へ直列化できる単位として
単一レコードモデルで表現する。
初期実装では `before_revision` / `after_revision` のような複数 revision 項目は持たず、
単一の `revision` で扱う。

概念上のレコード型は以下を基本とする。

```rust
struct AuditRecord {
    operation: AuditOperation,
    user_id: UserId,
    token_id: Option<TokenId>,
    address: Option<IpAddr>,
    target_path: Option<NormalizedPath>,
    result: AuditResult,
    timestamp: DateTime<Utc>,
    summary: Option<String>,
    revision: Option<u64>,
}
```

各項目の型と欠損条件は以下の方針とする。

| 項目 | 型の初期案 | 必須性 | `null` 許容条件 |
|:--|:--|:--|:--|
| `operation` | `AuditOperation` | 必須 | 許容しない |
| `user_id` | `UserId` | 必須 | 許容しない |
| `token_id` | `Option<TokenId>` | 条件付き必須 | Bearer 文脈を持たない場合、または内部例外で確定できない場合 |
| `address` | `Option<IpAddr>` | 条件付き必須 | Actix 接続情報から取得できない場合 |
| `target_path` | `Option<NormalizedPath>` | 条件付き必須 | 対象pathを持たない操作、解決不能、認可失敗または異常系の場合 |
| `result` | `AuditResult` | 必須 | 許容しない |
| `timestamp` | `DateTime<Utc>` | 必須 | 許容しない |
| `summary` | `Option<String>` | 任意 | 補足不要時 |
| `revision` | `Option<u64>` | 条件付き必須 | 認可失敗、一覧・検索系、対象 revision を持たない操作の場合 |

型選定の理由は以下とする。

- `operation`
  - 外部操作粒度に対応する列挙型とする
  - 文字列の自由入力ではなく列挙に固定し、JSONL 出力時だけ安定した文字列表現へ写像する
- `user_id`
  - 既存 DB モデルと対応する識別子型をそのまま使う
  - ユーザ名ではなく不変識別子を記録対象とする
- `token_id`
  - Bearerではtokenを記録し、REST Basicを含むため`Option<TokenId>`とする
  - MCP では通常 Bearer 前提のため実質必須だが、モデル上は異常系を表せるよう `Option` を維持する
- `address`
  - 監査入力元はリモート IP であり、初期案では `IpAddr` 相当で保持する
  - 永続化時は文字列表現に写像してもよいが、内部モデルではアドレス型を優先する
- `target_path`
  - 監査対象 path は正規化済み絶対 path に限定する
  - そのため内部モデルでは自由文字列ではなく `NormalizedPath` 相当の型で保持する
- `result`
  - 外部応答用エラー区分とは別の、監査用詳細結果分類を持つ列挙型とする
- `timestamp`
  - 保存時刻比較と将来のエクスポート整合性を考慮し、内部では `UTC` 基準で保持する
  - 表示時や CLI では別途ローカル時刻へ変換してよい
- `summary`
  - サーバ生成補足文言として扱い、クライアント入力は直接保持しない
- `revision`
  - revision を持つ操作だけで利用するため `Option<u64>` とする

`AuditOperation` は、少なくとも以下の粒度を区別する。

- `Get`
- `GetSection`
- `List`
- `ListPrompts`
- `GetPrompt`
- `ListResources`
- `ReadResource`
- `ListPageAssets`
- `ResolvePageAsset`
- `GetPageAssetMetadata`
- `GetPageAssetData`
- `UploadPageAsset`
- `DeletePageAsset`
- `Search`
- `Create`
- `Update`
- `Append`
- `Rename`

認可失敗は独立 operation とせず、
「何をしようとしたか」を表す `operation` と
「どう失敗したか」を表す `result` を分離して保持する。

`AuditResult` は、少なくとも以下の分類を区別できるものとする。

- `Success`
- `ScopeDenied`
- `PathPrefixDenied`
- `ReadOnlyDenied`
- `NotFound`
- `Conflict`
- `InvalidInput`
- `Unsupported`
- `InternalError`

必要に応じて後続設計で `Conflict` や `InvalidInput` の下位分類を追加してよいが、
JSONL 上の後方互換性を壊さないよう、初期実装では安定した列挙名を優先する。

項目ごとの欠損条件は、操作種別ごとに以下を基本とする。

- `token_id`
  - Bearer/MCPの正常系・認可失敗系では記録する
  - REST Basicでは通常の成功/失敗とも`null`とする。MCP/Bearerの通常認証済み操作ではtoken_idを記録する
- `address`
  - 取得できた場合は必ず記録する
  - 接続情報欠損時のみ `null` を許容する
- `target_path`
  - `get` / `get_section` / `create` / `update` / `append` / `rename` では原則記録する
  - アセット 6 tool では正規化済み page path を記録する
  - `list` / `search` は prefix 指定がある場合にその要求 prefix を記録し、prefix なしの場合は `null` を許容する
  - 認可失敗では、判定対象 path が定まる場合はその path を記録する
- `revision`
  - `create` / `update` / `append` / `rename` 成功時は記録する
  - `append` 集約時も「操作対象指定時の revision」を記録する
  - `get` / `get_section` は、取得対象 revision が確定している場合のみ記録してよい
  - アセット操作は page 本文 revision に直接対応しないため、成功時も `null` とする
  - `list` / `search`、認可失敗、入力不正、対象外操作では `null` を許容する
- `summary`
  - rename の移動先補足
  - append の amend 相当 / 新規 revision 区別
  - 認可失敗時の不足スコープや制約違反理由
  - 上記が不要な単純成功系では `null` を許容する

rename の扱いは単一 `target_path` モデルに合わせ、以下とする。

- 成功時
  - `target_path` には移動元 path を記録する
  - 移動先 path は `summary` に含める
- 認可失敗時
  - 失敗した側の path を `target_path` に記録する
  - 移動元 / 移動先のどちらで失敗したかを `summary` に含める

## 4. `append` 集約ロジックの状態管理

`append` の監査ログは、毎回即時に 1 件ずつ永続化するのではなく、
同一主体・同一対象に対する短時間の連続追記を
1 件の集約レコードとして確定できるようにする。

初期案では、集約の状態管理を
「集約キー」「保留状態」「flush 契機」の 3 要素で定義する。

集約キーは以下の 3 項目の組み合わせとする。

- `user_id`
- `token_id`
- `target_path`

この 3 項目がすべて一致する連続 `append` だけを同一集約対象とする。
`token_id` が変わる場合は、同一ユーザ・同一 path であっても別集約とみなす。

概念上の保留状態は、少なくとも以下を保持できるものとする。

```rust
struct PendingAppendAudit {
    key: AppendAuditKey,
    first_timestamp: DateTime<Utc>,
    last_timestamp: DateTime<Utc>,
    append_count: u64,
    revision: Option<u64>,
    summary_seed: AppendAuditSummarySeed,
    record_template: AuditRecord,
}
```

各保持項目の役割は以下とする。

- `key`
  - 同一集約対象かどうかの判定に使う
- `first_timestamp`
  - 集約期間の開始時刻を表す
  - 集約 summary や監査確認時の期間表示に利用する
- `last_timestamp`
  - 最後に `append` を受け取った時刻を表す
  - 時間窓タイムアウト判定の基準とする
- `append_count`
  - 集約した `append` 件数
  - 確定時 summary へ含める
- `revision`
  - 「操作対象指定時の revision」を保持する
  - 後続 `append` 到着時に上書きせず、最初に取り込んだ値を維持する
- `summary_seed`
  - amend 相当か新規 revision 生成かなど、
    後で summary へ展開するための中間情報を保持する
- `record_template`
  - `operation`、`user_id`、`token_id`、`address`、`target_path`、`result` など
    集約中に不変であるべき値を保持する

状態管理の基本ルールは以下とする。

1. `append` 成功イベント受理時に集約キーを計算する
2. 同一キーの保留状態がなければ新規に作成する
3. 同一キーの保留状態があれば `last_timestamp` と `append_count` を更新する
4. キー不一致の保留状態は、その時点で確定候補として flush 対象へ回す

保留状態の更新ルールは以下とする。

- `append_count` は受理ごとに加算する
- `last_timestamp` は最新イベント時刻で更新する
- `first_timestamp` は初回値を維持する
- `revision` は初回イベントで得た値を維持する
- `summary_seed` は、少なくとも以下を集約できるようにする
  - amend 相当件数
  - 新規 revision 件数
  - 必要であれば最後に観測した補足情報

flush 契機は、少なくとも以下の 4 種を持つものとする。

1. 時間窓満了 flush
2. キー切替 flush
3. 明示 flush
4. プロセス終了時 flush

各 flush 契機の扱いは以下とする。

- 時間窓満了 flush
  - `last_timestamp` から 1 分以上経過した保留状態を確定出力する
  - 初期案の時間窓は固定で 1 分とする
- キー切替 flush
  - 実装が単一保留状態または限定数の保留状態を扱う場合、
    新しいキーの `append` を受けた時点で既存の異なるキーを確定出力する
  - 複数キー同時保持を許す実装でも、同一キー以外は別保留状態として独立管理する
- 明示 flush
  - ローテーション直前、保守処理前、将来の管理 API 呼び出し時などで利用可能な内部操作として用意する
  - この時点で保留中の全 `append` 集約を確定対象へ移す
- プロセス終了時 flush
  - サーバ正常停止時に、保留中の全 `append` 集約を確定出力する
  - 終了時 flush は best-effort ではなく、writer の終了シーケンスに含める

時間窓の管理方法は、初期案として以下のどちらでもよいが、
責務は `buffer.rs` 側に置く。

- 定期 tick ごとに保留状態を走査する
- 新規イベント受理時と終了時に期限切れ状態を判定する

ただし、設計上は将来複数キー保留を許容できるよう、
保留状態は「キーから保留状態への map」として扱える形を前提にする。

```rust
type PendingAppendMap = BTreeMap<AppendAuditKey, PendingAppendAudit>;
```

この形にしておくことで、
短時間に複数ページへ並行して `append` が来た場合でも、
キーごとに独立して時間窓を管理できる。

確定出力時のレコード組み立ては以下とする。

- `record_template` を基礎に確定レコードを生成する
- `summary` には集約件数と期間を含める
- `timestamp` は flush 時刻ではなく、集約レコード確定時刻を採用する
- `revision` は保留状態が保持していた初回 revision を使う

失敗系の扱いは以下とする。

- `append` 成功イベントだけを集約対象に含める
- `append` の競合失敗、認可失敗、入力不正は即時に通常監査レコードとして扱い、保留状態へ入れない
- writer への flush 失敗時は保留状態を安易に破棄せず、上位へ内部失敗として返せるようにする

後続の永続化・保持削除へつながる前提として、
ローテーション前および保持削除前には、
対象ファイル境界を跨がないよう先に保留状態を flush できる構造とする。

## 5. JSONL 保存形式とファイル分割ルール

監査ログの永続化形式は JSONL とし、
1 レコードを 1 行へ直列化して追記する。
単一巨大ファイル化を避けるため、ファイル分割は固定サイズローテーションで行う。

保存先の基本方針は以下とする。

- Wiki ページ本体とは別の専用ディレクトリへ保存する
- ただし保存先ストレージは別物へ分離せず、同一データ領域配下で扱う
- 初期案の既定ディレクトリはデータルート配下の `audit/` とする

既定パスの概念上の例は以下とする。

- DB 既定パス: `data/database.redb`
- 監査ログ既定ディレクトリ: `data/audit/`

上記の`data/`は初期設計時の概念例であり、現行の既定値・設定キー・
CLIオプションは[CLI仕様](CLI_SPECS.md)の監査ログ設定を正とする。
本書では「監査ログ出力ディレクトリ」を単一の設定値として扱う。

ファイル命名規則は、少なくとも以下の性質を満たすものとする。

- ファイル名だけで監査ログ用途であることが識別できる
- ディレクトリ走査時に順序が安定する
- ローテーション後も一意に識別できる
- 将来の export / import や保守ツールが素直に列挙できる

初期案の命名規則は以下とする。

- アクティブ書込ファイル
  - `audit.current.jsonl`
- ローテーション済みファイル
  - `audit-YYYYMMDDTHHMMSSZ-000001.jsonl`
  - `audit-YYYYMMDDTHHMMSSZ-000002.jsonl`
  - ...

ここで、

- `YYYYMMDDTHHMMSSZ`
  - ローテーション確定時刻の UTC 表記
- 末尾連番
  - 同一秒内に複数回切替が発生しても一意性を維持するための補助番号

とする。

アクティブファイルとローテーション済みファイルを分ける理由は以下とする。

- 現在追記中の対象を即時に識別できる
- 保持削除処理でアクティブファイルを除外しやすい
- 不意の停止時でも「どこまでが追記対象か」を判定しやすい

追記方式は以下の方針とする。

- writer はアクティブファイルを append モードで開く
- 1 レコードごとに 1 行の JSON を出力し、末尾に LF を付与する
- 途中行の上書きや JSON 配列化は行わない
- flush 単位は writer 実装に委ねるが、少なくとも明示 flush と終了時 flush では OS バッファまで反映させる

直列化ルールは以下とする。

- 1 行は 1 個の JSON object とする
- 文字コードは UTF-8、BOM なしとする
- 改行コードは LF とする
- `null` 許容項目は JSON `null` で表現する
- MCPの`operation`と`result`は安定した小文字スネークケース文字列。REST operationは第11節の固定メソッド＋route template文字列とし、resultは安定した分類文字列を使う
- `timestamp` は UTC の ISO8601 / RFC3339 形式文字列で保存する

固定サイズローテーションの単位は、
初期案として「ファイルサイズ上限到達前に切替判定を行う」方式とする。

具体的には以下の順序を採る。

1. 書込対象レコードを 1 行の JSONL 文字列へ直列化する
2. 直列化後サイズを見積もる
3. 追加後に上限を超える場合は、先にアクティブファイルをローテーションする
4. 切替後の新しいアクティブファイルへ当該レコードを書き込む

ローテーションの境界条件は以下とする。

- 判定単位は「レコード単位」であり、行の途中で分割しない
- `append` 集約の保留状態がある場合は、先に明示 flush してからローテーションしてよい
- ローテーション済みファイルは追記対象へ戻さない
- ローテーション後の新規アクティブファイルは空ファイルから開始する

固定サイズ上限値そのものは設定値化を前提とする。
ただし初期実装では未設定時の内部既定値を持つものとし、
既存通常ログの 2 MiB 上限と同程度の運用感を候補にしてよい。

障害時の整合性方針は以下とする。

- 行単位で完結する JSONL のため、異常終了時は末尾行のみが不完全になる可能性を許容する
- 保守・確認ツール側では、末尾の不完全行を検出して無視または異常扱いできるようにする
- 既に確定済みでローテーションされたファイルは、後続書込で再変更しない

## 6. 保持期間超過ログの自動削除

保持期間を超過した監査ログファイルは、自動削除の対象とする。
削除責務は通常の書込経路とは分離し、
`retention.rs` から呼び出される保守処理として扱う。

初期案では、自動削除の実行契機を
「起動時の一度きり実行」と
「サーバ稼働中の定期実行」
の 2 段構成とする。

各実行契機の役割は以下とする。

- 起動時実行
  - 既に保持期限を超過している古い監査ログを早期に掃除する
  - 長期間停止していた後の再起動でも、初回起動時点で不要ファイルを減らせる
  - 監査ログ基盤の依存初期化処理の一部として扱う
- 定期実行
  - 稼働中に保持期限を超過したファイルを継続的に削除する
  - HTTP サーバ統合層から保守タスクとして起動する
  - 実行間隔は設定値ではなく内部固定値でもよいが、少なくとも分単位以上の粗い周期で十分とする

保持期間の適用単位は「個々のローテーション済み監査ログファイル」とする。
レコード単位でファイル内を再編集して削除することは行わない。

削除判定の基準は以下とする。

- 判定対象はローテーション済みファイルのみとする
- アクティブファイル `audit.current.jsonl` は削除対象に含めない
- 保持期間の基準時刻は「現在時刻 - 設定された保持期間」とする
- ファイルの保持判定には、原則としてローテーション済みファイル名に埋め込んだ UTC タイムスタンプを用いる
- ファイル名が規則外で判定できない場合は削除対象へ含めず、警告ログを出して温存する

ファイル名ベース判定を優先する理由は以下とする。

- JSONL 本文を開かずに削除可否を判断できる
- 大きな監査ログ群でも走査コストを抑えられる
- ローテーション時刻単位で保持運用を単純化できる

削除処理の基本フローは以下とする。

1. 監査ログ出力ディレクトリを列挙する
2. `audit.current.jsonl` を除外する
3. 命名規則に一致するローテーション済みファイルだけを候補にする
4. ファイル名からローテーション時刻を復元する
5. 保持期限を超過したファイルを削除する
6. 削除件数およびスキップ件数を内部ログへ記録する

`append` 集約との関係は以下とする。

- 保留中の `append` 集約はまだファイル化されていないため、保持削除の直接対象にならない
- ただし削除処理の前後でローテーションや flush と競合しないよう、実行直前に writer 側のアクティブファイル状態を固定できる構造とする
- 保持削除はアクティブファイルを触らないため、通常は明示 flush を必須としない
- 一方で、将来ローテーション直後に削除判定を行う実装では、先にローテーション完了状態を確定させてから retention を実行する

失敗時の扱いは以下とする。

- 削除対象ファイルの一部削除に失敗しても、他の候補は継続して処理してよい
- 個別失敗は警告ログへ記録し、サーバ全体の致命的起動失敗には直結させない
- ただし起動時に監査ログディレクトリ自体へアクセスできない場合は、監査ログ依存失敗として起動失敗としてよい

HTTP サーバ統合層との接続は以下とする。

- 監査ログ基盤が有効な場合のみ retention 初期化を行う
- サーバ起動シーケンス中に起動時 retention を一度実行する
- サーバ稼働中は `audit_retention_task` のような定期タスクを起動する

## 7. `tracing` と既存アクセスログとの役割分担

監査ログ設計では、
`tracing`、既存 `AccessLogger`、監査ログ JSONL を
同じものとして扱わず、用途の異なる 3 系統として分離する。

初期案の責務分担は以下とする。

- `AccessLogger`
  - HTTP リクエスト単位の受付・応答記録
- `tracing`
  - 監査イベント生成および内部診断用イベント
- 監査ログ JSONL
  - 要求仕様に基づく永続的な監査証跡

各系統の主目的は以下の通りとする。

- `AccessLogger`
  - 「いつ、どこから、どの HTTP request が来て、どう返したか」を残す
  - 運用上の通信確認、障害一次調査、認証失敗の記録を担う
- `tracing`
  - handler / service / audit 基盤の内部イベントを構造化して扱う
  - 監査ログレコード生成の中間表現や内部失敗原因の関連付けに使う
- 監査ログ JSONL
  - 「誰が、どの対象に、どの操作を行い、どういう結果になったか」を監査目的で保存する
  - Access log より業務寄り、`tracing` より永続証跡寄りの責務を持つ

既存 `AccessLogger` が保持する情報は、少なくとも以下である。

- リモートアドレス
- 認証済みユーザ
- HTTP メソッド
- リクエストパスとクエリ
- HTTP バージョン
- ステータスコード
- レスポンスサイズ
- Referer
- User-Agent
- 処理時間

このため、`AccessLogger` は HTTP transport 観点のログとして維持し、
以下の事項は監査ログへ移管しない。

- Referer
- User-Agent
- レスポンスサイズ
- HTTP バージョン
- 生のリクエスト URI 全体
- ミリ秒未満の応答時間

一方で、監査ログ JSONL で保持すべきであり、
`AccessLogger` のみでは不十分な情報は以下とする。

- 操作種別
- path ベースの対象
- Bearer `token_id`
- path prefix 制約違反やスコープ不足などの認可失敗詳細
- `append` 集約結果
- revision
- サーバ生成 `summary`

`tracing` の役割は、上記 2 系統の橋渡しではなく、
主として内部イベントの構造化に置く。

具体的には以下を担当する。

- request の開始 / 終了イベント
- 認証成功 / 失敗、認可判定、サービス実行、監査レコード確定などの内部 span / event
- writer flush 失敗、retention 失敗、rotation 実行などの内部診断情報

ただし、`tracing` のイベント自体を
監査証跡の正本として扱うことはしない。
監査上の保存対象はあくまで JSONL レコードであり、
`tracing` はその生成過程および内部診断補助として扱う。

重複・非重複範囲は以下のように整理する。

- 重複してよい項目
  - リモートアドレス
  - 認証済みユーザ
  - タイムスタンプ
  - 成否の大枠
- `AccessLogger` のみが持つ項目
  - HTTP メソッド、URI、ステータスコード、レスポンスサイズ、Referer、User-Agent、処理時間
- 監査ログ JSONL のみが持つ項目
  - `operation`、`token_id`、`target_path`、`result` の詳細分類、`revision`、`summary`
- `tracing` のみが持つ項目
  - 内部処理段階、span 文脈、ローテーション / retention / flush の内部成否、実装診断用フィールド

認証失敗時の扱いは以下とする。

- 認証失敗は監査ログ JSONL へ記録しない
- 認証失敗は `AccessLogger` および必要に応じた `tracing` の warning / error event で扱う
- 認可失敗は監査ログ JSONL の対象とする

HTTP endpoint に対しても既存 `AccessLogger` を適用し、
HTTP request 自体は必ずアクセスログへ残す。
その上で、業務操作として監査対象となるものだけを
監査ログ JSONL へ追加記録する二段構成とする。

### 7.1 初期実装での `tracing` 前提構成

4.6 の初期実装では、
監査仕様そのものは本書で定義した自前モデルを維持しつつ、
実行基盤は `tracing` を前提に構成する。

ここでの基本方針は、
「公開層 / service 層が監査イベントを確定し、
`src/audit/` が `tracing` と JSONL 永続化の境界を引き受ける」
という責務分担とする。

初期実装で固定する構成方針は以下とする。

- handler / service
  - 監査対象操作の結果を `AuditRecord` 相当の内部モデルへ整形する
  - `append` 成功だけを集約対象として識別する
  - `tracing` の subscriber / layer / writer 初期化には関与しない
- `src/audit/sink.rs`
  - 公開層から受け取った監査イベント投入の単一入口とする
  - 通常イベントと `append` 集約対象イベントの振り分けを担当する
  - 監査イベント投入時に、必要な内部診断用 `tracing` event を発行してよい
- `src/audit/buffer.rs`
  - `append` 集約キー計算、保留状態更新、期限切れ判定、明示 flush 対象抽出を担当する
  - ファイル I/O や `tracing` subscriber 構築責務は持たない
- `src/audit/writer.rs`
  - 確定済み `AuditRecord` を JSONL 正本へ永続化する
  - 実装上は `tracing` 前提の非同期安全な書込経路へ接続する
  - ただし永続化されるレコード内容の正本は、`tracing` event そのものではなく `AuditRecord` とする
- `src/audit/rotation.rs`
  - JSONL 書込前後のファイル切替判定を担当する
  - `append` 保留状態がある場合の事前 flush 契機を writer 側へ要求できる構造とする
- `src/audit/retention.rs`
  - ローテーション済みファイルの列挙と削除だけを担当する
  - アクティブ writer や保留バッファの内部状態を直接変更しない
- HTTP サーバ統合層
  - `tracing` を含む監査基盤の初期化、終了時 flush、定期 retention 起動を担当する
  - 業務レコードの内容生成は担当しない

この構成でのイベント流れは、概念上以下の順とする。

1. handler または service が監査対象結果を確定する
2. `sink` が通常イベントか `append` 集約対象かを判定する
3. 通常イベントは writer 経路へ渡し、`append` 成功は buffer へ保留する
4. buffer が flush 契機に応じて確定済み `AuditRecord` 群を組み立てる
5. writer が rotation と連携しつつ JSONL 正本へ永続化する
6. retention は独立した保守契機でローテーション済みファイルだけを掃除する

`tracing` との接続位置は、以下のように整理する。

- 監査イベントの業務的な正本
  - `AuditRecord` とその集約状態
- `tracing` に委ねるもの
  - 監査投入、buffer flush、writer 書込、rotation、retention、終了時 flush の内部診断イベント
  - 複数要求や将来の通知処理が並行した場合でも、処理段階を構造化して追跡するための span / field
- `tracing` に委ねないもの
  - 監査レコードの項目定義そのもの
  - `append` 集約ルール
  - JSONL 正本の命名規則、保持期間、監査対象判定

監査イベント投入を `tracing` へどこまで寄せるかについては、
初期実装では「`tracing` を内部診断と実行基盤に使い、
監査レコードの業務判断は `sink` / `buffer` / `writer` 側に残す」
方針とする。

これは、以下の理由による。

- `append` 集約は、監査仕様に依存する時間窓と保留状態を持ち、
  単純な event 転送だけでは責務が表現しにくい
- 監査 JSONL は要求仕様上の正本であり、
  `tracing` 出力形式へ業務仕様を従属させない方が安定する
- 一方で並行投入、終了時 drain、writer 失敗診断は
  `tracing` を前提にした構造の方が将来拡張に耐えやすい

`append` 集約の責務位置は、
`tracing` layer 側ではなく `buffer.rs` 側に置く。

理由は以下の通りとする。

- 集約キーは `user_id`、`token_id`、`target_path` という
  監査仕様固有の意味を持つ
- amend 相当件数や新規 revision 件数の集約は、
  subscriber 汎用責務ではなく監査仕様責務である
- 将来 `resources/*` や通知系イベントが追加されても、
  監査対象ごとの集約条件を `src/audit/` 内で閉じて保守できる

JSONL writer の接続方針は以下とする。

- writer は `AuditRecord` またはその配列を受け取り、
  JSONL 1 行単位の永続化へ専念する
- writer 実装は `tracing` 前提の非同期安全な構成を採るが、
  監査レコードの確定前に event 列へ分解して再構築する設計は採らない
- これにより、JSONL 正本生成の責務と内部診断イベントの責務を分離する

rotation / retention / shutdown flush の接続方針は以下とする。

- rotation
  - writer 書込経路から呼ばれるファイル切替責務として扱う
  - 必要時は先に buffer へ明示 flush を要求し、
    集約途中レコードがファイル境界を跨がないようにする
- retention
  - HTTP サーバ統合層から起動時および定期保守として呼び出す
  - アクティブファイルや保留バッファを直接変更しない
- shutdown flush
  - HTTP サーバ停止経路から監査基盤の終了シーケンスを呼び出し、
    buffer の保留状態を全 flush した後に writer の最終 flush を行う
  - この終了処理は best-effort ではなく、
    初期実装でも明示的な停止手順に含める

以上より、4.6 の再着手では
`tracing` を「監査仕様の代替」ではなく
「監査基盤を非同期安全かつ診断可能に動かすための実行基盤」として用いる。
一方で、監査対象判定、`append` 集約、JSONL 正本モデルは
引き続き `src/audit/` の自前責務として維持する。

## 8. MCP promptsの監査設計

### 8.1 `list_prompts`

`AuditOperation::ListPrompts`の永続化名は`list_prompts`とし、
既存ページ一覧の`list`と区別する。

成功時は次の内容を記録する。

- `result = success`
- `target_path = null`
- `revision = null`
- `summary = "count=<件数> has_more=<true|false>"`

失敗時は次の結果分類を使用する。

- scope不足: `scope_denied`
- cursor不正: `invalid_input`
- DB失敗または候補不整合: `internal_error`

失敗時も`target_path = null`、`revision = null`とし、summaryには
`operation is not allowed`、`cursor is invalid`、`internal error`の
固定公開messageだけを記録する。

cursor値、prompt名、prompt一覧は記録しない。

### 8.2 `get_prompt`

`AuditOperation::GetPrompt`の永続化名は`get_prompt`とし、
既存ページ取得の`get`と区別する。

成功時は次の内容を記録する。

- `result = success`
- `target_path = null`
- `revision = latest revision`
- 値制約を満たす要求prompt名だけを`name=<prompt名>`としてsummaryへ記録可能

失敗時は次の結果分類を使用する。

- prompt不存在・非公開: `not_found`
- 引数不正: `invalid_input`
- scope不足: `scope_denied`
- DB失敗、front matter・名前索引不整合: `internal_error`

失敗時は`target_path = null`、`revision = null`とする。
不正な要求prompt名はsummaryへ記録しない。

### 8.3 共通項目と記録禁止情報

両操作とも、ユーザID、token ID、取得可能な入力元IP address、
timestampを既存共通項目として記録する。

監査書込みはbest-effortとし、sink lock、書込み、ユーザID解決の失敗によって
prompts操作の成功・失敗結果を変更しない。

次の情報は監査ログへ記録しない。

- prompt本文、system、展開後message
- 引数名と引数値
- front matter全体
- ページpath、page ID
- cursor値、prompt一覧
- DB内部エラー
- Bearer token平文、Authorization header
- serialize済みrequest body

## 9. MCP resourcesの監査設計

### 9.1 `list_resources`

`AuditOperation::ListResources`の永続化名は`list_resources`とし、
既存ページ一覧の`list`およびprompt一覧の`list_prompts`と区別する。

成功時は次の内容を記録する。

- `result = success`
- `target_path = null`
- `revision = null`
- `summary = "count=<件数> has_more=<true|false>"`

失敗時は次の結果分類を使用する。

- scope不足: `scope_denied`
- cursor不正: `invalid_input`
- DB失敗または候補不整合: `internal_error`

失敗時も`target_path = null`、`revision = null`とし、summaryには
`operation is not allowed`、`cursor is invalid`、`internal error`の
固定公開messageだけを記録する。

cursor値、resource URI一覧、resource名一覧、ページpath、page IDは記録しない。
resource ACL非許可により一覧から除外されたページ由来resourceは、
個別の認可失敗監査レコードとして記録しない。

### 9.2 `read_resource`

`AuditOperation::ReadResource`の永続化名は`read_resource`とし、
既存ページ取得の`get`およびprompt取得の`get_prompt`と区別する。

成功時は次の内容を記録する。

- `result = success`
- `target_path = null`
- 固定組み込みresourceでは`revision = null`
- ページ由来resourceでは`revision = latest revision`
- 値制約を満たす要求URIだけを`uri=<resource URI>`としてsummaryへ記録可能
- resource種別を`kind=builtin`または`kind=page`としてsummaryへ記録可能

失敗時は次の結果分類を使用する。

- resource不存在・非公開: `not_found`
- URI不正: `invalid_input`
- scope不足: `scope_denied`
- DB失敗、URI索引不整合、latest source欠落、front matter再検証失敗:
  `internal_error`

失敗時は`target_path = null`、`revision = null`とする。
不正な要求URIはsummaryへ記録しない。
resource ACL非許可であることを示す詳細はsummaryへ記録しない。

### 9.3 共通項目と記録禁止情報

両操作とも、ユーザID、token ID、取得可能な入力元IP address、
timestampを既存共通項目として記録する。

監査書込みはbest-effortとし、sink lock、書込み、ユーザID解決の失敗によって
resources操作の成功・失敗結果を変更しない。

次の情報は監査ログへ記録しない。

- resource本文
- front matter本文
- ページpath、page ID
- cursor値、resource URI一覧、resource名一覧
- resource ACL非許可であることを示す詳細
- DB内部エラー
- Bearer token平文、Authorization header
- serialize済みrequest body

## 10. MCP アセット Toolsの監査設計

MCP アセット Toolsは、ページに紐付く通常アセットを扱う
pathベースtoolとして監査する。
`AuditOperation`の永続化名はtool名と同じsnake_caseとする。

対象operationは以下とする。

- `AuditOperation::ListPageAssets`: `list_page_assets`
- `AuditOperation::ResolvePageAsset`: `resolve_page_asset`
- `AuditOperation::GetPageAssetMetadata`: `get_page_asset_metadata`
- `AuditOperation::GetPageAssetData`: `get_page_asset_data`
- `AuditOperation::UploadPageAsset`: `upload_page_asset`
- `AuditOperation::DeletePageAsset`: `delete_page_asset`

### 10.1 参照系アセット tool

`list_page_assets`、`resolve_page_asset`、`get_page_asset_metadata`、
`get_page_asset_data` は参照系として扱う。

成功時は次の内容を記録する。

- `result = success`
- `target_path = 正規化済み page path`
- `revision = null`
- `list_page_assets` の summary は `count=<返却件数>` とする
- `resolve_page_asset`、`get_page_asset_metadata`、`get_page_asset_data` の summary には、`asset_id`、`file_name`、`mime_type`、`size` を記録してよい

失敗時は次の結果分類を使用する。

- scope不足: `scope_denied`
- path prefix制約違反: `path_prefix_denied`
- page / asset 不存在、削除済み asset、外部到達不能 asset: `not_found`
- 入力不正: `invalid_input`
- 保存済み asset 10MiB 超過、事前検出できる result / transport payload 上限超過: `conflict`
- DB / FS / base64 encode などの想定外失敗: `internal_error`

失敗時 summary は公開message相当の固定的・抽象的文言に限定する。

### 10.2 更新系アセット tool

`upload_page_asset` と `delete_page_asset` は更新系として扱う。

成功時は次の内容を記録する。

- `result = success`
- `target_path = 正規化済み page path`
- `revision = null`
- `upload_page_asset` の summary には、`asset_id`、`file_name`、`mime_type`、`size`、`summary=asset uploaded` を記録してよい
- `delete_page_asset` の summary には、`asset_id`、`file_name`、`summary=asset deleted` を記録してよい

失敗時は次の結果分類を使用する。

- scope不足: `scope_denied`
- path prefix制約違反: `path_prefix_denied`
- `ReadOnly` ユーザによる upload / delete: `scope_denied`
- page / asset 不存在、削除済み asset、外部到達不能 asset: `not_found`
- 同名アセット重複、ロック中ページ: `conflict`
- path / file_name / mime_type / data_base64 不正、upload size 制約超過: `invalid_input`
- 初期版対象外操作: `unsupported`
- DB / FS などの想定外失敗: `internal_error`

失敗時 summary は公開message相当の固定的・抽象的文言に限定する。
ロック中ページへの upload / delete は lock token 入力を受け付けないため、
`conflict` として即時監査対象にする。

### 10.3 共通項目と記録禁止情報

全アセット tool とも、ユーザID、token ID、取得可能な入力元IP address、
timestampを既存共通項目として記録する。
監査書込みはbest-effortとし、sink lock、書込み、ユーザID解決の失敗によって
アセット操作の成功・失敗結果を変更しない。

アセット操作はappend集約対象外とし、成功・失敗とも原則即時書き込みとする。

次の情報は監査ログ、tracing、error summaryへ記録しない。

- `data_base64`
- アセット本体 bytes
- serialize済みrequest body
- Bearer token平文、Authorization header
- lock token
- 内部保存パス
- DB内部エラー詳細
- raw `data_base64` を含む upload 入力構造全体
- 削除済み、ゾンビ、所属不一致として除外した asset の一覧や file_name

## 11. REST監査拡張

> 2026-10-13: MS4でREST監査と共通起動・保持・正常停止を実装済み。
> MS4の実検証とCP4承認を経て、MS5グループ2で両認証のR32実記録、
> グループ3で関係回帰・新実体の両mode正常停止を確認した。
> 条件・再利用・制約は正式記録を参照し、最終SC評価・CP5とは区別する。

実装・結果の対応は[検証観点・証拠参照](REST_API_AUTHORIZATION_TEST_VIEWPOINTS.md)を参照する。

### 11.1 モデル拡張と操作主体

既存JSONLのトップレベル項目を維持し、新しいログfieldは追加しない。RESTは公開HTTP操作ごとに記録し、既存MCPは既存operation・result・append集約の意味を維持する。

| 項目 | RESTの設計 |
|---|---|
| operation | 既存AuditOperationへRest(RestAuditOperation)を追加。RestAuditOperationは登録済み30組のmethod+route templateを列挙する閉じたenum。例GET /api/pages/{page_id}/source。Serialize/Deserializeを固定文字列として扱い、JSON object/tag構造にしない。既存MCP snake_case文字列の読み書きは同じまま |
| user_id | 認証時に検証成功したUserInfo.idを認証補助情報へ保持し、要求途中の別lookupでユーザ削除/renameにより主体を失わない。AuthUser.user_id()が現行ではユーザ名であることに注意し、JSONLへユーザ名をIDとして入れない |
| token_id | Bearerは既存token_id、Basicは明示null。token平文/token nameをsummaryへ追加しない |
| address | 既存接続情報で得たIpAddr、欠損時null。生ヘッダをそのまま扱わない |
| target_path | 同一txn/snapshotで認可した正規化Wiki path。hello/me・解決前の入力失敗・不在ID・zombieはnull。rename/restore成功は元path、拒否は失敗側path。親候補非認可は拒否した候補path |
| revision | 実際に取得したsource/meta rev、source PUTやrenameのcommitted revision、restoreの保持latest、rollback/compactionの操作指定rev。一覧/検索/asset/lock/draft作成等はnull。エラーでは確定している実対象/commit済みrevだけを残し、未解決raw revを記録しない |
| result | 最終の公開操作結果に基づく型付き分類。2xx/302/304はSuccess。scope/path/ReadOnlyは個別deny。不在はNotFound、入力400/413/415等はInvalidInput、409はConflict、5xxはInternalError。RESTの410/423/一般403を表すGone/Locked/OperationDeniedだけを追加し、既存列挙の名前は変更しない |
| timestamp | 境界で最終応答が定まった時点のUTC |
| summary | 下表と共通補足規則に従いサーバ生成。補足がない成功はnull。raw URL/query/body、token/lock資格情報、FS内部pathは含めない |

AuthContextのscope/path/attributeの意味は変えない。内部の認証主体ID補足はBasicのUserInfo取得（src/rest_api/auth.rs:388–407）とBearer検証のUserInfo（src/auth.rs:266–292）から設定する。既存テスト用constructorやMCPのユーザ名accessorを破壊せず、実認証では安定IDを保持するconstructor/accessorを追加する。既存MCPの監査user再解決は必要な範囲で補助ID利用へ接続できるが、MCP operation/summaryを変更しない。

MS4拡張前のAuditOperationはenumのsnake_case serdeを用いていたため、
Rest variant追加をderiveへ任せるとoperation値が構造化される。
MS4では固定文字列の専用変換を実装し、既存MCP JSONLの逆直列化と
REST固定文字列round-tripを検証した。
既存AuditResultのReadOnlyDeniedと、MCPのScopeDeniedへの正規化を維持する。

### 11.2 全30操作/32アクションのsummary・項目表

表のNo.はREST_API_AUTHORIZATION_TEST_VIEWPOINTS.mdの32行に対応する。method/route templateは固定定義であり、実URLのページID/asset ID/file名をoperationへ埋めない。rename/restoreおよびrollback/compactionは同じoperation、summary.actionだけで区別する。

記号: `P`=同一txnで確定した認可対象path、`V`=実対象として確定したrevision、`n`=実際に返した候補件数（非負整数）、`b`=確定したtrue/false、`D`=最終配置先（正規化・認可で用いた値）。変数pathは空白/controlなしの正規形を用い、raw入力を転記しない。未確定値は該当補足を省略し、nullの項目に文字列nullを埋めない。

共通失敗補足`E`は`failure=<固定分類>`。分類はresultに対応するscope_denied/path_prefix_denied/read_only_denied/not_found/conflict/gone/locked/operation_denied/invalid_input/internal_error。handlerが確定したactionは失敗時も付加できる。両指定/指定なし等でactionが未確定ならactionを推測せずEのみ。path拒否のrename/restoreだけ`denied_side=source|destination`を付加。親候補非認可は`denied_side=parent`。入力量超過を境界だけで検知した場合は`failure=payload_too_large`という固定補足も使用でき、resultはInvalidInput。

| No. | operation・分岐 | 成功summary | target_path / revision成功 | 失敗summary・欠損条件 |
|---|---|---|---|---|
| 1 | GET /api/hello | null | null / null | E、常にpath/rev null |
| 2 | GET /api/users/me | null | null / null | E、常にpath/rev null |
| 3 | POST /api/pages | null | 作成P / null | E、入力解決後Pのみ、rev null |
| 4 | GET /api/pages | count=n has_more=b | 要求prefix P / null | E、prefix解決前null。cursorは記録しない |
| 5 | GET /api/pages/deleted | count=n | 指定P / null | E、path解決前null |
| 6 | GET /api/pages/id | null | 指定/対象P / null | E、要求path解決済みならP |
| 7 | GET /api/pages/short | null | 指定/対象P / null | E、要求path解決済みならP |
| 8 | GET /api/pages/template | count=n | null / null | E、候補path群・ID群を記録しない |
| 9 | GET /api/pages/search | count=n | null / null | E、query/snippet/結果path群を記録しない |
| 10 | GET /api/pages/{page_id}/source | null（304も同じ） | P / 取得V | E、解決済みP/Vのみ。ETagは記録しない |
| 11 | PUT /api/pages/{page_id}/source | amend=b | P / committed V | E＋確定amend。後段失敗はcommit補足。解析前amend省略 |
| 12 | GET /api/pages/{page_id}/meta | null | P / source V、draftはrev null | E、取得対象確定後P/Vのみ。link_refsをsummaryへ複製しない |
| 13 | GET /api/pages/{page_id}/parent | null | 指定ページP / null | E、親非認可はdenied_side=parent・target=拒否親P、HTTP404。ID不在null |
| 14 | GET /api/pages/{page_id}/short | null | P / null | E、ID不在null |
| 15 | GET /api/pages/{page_id}/path | null | P / null | E、ID不在null |
| 16 | POST /api/pages/{page_id}/path action=rename | action=rename destination=D | 元P / committed V | E＋action=rename、path拒否はdenied_side。D確定かつ変更済み後段失敗のみD補足可 |
| 17 | POST /api/pages/{page_id}/path action=restore | action=restore destination=D | 削除時P / 復元latest V | E＋action=restore、path拒否はdenied_side。後段失敗の扱いはrename同様 |
| 18 | POST /api/pages/{page_id}/revision action=rollback | action=rollback | P / 確定rollback_to V | E＋action=rollback、指定対象未確定revはnull、後段失敗commit補足 |
| 19 | POST /api/pages/{page_id}/revision action=compaction | action=compaction | P / 確定keep_from V | E＋action=compaction、指定対象未確定revはnull、後段失敗commit補足 |
| 20 | GET /api/pages/{page_id}/assets | count=n | owner P / null | E、page解決前null |
| 21 | POST /api/pages/{page_id}/assets/{file_name} | null | owner P / null | E、extractor/owner未解決はnull。file名/bytes記録なし |
| 22 | GET /api/pages/{page_id}/assets/{file_name} | null（302も同じ） | owner P / null | E、owner未解決null。Location/asset ID記録なし |
| 23 | POST /api/pages/{page_id}/lock | null | P / null | E、index未解決null。lock tokenは成功でも記録しない |
| 24 | PUT /api/pages/{page_id}/lock | null | P / null | E、index未解決null。lock token非記録 |
| 25 | GET /api/pages/{page_id}/lock | null | P / null | E、index未解決null。lock状態/資格情報非記録 |
| 26 | DELETE /api/pages/{page_id}/lock | null | P / null | E、index未解決null。draft削除を含め公開要求一件 |
| 27 | DELETE /api/pages/{page_id} | recursive=b | P / null | E＋確定recursive、後段失敗commit補足。未解析recursive省略 |
| 28 | POST /api/assets | null | 指定/owner P / null | E、path/owner未解決null、file/bytes記録なし |
| 29 | GET /api/assets | null（302も同じ） | 指定/owner P / null | E、path/owner未解決null、Location非記録 |
| 30 | GET /api/assets/{asset_id}/data | null（304も同じ） | owner P / null | E、ID/owner未解決・zombie null。本体/ETag非記録 |
| 31 | GET /api/assets/{asset_id}/meta | null | owner P / null | E、ID/owner未解決・zombie null |
| 32 | DELETE /api/assets/{asset_id} | null | owner P / null | E、ID/owner未解決・zombie null。既存deletedかzombieかをraw ID等で補足しない |

無操作rename204は成功として記録するが、新revisionがなければそのtxnで確定した現在latestを記録。複数ページ再帰のrevision群は追加しない。RESTはappend集約対象とせず一要求一投入。結果件数やhas_moreは返したレスポンス生成値から得て、別DB参照で再計算しない。

#### 更新後の失敗補足

DB commit後FTS等が失敗するとHTTP500でも変更が残り得る（静的調査で確認した現状: src/rest_api/pages/source.rs:503–589）。最終result=InternalErrorを維持し、handlerが確定した更新metadataから`phase=post_commit committed=true`をEに付加する。rename/restoreでは確定action/D、sourceではcommitted V/P、revision操作では確定指定Vを保持する。phaseはfixed enumのpost_commit、committedはbooleanだけで、例外文字列や内部FS pathは転記しない。

commit前拒否/失敗でDB未変更が確定した場合のみ`phase=pre_commit committed=false`を使用可能。metadataなしのextractor失敗や不明な内部失敗でcommitted=falseを推測しない。uploadの登録/commit失敗や新規file回収失敗は業務DB/FSの失敗としてHTTP500/内部失敗を維持する。監査sink障害だけで既存業務応答を変えない原則と混同しない。新規fileのcleanup失敗などDB commitのないFS残存はInternalErrorとして固定`phase=cleanup`を補足できるが、`committed=false`はDB状態の意味とし「一切副作用なし」の証拠にしない。DB更新入口の戻り値/後処理区間でmetadataを早期に確定し、500後に現在DBを読み直して推測しない。

### 11.3 認証後request境界とhandler補足

認証済みの定義済み公開REST操作にだけ一記録する。登録時にfixed route descriptorをresourceへ渡し、resource middlewareでHTTP method→固定30operationを選ぶ。動的pathからoperationを組み立てない。既存`.route`登録を必要最小限の`.service(web::resource(...).wrap(...))`へまとめ、同じresourceのGET/POST等のoperation表を明示する。scopeの認証middleware外側／resource監査内側という順にし、認証失敗は監査境界へ入らない。AuthContextと認証済みactor IDはresource boundaryに到達時すでに得られる。

| 境界段階 | 処理と保証 |
|---|---|
| resourceのhandler/extractor以前 | fixed operationを選択、actor/token/addressを複製し、空のRestAuditMetadataをrequest extensionsへ格納。query/body/ID等を監査のために先読みしない |
| handler/共通guard | metadataへ型付きaction、result、最終target、revision、返却count、commit phase等を補足するだけ。監査sinkへ直接投入しない。borrow/lockをawaitを跨いで保持しない |
| nextがServiceResponseを返した | final HTTP statusとtyped metadataを統合し一記録。302/304/早期400/403/404/410/409/423/500を含める |
| nextがErrを返した | ErrorのResponseErrorが生成するstatus/responseを同じ境界で受け取り、そのstatusでfallback分類し一記録。そのresponseを返し、handler完了扱いにしない |
| metadataなし/不足 | statusから上表の分類を使用、target/revision/action/countは不明ならnull/省略。403はOperationDeniedで、scope/path/ReadOnlyと推測しない。400/413/415はInvalidInput、5xxはInternalError。raw error本文やreq URLをsummaryへコピーしない |
| 固定operationなし | 未登録route/未対応method/認証失敗は従来HTTPログ対象。今回の監査対象を未知HTTP操作へ勝手に拡張するoperationを作らない |

resource boundaryはweb::Bytes extractorより外側なので、payload limit/取得失敗でhandlerが実行されなくても記録できる。scope guardがhandler入口で拒否する際はtyped deny resultをmetadataへ補足する。path guard/DB policyもfailed pathとsideを補足する。親候補非認可はHTTP404に対してPathPrefixDeniedを明示し、status fallbackのNotFoundで上書きしない。

メタデータはrequest-local共有の最小struct（固定enum/正規化path/整数/bool/Option）とし、公開レスポンスには露出させない。HTTP statusが500なら直前成功metadataのSuccessをInternalErrorへ更新し、commit事実だけを残す。retention/writerの診断で公開操作レコードを再帰的に生成しない。POST uploadのbodyはextractor以外が重複取得せず、response bodyを監査のために読取/再解析しない。

一要求一記録は、一つのresource boundaryから一度投入を試みる意味であり、best-effort writer障害時の確実な一回永続化を保証するものではない。ネットワーク送信後の切断・プロセス強制killを正常な公開操作完了として監査する新仕様は設けない。DB/response metadataを確定して同期lock/txnを閉じてからsinkへ投入し、DB lockとaudit lockの逆順deadlockを避ける。

### 11.4 共通監査基盤の起動・運用・停止

| 契機 | 接続と失敗方針 |
|---|---|
| config受渡し | RunCommandContextの既存AuditLogConfigをREST運用でも常にhttp_serverへ渡す。MCPフラグでNoneにしない（src/command/run.rs:221–225）。新しい設定項目や既定出力先を増やさない |
| 起動 | MCP endpointの有無からsink生成を切り離す（src/http_server/mod.rs:187–195）。既存config/出力dir準備・起動retention・writer/sinkを一度組み立て、AppStateからREST/MCPで同じArcを利用。dir/config依存失敗は起動失敗として返す |
| request投入 | existing AuditSink.recordで同期直列投入。RESTは即時書込経路、MCP appendだけ既存集約。record/lock障害は固定warningを出し業務HTTP結果を変更しない。再試行で重複投入する仕組みは追加しない |
| rotation | 既存writerのsize判定・rotate_if_neededを再利用（src/audit/writer.rs:105–136）。REST独自fileへ分けず同じJSONL writer。MCPのbuffer/rotation責務を監査層で保持する |
| retention | 起動時に一回、稼働中は固定の粗い周期（設計値60秒、非公開内部定数）で既存build_retention_plan/execute_retention_planを呼ぶ。アクティブfile除外、規則外file温存、個別削除失敗はwarning/継続。周期実行のdirアクセス失敗も診断して次周期へ、業務応答を変えない |
| retention同期 | request投入/rotationと同じsinkのwrite lockでwriterのactive状態と保持掃除を直列化。sink上の小さなmaintenance入口から既存retentionへ接続。lock中にawaitしない。ファイル列挙/削除をhandlerで行わない |
| 正常停止 | server.await終了後、retentionタスクへ停止通知して完了を待ち、稼働requestが終了した状態でAuditSink.flushを呼ぶ。bufferの保留append→writer最終flushを順序どおり必ず実行。途中でserverがErrでもflushを試み、元error/flush errorを診断して終了Resultへ保持 |
| flush失敗 | request投入best-effortと区別。終了flushの失敗を無視して正常完了にせず、runの終了Resultへ内部失敗として伝播する。無制限retry/二重appendは追加しない。既存設計の「明示停止手順・非best-effort」を満たす（docs/MCP_AUDIT_LOG_DESIGN.md:364,394,758–768） |

HTTPサーバのstateはlock cleanup taskへ移す前にcloneし、停止後sinkの参照を保持する。保守taskの停止を待つ際はAppState/sinkのguardを持たない。lock cleanupも停止終了を整理し、終了flush後に新しい公開監査が投入される順序を作らない。Unix/Windows/ShutdownSignalなど既存正常終了を同じserver.await後の終了シーケンスへ接続する。強制kill・OS障害時のflushまでは保証しない。

現行writerはBufWriterなのでrecord成功とディスクdurable化を同一視しない。
今回新しいfsync保証・キュー・監査用DB・外部workerは導入しない。
停止flushは既存group内部へ確定snapshotを保持し、非破壊で取得する。
write_record成功keyだけを退役し、失敗/未試行groupを保持する。
append write失敗でもwriter最終flushを必ず試行する。
部分writeがあり得る停止sequenceの再flushでsnapshotを自動再投入せず、
最終writer.flush失敗でも成功groupをbufferへ戻さない。
通常record/record_append入口を永久停止させる変更はしない。
保持は永続復旧・一回永続化の保証ではなく、終了Resultは失敗を維持する。

HTTP終了後はretention/lock cleanupを停止してjoinし、
同期MCP業務とrecordが保持するAppState read guardをwrite barrierで待つ。
停止gateによりqueued業務を監査追加なしで拒否した後、
既存MCP sweep停止とsession closeをguard外で行って最終flushへ進む。
SDKの既終了actorは停止完了として扱い、それ以外の終了失敗を保持する。
この順序で最終flush後の公開監査投入を防ぐ。
