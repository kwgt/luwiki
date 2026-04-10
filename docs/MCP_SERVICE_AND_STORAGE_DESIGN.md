# MCPサービス・永続化設計

本書は、MCP内部設計のうち、
path ベースサービス層、永続化モデル拡張、DB API、更新系操作の共通化を整理するための文書である。

本書は、共通部である `docs/MCP_INTERNAL_DESIGN.md` を前提とし、
現行 `docs/MCP_INTERNAL_DESIGN.md` の以下の章を移設する受け皿として用いる。

- 9. path ベースサービス層の橋渡し設計
- 10. 更新系操作の共通化単位

関連する設計文書は以下の通り。

- `docs/MCP_ARCHITECTURE_DESIGN.md`
  - 責務配置、`src/mcp/` 構成、既存 REST API 層との共通化方針を確認する場合に参照する
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - `run` コマンド、設定値伝播、HTTPサーバ初期化経路を確認する場合に参照する
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 公開ツールごとの入力契約、出力モデル、エラー写像を確認する場合に参照する
- `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 監査ログ設定値、監査レコード、`append` 集約の詳細を確認する場合に参照する
- `docs/MCP_AUTHORIZATION_TEST_VIEWPOINTS.md`
  - path ベース認可および required scope のテスト観点を確認する場合に参照する
- `docs/MCP_APPEND_TEST_VIEWPOINTS.md`
  - `append` の amend 判定、競合制御、待機挙動のテスト観点を確認する場合に参照する

---

## 1. 対象範囲

本書では以下を対象とする。

- path 解決と認可の橋渡し
- Bearer トークン管理情報の拡張設計
- ユーザ属性モデルの拡張設計
- サービス層の入出力モデル
- 追加が必要な DB API
- 更新系操作の共通化単位

## 2. path ベースサービス層の橋渡し設計

本章では、path ベース操作を内部処理へ橋渡しするサービス層の設計として、
外部から見える path ベース要求を、既存の `DatabaseManager` が持つ
page_id ベース操作や prefix ベース操作へ安全に変換する流れを定義する。

### 2.1 基本方針

サービス層は、MCP 公開層から受け取った path ベース入力に対し、
次の順で処理を進める。

1. path の妥当性検証
2. path 正規化
3. 操作種別に応じた認可判定対象 path の決定
4. path prefix 制約を含む認可判定
5. 必要に応じた path から page_id への解決
6. 既存 DB API または FTS の呼び出し
7. MCP 応答用の結果モデルへ整形

この順序を統一することで、
MCP 公開層は path ベースの入力処理に専念でき、
DB 層は従来どおり page_id や prefix を受け取る低水準 API として維持できる。

### 2.2 path 解決の共通処理

サービス層には、少なくとも以下の共通処理を持たせる。

- `validate_and_normalize_path`
  - path 妥当性検証と正規化
- `resolve_page_by_path`
  - 現在 path から `page_id` とページ状態を解決
- `resolve_prefix_request`
  - list / search 用の prefix 入力検証と認可判定
- `ensure_authorized_path`
  - 操作種別と対象 path に応じた path prefix 制約判定

`resolve_page_by_path` は、
現時点の `DatabaseManager` に path から現在ページを解決する専用 API が不足しているため、
後続で DB 側へ補助 API を追加する前提とする。

#### 2.2.1 現在 path 解決の共通入口

`resolve_page_by_path` は、MCP の read / get_section / update / append / rename に共通で使う
「現在 path からページ実体を解決する入口」として定義する。

この入口は、単に `page_id` を引くだけでなく、
MCP 側で後続処理に必要となる現在状態をまとめて返す責務を持つ。

概念上の入出力は以下を基本とする。

```rust
struct ResolvedPage {
    normalized_path: NormalizedPath,
    page_id: PageId,
    page_index: PageIndex,
    latest_revision: Option<u64>,
}

enum ResolvePageFailure {
    InvalidPath,
    NotFound,
    Deleted,
    Draft,
    InternalError,
}

fn resolve_page_by_path(
    db: &DatabaseManager,
    raw_path: &str,
) -> Result<ResolvedPage, ResolvePageFailure>;
```

ここでの責務分担は以下とする。

- 入力は必ず外部から受け取った raw path とする
- 入口内部で `validate_and_normalize_path` を先に適用し、未正規化 path を DB 解決へ渡さない
- 現在 path として `PAGE_PATH_TABLE` 上に存在するページだけを解決対象とする
- 削除済みページテーブルは参照しない
- path 解決後は `PageIndex` を取得し、後続操作に必要な状態をまとめて返す
- 認可判定はこの入口の責務に含めず、呼び出し側で `ResolvedPage.normalized_path` を使って行う

失敗分類は以下の方針とする。

- `InvalidPath`
  - path 妥当性検証または正規化に失敗した場合
- `NotFound`
  - `PAGE_PATH_TABLE` 上で現在 path として解決できない場合
  - rename 後の旧 path 指定もここへ含める
- `Deleted`
  - 初期実装では current path からは通常到達しない想定だが、
    DB 不整合や将来拡張に備えた防御的分類として保持する
- `Draft`
  - path は引けたが、解決先 `PageIndex` が draft の場合
- `InternalError`
  - DB 読み取り失敗や index 不整合など

`Deleted` と `Draft` を `NotFound` へ潰さず区別する理由は、
MCP の対象外機能拒否や更新系失敗理由の整形、監査ログ `result` / `summary` の粒度を保つためである。

`ResolvedPage` が返す項目の用途は以下とする。

- `normalized_path`
  - 認可判定、監査ログ、後続応答整形の共通基準 path
- `page_id`
  - 既存 DB API 呼び出しのための内部識別子
- `page_index`
  - deleted / draft / current path / latest revision の判定材料
- `latest_revision`
  - 通常ページでは `Some(latest)`、draft では `None`
  - 後続の source 取得、update / append / rename 成功応答、監査ログに利用する

処理順序は以下で固定する。

1. raw path の妥当性検証と正規化
2. 正規化済み path から `get_page_id_by_path` 相当で `page_id` を解決
3. `get_page_index_by_id` で `PageIndex` を取得
4. `PageIndex` の状態整合を検証する
5. `ResolvedPage` を構築して返す

状態整合の検証規則は以下とする。

- `get_page_id_by_path` が `None` の場合は `NotFound`
- `get_page_index_by_id` が `None` の場合は `InternalError`
- `page_index.current_path()` が `None` の場合は `Deleted`
- `page_index.current_path()` が返した path が入力の正規化済み path と一致しない場合は `InternalError`
- `page_index.is_draft()` が `true` の場合は `Draft`

この入口により、MCP サービス層は「path 文字列の解決」と
「解決後状態の整合確認」を毎回書き直さずに済む。
また、認可前後のどちらで `page_id` 解決を行ったかを操作ごとに明示しつつ、
解決そのものの責務は 1 箇所へ集約できる。

#### 2.2.2 既存 DB API との接続方針

初期段階では、`resolve_page_by_path` の内部実装は
既存の `get_page_id_by_path` と `get_page_index_by_id` の組み合わせで成立する。

一方で、MCP 用共通入口としては以下の補助 API を
DB 層へ追加できる形で設計しておく。

- `get_current_page_entry_by_path`
  - 入力: 正規化済み path
  - 出力: `Option<(PageId, PageIndex)>`
- または `get_page_index_entry_by_path`
  - 入力: 正規化済み path
  - 出力: `Option<PageIndexEntryLike>`

追加 API の責務は、path 解決と index 取得を 1 トランザクションで行い、
MCP サービス層が DB 呼び出し回数や不整合処理を意識しすぎないようにする点にある。

ただし 4.6.1 の時点では、公開設計上の共通入口を `resolve_page_by_path` として固定し、
DB 層の最終 API 形は 4.7.3 で詳細化する。

#### 2.2.3 list / search 用 prefix 要求の共通入口

`list` と `search` では、
単一ページを `page_id` へ解決するのではなく、
「要求 prefix 自体が許可されるか」と
「結果として得られた各 current path が許可範囲内か」を
分けて扱う必要がある。

このため、単一ページ解決とは別に `resolve_prefix_request` を
共通入口として定義する。

概念上の入出力は以下を基本とする。

```rust
struct ResolvedPrefixRequest {
    requested_prefix: Option<NormalizedPath>,
    filter_mode: PrefixFilterMode,
}

enum PrefixFilterMode {
    NoPrefix,
    DescendantsOf(NormalizedPath),
}

enum ResolvePrefixFailure {
    InvalidPrefix,
    PrefixDenied,
    InternalError,
}

fn resolve_prefix_request(
    auth: &AuthContext,
    raw_prefix: Option<&str>,
) -> Result<ResolvedPrefixRequest, ResolvePrefixFailure>;
```

ここでの責務分担は以下とする。

- `raw_prefix` が指定された場合のみ path 妥当性検証と正規化を行う
- 正規化済み prefix が得られた場合は、その prefix 自体に対して `read` と path prefix 制約を判定する
- prefix 指定がない場合は、事前の path 制約判定を行わず、結果後段フィルタだけを有効化する
- 戻り値には「要求 prefix」と「結果フィルタ方式」を明示的に保持する

失敗分類は以下とする。

- `InvalidPrefix`
  - prefix 入力が path として不正な場合
- `PrefixDenied`
  - prefix 自体は正規だが、Bearer の path prefix 制約で許可されない場合
- `InternalError`
  - 認証文脈の不整合や内部補助処理失敗など

`list` / `search` において prefix 指定を受けた場合の処理順序は以下とする。

1. prefix 入力の妥当性検証と正規化
2. 要求 prefix 自体に対する `read` と path prefix 制約判定
3. 一覧取得または検索実行
4. 各結果の current path に対する後段フィルタ

prefix 未指定時の処理順序は以下とする。

1. 事前 prefix 判定なし
2. 一覧取得または検索実行
3. 各結果の current path に対する後段フィルタ

この「要求 prefix 判定」と「結果 path 後段フィルタ」は、
片方で他方を代替しない。

- 要求 prefix 判定
  - クライアントが「どの範囲を見ようとしているか」を認可対象として扱う
- 結果 path 後段フィルタ
  - prefix 未指定時や検索結果混在時でも、許可範囲外の項目を返さないために使う

後段フィルタの判定対象は、常に結果項目の current path とする。
削除済み path や旧 path、要求時の raw prefix 文字列をそのまま比較対象にはしない。

#### 2.2.4 list の prefix 検証と後段フィルタ方針

`list` は path 階層を前提とするため、
要求 prefix を DB 走査条件として直接利用しやすい。
ただし、認可上は以下の二段階を必須とする。

- 要求 prefix がある場合
  - `resolve_prefix_request` で prefix 自体の許可判定を行う
  - `list_page_entries_by_prefix` の呼び出し引数には、正規化済み prefix を使う
  - 取得後、各 `PageListEntry.path()` を current path として再度 path prefix 制約で後段フィルタする
- 要求 prefix がない場合
  - `list` 用の既定 prefix は `/` として一覧取得してよい
  - ただし `/` を暗黙指定したものとして事前許可した扱いにはしない
  - 取得後の全件に対し、current path ベースの後段フィルタを必ず適用する

prefix 指定なし時に後段フィルタだけでよいとする理由は、
トークンが許可している jail を跨いで複数 prefix が存在しうるためである。
この場合、事前に単一 prefix を認可対象として固定すると、
全許可範囲を自然に列挙できない。

`list` の結果後段フィルタは以下の規則とする。

- 判定対象は `PageListEntry.path()` のみ
- path prefix 制約を満たす項目だけを残す
- フィルタ後に件数 0 でも正常結果とする
- 要求 prefix 自体が不許可だった場合は空結果へ丸めず `PrefixDenied` とする

#### 2.2.5 search の prefix 検証と後段フィルタ方針

`search` は FTS の都合で、
要求 prefix があっても結果集合に他 path が混ざりうる実装を取りやすい。
そのため、`list` 以上に後段フィルタを重視する。

`search` の設計方針は以下とする。

- 要求 prefix がある場合
  - `resolve_prefix_request` で prefix 自体の許可判定を行う
  - FTS 実行時に prefix 条件を渡せるなら絞り込みに利用してよい
  - ただし FTS 側絞り込みを認可の代替として扱わない
  - 検索結果の `page_id` から current path を解決した後、各 path に対して後段フィルタする
- 要求 prefix がない場合
  - 全体検索を実行してよい
  - ただし結果返却前に、各 current path に対する後段フィルタを必ず適用する

`search` における後段フィルタの判定順序は以下とする。

1. FTS 結果から `page_id` と revision を受け取る
2. `page_id` から current path を解決する
3. current path が path prefix 制約を満たすか判定する
4. 必要なら要求 prefix 配下かも追加判定する
5. 条件を満たした項目だけを応答へ残す

ここで 4 を追加する理由は、
FTS 側の prefix 絞り込みが厳密でない場合でも、
要求 prefix の意味論を応答面で保つためである。

`search` の結果後段フィルタは以下の両条件を満たす場合のみ通過とする。

- Bearer の path prefix 制約を満たす
- 要求 prefix がある場合は、その prefix と完全一致または配下 path である

フィルタ後に結果 0 件となった場合は正常結果とし、
要求 prefix が不許可である場合だけを `PrefixDenied` として明確に失敗させる。

#### 2.2.6 create / update / rename の path 正規化規則

更新系のうち `create` / `update` / `rename` は、
いずれも path を入力に取るが、検査対象と失敗条件が異なる。
この差を吸収するため、path の取り扱いは
「入力 path の正規化規則」と「操作別追加検査」に分けて定義する。

共通の正規化規則は以下を基本とする。

- raw path は必ず `validate_and_normalize_path` を通す
- 正規化前の生文字列で衝突判定や認可判定を行わない
- path は絶対 path のみ許可する
- 既存 `rest_api::pages::validate_page_path` が持つ禁止文字規則を継承する
- trailing slash は path 同値性を曖昧にしないよう、root を除き除去した正規形へ揃える
- `.` / `..` のような経路解釈を持ち込まない
- 空文字、相対 path、禁止文字を含む path は `InvalidPath` とする

`NormalizedPath` は比較・認可・監査ログ・DB 問い合わせで共通利用する単位とし、
少なくとも以下を満たす値として扱う。

- `/` から始まる
- root 以外で末尾 `/` を持たない
- path 境界比較にそのまま利用できる

操作別の入力モデルは以下を基本とする。

```rust
struct CreatePathInput {
    target_path: NormalizedPath,
}

struct UpdatePathInput {
    current_path: NormalizedPath,
}

struct RenamePathInput {
    current_path: NormalizedPath,
    rename_to: NormalizedPath,
}
```

共通化の原則は以下とする。

- `create` は存在しないはずの target path を扱う
- `update` は current path として既存ページを解決できることが前提となる
- `rename` は current path と rename 先 path の 2 点を同時に扱う
- 認可判定は、`create` は target path、`update` は current path、`rename` は current path と rename_to の双方を対象とする

#### 2.2.7 create の path 検証規則

`create` の path 検証は以下の順で行う。

1. raw target path の妥当性検証
2. target path の正規化
3. target path に対する `create` と path prefix 制約判定
4. 現在 path テーブル上での衝突確認
5. 必要なら対象外状態の確認

`create` の衝突確認規則は以下とする。

- `PAGE_PATH_TABLE` に同一 current path が存在する場合は `PageAlreadyExists`
- 削除済み path テーブルとの衝突は、初期 MCP では restore を提供しないため、
  create の禁止理由には含めない
- したがって、現在 path に現存しない限り create 自体は候補となる

`create` の追加規則は以下とする。

- root path `/` の新規作成は許可しない
- 既存仕様上のページ作成単位に合わせ、通常ページ作成として扱う
- 親 path の存在有無は 4.6.3 の必須制約には含めず、既存 DB/API の振る舞いに合わせる

#### 2.2.8 update の path 検証規則

`update` の path 検証は以下の順で行う。

1. raw current path の妥当性検証
2. current path の正規化
3. `resolve_page_by_path` による現在ページ解決
4. 解決済み current path に対する `update` と path prefix 制約判定
5. 後続の source 更新処理

`update` では、入力 path に対する存在確認と状態確認を
`resolve_page_by_path` へ委譲する。
このため、path 検証段階で扱う主な失敗は以下となる。

- path 自体が不正
- 現在 path として存在しない
- draft を指している
- deleted 状態へ不整合に到達した

`update` では path 衝突判定は不要である。
更新対象は current path として既に一意解決済みであり、
path 自体を書き換えないためである。

#### 2.2.9 rename の path 検証規則

`rename` は更新系の中で最も検査項目が多いため、
current path 側と rename_to 側を分けて扱う。

検証順序は以下で固定する。

1. raw current path の妥当性検証と正規化
2. raw rename_to path の妥当性検証と正規化
3. current path を `resolve_page_by_path` で解決
4. 解決済み current path に対する `update` と path prefix 制約判定
5. rename_to path に対する `update` と path prefix 制約判定
6. rename_to の衝突確認
7. rename 元先の追加整合検査

rename の衝突確認規則は以下とする。

- `PAGE_PATH_TABLE` に rename_to と同一 current path が存在する場合は `PageAlreadyExists`
- 再帰 rename の場合は、移動対象集合の内部に含まれる path との重複だけは許容する
- ただし移動対象外ページとの衝突は常に禁止する

rename の追加整合検査は以下とする。

- root page を移動元にしてはならない
- current path と rename_to が正規化後に完全一致する場合は no-op として成功扱いしてよい
- rename_to が current path 配下になる移動は `InvalidMoveDestination` とする
- current path と rename_to の双方が path jail 内である場合のみ許可する
- jail 内から jail 外、jail 外から jail 内への移動は `PathPrefixDenied` とする

rename における `rename_to` の解釈は以下とする。

- MCP 公開面では `rename_to` は最終到達 path を明示する引数として扱う
- 末尾 `/` を付けた曖昧な「親ディレクトリ指定」としては扱わない
- 既存 DB 実装が持つ suffix 補完ロジックは再利用候補だが、
  MCP サービス層では正規化済み最終 path を確定してから DB 層へ渡す

この方針により、MCP の rename は
「どこへ移動するか」を path 1 本で明確に表せる。
既存 REST API / DB 層に残る末尾 `/` 解釈との差分は、
MCP 側の path 正規化規則として明示的に吸収する。

#### 2.2.10 `append` の保存フロー

`append` は MCP 専用の更新操作として扱うが、
内部では「既存最新ソースの読取」「追記後全文の生成」「amend 相当判定」
「競合制御付き保存」の 4 段に分けて扱う。

概念上の入出力は以下を基本とする。

```rust
struct AppendRequest {
    path: NormalizedPath,
    content: String,
}

struct AppendResult {
    path: NormalizedPath,
    revision: u64,
    amended: bool,
}

enum AppendFailure {
    InvalidInput,
    NotFound,
    Draft,
    Conflict,
    InternalError,
}
```

`append` の処理順序は以下で固定する。

1. `path` を `resolve_page_by_path` で current page へ解決する
2. 解決済み current path に対して `append` と path prefix 制約を判定する
3. `content` を追記文字列として検証する
4. 最新 revision の source を取得する
5. 追記後全文を生成する
6. amend 相当で保存できるかを判定する
7. 必要に応じてロック解除を待機する
8. `append` 専用保存 API または `put_page` 補助経路で保存する
9. `AppendResult` を構築する

`append` の「単純追記」は以下を意味する。

- 既存本文の末尾へ `content` をそのまま連結する
- 既存本文の途中挿入、先頭挿入、置換、削除は行わない
- 追記位置は常に現在の最新 source の末尾である
- クライアントは全文を送らず、追記差分だけを送る

改行規則は以下とする。

- サーバは `content` の内部改変を原則行わない
- 既存本文末尾と `content` 先頭の間に改行を自動補完しない
- そのため、行を分けて追記したい場合の改行はクライアントが `content` に含める
- 空文字追記は無意味なため `InvalidInput` とする

この方針により、`append` は「末尾へバイト列的に追記する」意味論を保ち、
改行整形の暗黙規則を持ち込まない。

#### 2.2.11 amend 相当判定

`append` は通常の `update` と異なり、要求仕様に従って
限定条件下でのみ amend 相当保存を許可する。

amend 相当保存を許可する条件は以下をすべて満たす場合とする。

- 対象が draft ではない
- 追記対象が current page として正常解決できている
- 保存対象が「現在の最新 revision」に対する追記である
- 最新 revision の直前更新者が、今回の操作ユーザと同一である
- 競合制御上、保存直前に観測した最新 revision が変更されていない

上記を満たす場合は、既存最新 revision を上書きする amend 相当保存として扱う。
1 つでも満たさない場合は、新規 revision を追加する通常 append とする。

ただし以下は amend 相当ではなく即時失敗とする。

- draft への append
- current page を解決できない
- `content` が空
- ロック待機が必要だが期限内に解消しない

`AmendForbidden` は、
「amend 相当保存を要求したが認められない」という外部指定失敗ではなく、
内部判定の結果として conflict 系に写像するための内部失敗として扱う。
MCP 公開面では、append ツールに amend 指定引数を公開しない。

#### 2.2.12 競合制御と待機方針

要求仕様に従い、`append` は通常更新より緩い競合制御を許可しない。
そのため、保存直前に対象ページが他操作でロック中または最新 revision 競合状態にある場合は、
一定時間だけ待機し、それでも解消しなければ conflict とする。

待機対象は以下の 2 系統とする。

- 明示ロック競合
  - 既存ロック機能により対象ページが他主体に保持されている場合
- 最新 revision 競合
  - 追記前に読んだ最新 revision と、保存直前の最新 revision が一致しない場合

待機制御の基本方針は以下とする。

- 待機は `append` 専用サービス層で行う
- DB 層へ無限再試行を持ち込まない
- 待機中は短い間隔で current state を再確認する
- 待機上限時間を超えた場合は `Conflict` とする

初期設計では、待機に関する具体値は設定化せず、
固定の短時間タイムアウトを内部定数として持つ方針でよい。
最終的な秒数は 4.7 以降または実装タスクで詰める。

競合判定の流れは以下を基本とする。

1. 最新 revision と source を読む
2. 追記後全文を組み立てる
3. 保存直前に lock / latest revision を再確認する
4. 競合があれば短時間待機して 1 へ戻る
5. 待機上限を超えたら `Conflict`
6. 競合が解消したら save を実行する

この設計により、
append が「読んだ時点の古い末尾」へ追記してしまうことを避ける。

#### 2.2.13 既存保存 API との接続方針

初期段階では、`append` 保存そのものは
既存 `put_page(page_id, user_name, source, amend)` を再利用する方向を第一候補とする。

サービス層が担当する責務は以下とする。

- current path 解決
- append 権限判定
- 既存 source 取得
- 追記後全文の組み立て
- amend 相当可否の判定
- lock / revision 競合の待機制御
- 保存結果の `revision` / `amended` 判定

DB 層へ委譲する責務は以下とする。

- 実際の source 書き込み
- amend の場合の既存 revision 更新
- 非 amend の場合の新規 revision 追加
- 既存 `PageSource` / `PageIndex` の整合維持

ただし現在の `put_page` は、
append 専用の競合待機や「読取時 revision と保存時 revision の一致確認」を持たない。
そのため、後続の 4.7.4 では以下のいずれかを設計対象とする。

- `put_page` をラップする `append_page_by_id`
- revision 前提条件を受け取る `put_page_if_latest`
- append 専用の保存 API を新設する

4.6.4 の時点では、公開設計上の保存フローとしては
「サービス層が append 固有判定を持ち、DB 層は最終書き込み責務を担う」
ところまでを確定事項とする。

#### 2.2.14 対象外機能の拒否方針

初期 MCP では、以下を公開対象に含めない。

- 削除済みページ参照
- restore
- アセット操作
- ロック操作

対象外機能に対する拒否は、
「ツール定義に含めないこと」と
「類似入力が既存ツールへ混入した場合の失敗分類」を
分けて定義する。

基本方針は以下とする。

- 公開ツール一覧には対象外機能を含めない
- 対象外専用の補助引数も定義しない
- 既存ツールの入力として対象外機能を示唆する値が渡された場合は、
  `unsupported` と `invalid_input` / `not_found` を区別して返す

各対象外機能の扱いは以下とする。

##### 2.2.14.1 削除済みページ参照

削除済みページ参照は初期 MCP の対象外であり、
current path を基準とする通常解決経路に含めない。

- `resolve_page_by_path` は `PAGE_PATH_TABLE` だけを参照し、
  `DELETED_PAGE_PATH_TABLE` を通常解決へ含めない
- そのため、削除済みページの旧 path を指定された場合は `NotFound` とする
- 「削除済みページを見たい」という専用要求が表現された場合のみ `unsupported` とする

この方針により、
通常の path 解決失敗と「削除済みページを読む専用機能がないこと」を区別できる。

##### 2.2.14.2 restore

restore は rename と明確に分離し、初期版では提供しない。

- `rename_page` ツールには `restore_to` のような引数を持ち込まない
- 削除済みページを復帰させる意図を持つ専用操作要求は `unsupported` とする
- 既存 path が current page として解決できない場合は、restore へ自動フォールバックせず `not_found` とする

つまり、MCP の rename は常に current page 同士の移動だけを扱い、
restore 相当の意味づけは行わない。

##### 2.2.14.3 アセット操作

アセットは初期 MCP の対象外であり、
ページツールから透過的に扱えるものとしても露出しない。

- asset 一覧、取得、追加、削除、ページとの紐付け変更はすべて `unsupported`
- page 本文中の asset 記法自体は page content の一部として扱うが、
  参照整合性解決や asset 実体操作は行わない
- asset 識別子や内部保存パスは MCP 応答へ含めない

これにより、MCP は初期段階では page text 操作に責務を限定する。

##### 2.2.14.4 ロック操作

ロック取得・更新・解除・参照は初期 MCP の対象外である。

- lock 専用ツールは公開しない
- lock token や lock 情報は外部入力にも外部出力にも含めない
- ただし page 更新系内部で lock 状態を観測すること自体は許可する
- `append` のように要求仕様上 wait を伴う場合でも、
  それは lock 操作公開ではなく内部競合制御として扱う

このため、MCP から lock を直接操作することはできないが、
内部実装は既存ロック基盤を競合検出のために再利用してよい。

#### 2.2.15 `unsupported` と他失敗分類の使い分け

対象外機能の拒否では、`unsupported` を広く使いすぎない。
使い分けは以下とする。

- `unsupported`
  - 初期版で提供しない操作種別そのものを要求している
  - 例: restore、asset 操作、lock 操作
- `not_found`
  - current path 前提の通常解決として対象が見つからない
  - 例: rename 後の旧 path、削除済みページの旧 path を通常 read した場合
- `invalid_input`
  - 現在の公開ツール入力として構文や意味が不正
  - 例: 不正 path、不正 section 指定

この整理により、初期版対象外であることと、
通常入力として単に見つからないことを応答上で混同しない。

### 2.3 操作種別ごとの橋渡し

#### 2.3.1 read

- 入力: 現在 path
- サービス層:
  - path 妥当性検証
  - read 用認可判定
  - path から `page_id` を解決
  - `page_id` を用いてページ情報や最新ソースを取得
- 出力: path ベースのページ情報

#### 2.3.2 list

- 入力: prefix path
- サービス層:
  - prefix 妥当性検証
  - prefix 自体に対する read 認可判定
  - `list_page_entries_by_prefix` を利用
  - 結果を必要に応じて認可範囲で後段フィルタ
- 出力: path ベース一覧

#### 2.3.3 search

- 入力: 検索式、任意 prefix
- サービス層:
  - 検索式検証
  - prefix 指定がある場合は prefix 妥当性検証と認可判定
  - FTS 実行
  - 検索結果の `page_id` から path を解決
  - 認可範囲外結果を除外
- 出力: path ベース検索結果

#### 2.3.4 create

- 入力: target path、初期ソース
- サービス層:
  - path 妥当性検証
  - target path に対する create 認可判定
  - 既存パス衝突確認
  - 既存の作成 API を呼び出し
- 出力: 作成後の path と内部結果

現在の DB API は `create_page` と `create_draft_page` を持つため、
MCP の create はドラフト前提ではなく通常ページ作成として扱う方向で設計する。

#### 2.3.5 update

- 入力: 現在 path、更新後ソース
- サービス層:
  - path 妥当性検証
  - 現在 path に対する update 認可判定
  - path から `page_id` を解決
  - `put_page` 相当の既存 API を利用
- 出力: 更新後の path と revision 情報

#### 2.3.6 append

- 入力: 現在 path、追記文字列
- サービス層:
  - path 妥当性検証
  - 現在 path に対する append 認可判定
  - path から `page_id` を解決
  - 現在ページ状態を確認
  - amend 相当可否を判定
  - `append` 専用の保存 API または `put_page` 補助経路を利用
- 出力: 更新後の path と revision 情報

`append` は公開面としては MCP 専用だが、
保存時の内部処理は page_id ベースで扱う。

#### 2.3.7 rename

- 入力: current path、rename_to path
- サービス層:
  - 双方の path 妥当性検証
  - current path と rename_to path の両方に対する update 認可判定
  - current path から `page_id` を解決
  - 既存の recursive rename API を利用
- 出力: 変更後 path

### 2.4 Bearerトークン管理情報の拡張設計

MCP の認証・認可を成立させるため、
Bearerトークン管理情報は現行の `read` / `write` 二値スコープ前提から拡張する。

4.7.1 では、
保存すべき項目、保存しない派生情報、既存データとの互換方針を
`BearerTokenInfo` 単位で確定する。

#### 2.4.1 Bearerトークン管理情報の保存項目

Bearerトークン管理情報の保存対象は、少なくとも以下とする。

```rust
struct BearerTokenInfo {
    token_id: TokenId,
    user_id: UserId,
    scopes: BearerScopeSet,
    path_prefixes: PathPrefixSet,
    created_at: DateTime<Local>,
    updated_at: DateTime<Local>,
    ttl: Duration,
    expire_at: DateTime<Local>,
    revoked: bool,
    name: Option<String>,
}
```

このうち現行実装との差分は以下である。

- `scopes`
  - `read` / `write` に加え、`create` / `update` / `append` / `delete` を保持可能にする
- `path_prefixes`
  - 正規化済み絶対 path 群を新規保持する

それ以外の `token_id` / `user_id` / `created_at` / `updated_at` /
`ttl` / `expire_at` / `revoked` / `name` は、既存責務を維持する。

#### 2.4.2 分解スコープの内部表現

`BearerScope` は拡張可能な列挙として維持し、初期 MCP 対応では以下を保持対象とする。

- `Read`
- `Write`
- `Create`
- `Update`
- `Append`
- `Delete`

保存方針は以下とする。

- DB には CLI 指定どおりのスコープ集合をそのまま保存する
- `write` を保存時に `read` / `create` / `update` / `append` / `delete` へ展開しない
- 分解済みスコープだけを持つ場合でも `write` を自動付与しない
- 重複は `BearerScopeSet` 側で除去する
- 表示順とシリアライズ文字列は外部仕様に合わせて安定化する

この方針により、
保存データは「指定内容の記録」であり、
包含規則は `allows(required_scope)` などの判定ロジック側へ閉じ込められる。

#### 2.4.3 path prefix 群の内部表現

`path_prefixes` は、Bearer 認証成功後の path jail 判定に使う保持項目とする。

概念上の型は以下を基本とする。

```rust
struct PathPrefixSet {
    prefixes: BTreeSet<NormalizedPath>,
}
```

保持規則は以下とする。

- 保持対象は正規化済み絶対 path のみ
- 重複 prefix は保持しない
- `/` を含む場合は全領域アクセス可として扱う
- prefix 未設定と `/` 保持状態は、判定上は同義として扱ってよい
- 包含関係にある複数 prefix は、より広い prefix へ縮約して保持してよい

`PathPrefixSet` が持つ責務は以下とする。

- path prefix 制約の永続化
- current path / target path / requested prefix に対する許可判定
- CLI の path 制約表示に必要な基礎情報提供

一方で以下は保存しない。

- path 制約の有無を表す専用 bool
- 「全領域アクセス」表示用の専用フラグ
- path jail 名や説明文などの表示専用文字列

これらは `path_prefixes` から導出する。

#### 2.4.4 CLI 表示用情報の保存責務

`token create` / `token list` / `token info` で表示したい情報のうち、
保存するものと導出するものを分ける。

保存対象は以下とする。

- `token_id`
- `user_id`
- `scopes`
- `path_prefixes`
- `created_at`
- `updated_at`
- `ttl`
- `expire_at`
- `revoked`
- `name`

保存しない派生表示情報は以下とする。

- 対象ユーザ名
  - `user_id` から都度解決する
- 実効権限表示
  - `scopes` から導出する
- path 制約有無表示
  - `path_prefixes` から導出する
- 全領域アクセス警告表示
  - `path_prefixes` が未設定または `/` を含むかから導出する
- 期限切れ状態表示
  - `expire_at` と現在時刻から導出する

この方針により、一覧表示や詳細表示のためだけの冗長列を持たずに済む。
また、ユーザ名変更時も `user_id` 解決だけで最新表示へ追従できる。

#### 2.4.5 `updated_at` の更新契機

`updated_at` は「管理情報の最終更新時刻」として維持し、
表示用の `last_used_at` 代替にはしない。

更新契機は少なくとも以下とする。

- トークン新規作成
- TTL 延長
- revoke 実行
- path prefix 追加
- path prefix 削除

一方で、単なる一覧表示や認証失敗では更新しない。

#### 2.4.6 既存データとの互換方針

Bearerトークン管理情報については、
要求仕様どおり保存形式変更を許容する前提で進める。

ただし、移行時の読み取り互換は以下を基本とする。

- 旧データの `scopes` が `read` / `write` だけでも読めること
- `path_prefixes` を持たない旧データは「全領域アクセス可」として解釈できること
- 新フィールド追加後も MessagePack named field ベースで後方読取の逃げ道を持てること

初期実装では、Bearer 管理情報だけは必要に応じて
デシリアライズ互換補助や移行処理を導入してよい。
この点は、他の永続化データ原則維持の例外として扱う。

#### 2.4.7 4.7.1 時点で保存対象に含めない項目

以下は 4.7.1 の保存対象に含めない。

- `last_used_at`
- 実効権限のキャッシュ列
- path 制約有無フラグ
- 監査ログ専用の集計情報
- MCP 専用の追加メタデータ

理由は、いずれも既存保持項目や実行時文脈から導出可能であり、
認証時または表示時の冗長更新を増やす利益が小さいためである。

### 2.5 ユーザ属性モデルの拡張設計

`NoBasicAuth` および `ReadOnly` を実装へ落とし込むため、
`UserInfo` は従来の「認証情報 + 表示名」だけでなく、
将来拡張可能な属性集合を保持できる形へ拡張する。

4.7.2 では、
保存形式、Basic 認証判定責務、既存ユーザデータとの互換方針を確定する。

#### 2.5.1 ユーザ属性の保存項目

`UserInfo` の保存対象は、少なくとも以下を基本とする。

```rust
struct UserInfo {
    id: UserId,
    username: String,
    password: String,
    salt: [u8; 16],
    display_name: String,
    attributes: UserAttributeSet,
    timestamp: DateTime<Local>,
}
```

現行実装との差分は `attributes` の追加である。
他の既存項目は従来責務を維持する。

`attributes` を `UserInfo` 側へ置く理由は以下とする。

- `NoBasicAuth` はトークン単位ではなくユーザ単位の性質である
- Basic 認証可否は `UserInfo` 解決時点で判断できるべきである
- `ReadOnly` もトークン単位ではなくユーザ単位の性質である
- write 系操作可否も `UserInfo` 解決時点で判断できるべきである
- Bearer トークン管理情報へ重複保持すると責務が分散する

#### 2.5.2 属性集合の内部表現

ユーザ属性は拡張可能な列挙集合として保持する。
初期実装で導入する属性は `NoBasicAuth` と `ReadOnly` とする。

概念上の型は以下を基本とする。

```rust
enum UserAttribute {
    NoBasicAuth,
    ReadOnly,
}

struct UserAttributeSet {
    attributes: BTreeSet<UserAttribute>,
}
```

保持規則は以下とする。

- 重複属性は保持しない
- 未設定は「属性なし」を意味する
- 初期実装で未知属性は生成しない
- 将来属性追加時に列挙拡張できる形を維持する

`UserAttributeSet` が持つ責務は以下とする。

- 属性集合の永続化
- `NoBasicAuth` や `ReadOnly` など個別属性の包含判定
- CLI 詳細表示の基礎情報提供

#### 2.5.3 `NoBasicAuth` / `ReadOnly` の判定責務

`NoBasicAuth` は Basic 認証拒否のための共通ユーザ属性とし、
判定責務は `UserInfo` 側へ置く。

責務分担は以下とする。

- `UserInfo`
  - `NoBasicAuth` を保持する
  - `ReadOnly` を保持する
  - Basic 認証を許可できるかを判定できる
  - write 系操作を許可できるかを判定できる
- REST API Basic 認証入口
  - 資格情報検証後に `UserInfo` の属性を参照し、拒否時は 401 を返す
- Bearer 認証入口
  - `NoBasicAuth` を拒否条件に使わない
  - `ReadOnly` に必要な属性情報を後段認可へ渡す
- MCP 認証入口
  - Basic 認証を受理しないため、`NoBasicAuth` 自体は判定しない
  - `ReadOnly` は認証失敗理由には使わず、write 系認可で `forbidden` 判定に使う

- MCP / REST の write 系認可
  - `ReadOnly` を保持するユーザに対しては、required scope や path prefix 制約を満たしていても write 系操作を拒否する

この方針により、
`NoBasicAuth` は「Basic を禁止するユーザ属性」であり、
Bearer や MCP の認可属性ではないことを明確に保てる。
一方で `ReadOnly` は「write を禁止するユーザ属性」であり、
Basic / Bearer / MCP を横断して後段認可で使う属性であることを明確に保てる。

#### 2.5.4 CLI 管理情報との責務分離

ユーザ属性は user 系 CLI で管理し、
token 系 CLI には混在させない。

保存対象は以下とする。

- `attributes`

保存しない派生表示情報は以下とする。

- `basic_auth_allowed`
  - `attributes` から導出する
- 属性説明文
  - 表示時に列挙名から導出する

表示責務の方針は以下とする。

- `user add`
  - 初期属性の指定を受け付ける
- `user edit`
  - 属性の追加・削除・置換を扱えるようにする
- `user list`
  - 一覧責務を優先し、属性詳細は表示しない
- `user info`
  - 属性集合を含む完全表示の出口とする
- `token list` / `token info`
  - ユーザ属性を表示しない

#### 2.5.5 既存データとの互換方針

既存 `UserInfo` には `attributes` が存在しないため、
追加後の読取互換を用意する。

互換方針は以下を基本とする。

- `attributes` を持たない旧データは空集合として解釈する
- 旧データのユーザはすべて Basic 認証可能ユーザとして扱う
- 新フィールド追加後も MessagePack named field ベースで後方読取の逃げ道を持てること

この方針により、既存ユーザデータを即時移行しなくても
`NoBasicAuth` 未設定ユーザとして継続利用できる。

#### 2.5.6 4.7.2 時点で保存対象に含めない項目

以下は 4.7.2 の保存対象に含めない。

- Basic 認証可否の専用 bool
- Bearer 専用属性
- MCP 専用属性
- 属性ごとの説明文キャッシュ

理由は、いずれも `attributes` から導出可能であり、
属性集合と二重管理する利益が小さいためである。

### 2.6 サービス層の入出力モデル

サービス層は、MCP 公開層から HTTP 依存情報を受け取らず、
次のような内部モデルを受け取る前提とする。

- 認証済み主体
- Bearer スコープ集合
- path prefix 制約情報
- 操作種別
- path ベース入力

これにより、MCP transport 依存の文脈は `transport.rs` / `handler.rs` で閉じ、
サービス層は業務ロジックに集中できる。

### 2.7 追加が必要な DB API

橋渡し設計の成立には、少なくとも以下の DB 側補助 API を後続で設計する必要がある。

- 現在 path からページを解決する API
- path ベースで現在ページ状態を取得する API
- `append` 実装に必要な page_id ベース補助 API

既存の `list_page_entries_by_prefix`、`create_page`、`put_page`、
`rename_pages_recursive_by_id` は再利用候補とする。

4.7.3 では、このうち読取系に限定して、
MCP サービス層が必要とする API を次の方針で整理する。

#### 2.7.1 4.7.3 の基本方針

DB API 拡張は、MCP サービス層の都合で
既存 `DatabaseManager` を全面的に path ベースへ寄せることを目的としない。

方針は以下とする。

- DB 層の主軸は従来どおり `page_id` ベースとする
- MCP 向けに追加する path ベース API は、current path 解決と現在状態取得に限定する
- list は既存 `list_page_entries_by_prefix` を再利用し、新規 API は追加しない
- search は FTS 実行自体を既存のまま維持し、結果整形に必要な path 解決だけを薄く追加する
- rename 支援のための衝突確認は、汎用の path 解決 API を再利用して行う
- restore や削除済みページ参照は初期版対象外のため、削除済み path 専用 API は本節の対象に含めない

この方針により、MCP 側は
「path を受けて必要最小限の current page 情報を得る」ことだけを DB 層へ委譲し、
認可、入力検証、後段フィルタ、エラー分類はサービス層に残す。

#### 2.7.2 既存 API の再利用範囲

4.7.3 時点で、以下の既存 API はそのまま再利用候補とする。

- `get_page_id_by_path`
  - 単純な current path 解決の最小単位として利用できる
- `get_page_index_by_id`
  - page の current / deleted / draft 状態確認に利用できる
- `get_page_source`
  - 最新 revision の本文取得に利用できる
- `list_page_entries_by_prefix`
  - list の prefix 走査とページ一覧構築にそのまま利用できる
- `get_page_lock_info`
  - update / append / rename の事前ロック確認に利用できる

一方で、MCP サービス層がこれらを都度組み合わせるだけでは、
read / update / append / rename で同じ読取手順が重複しやすい。
また search では `page_id` ごとに `get_page_index_by_id` を繰り返す
N+1 型アクセスが発生しやすい。

そのため、current path 解決系と検索結果解決系については、
薄い集約 API を追加する。

#### 2.7.3 current path 解決 API

`resolve_page_by_path` の DB 側入口として、
current path から現在ページを一貫した読取トランザクションで解決する
専用 API を追加する。

概念上の責務は以下とする。

- 入力された current path を `PAGE_PATH_TABLE` で `page_id` へ解決する
- 解決した `page_id` に対して `PAGE_INDEX_TABLE` を参照する
- draft / deleted / current path 欠落のような状態を current page 観点で返せるようにする
- 必要に応じて最新 revision の `PageSource` も同一読取トランザクションで取得する

返却モデルは、MCP サービス層が read / update / append / rename の共通前処理に
そのまま使える粒度とする。少なくとも以下の項目を保持する。

- `page_id`
- `page_index`
- `latest_revision`
- `latest_source`
- `current_path`

概念上の API 名は以下を想定する。

- `get_current_page_state_by_path(path)`

返却方針は以下とする。

- current path に一致する live page がある場合は `Some(CurrentPageState)` を返す
- path が見つからない場合は `None` を返す
- deleted page のみが存在する場合でも `PAGE_PATH_TABLE` に現れないため `None` として扱う
- draft page が current path として見つかった場合は、MCP 初期版の通常 read / update 対象ではないため、状態識別可能な値を返すか `CurrentPageState` 内に `is_draft` を保持する

ここで deleted / draft / current path 欠落を個別エラーへ変換する責務は、
DB 層ではなく MCP サービス層に置く。
DB 層は「current page として解決できたか」と
「解決先の現在状態はどうか」を返す役割に留める。

#### 2.7.4 read / update / edit / append / rename に対する適用範囲

`get_current_page_state_by_path(path)` は、少なくとも以下の操作で共通利用する。

- `get_page`
  - current path 解決
  - 最新本文取得
  - current path の最終確認
- `get_page_section`
  - `get_page` と同じ current page 解決結果を利用
- `update_page`
  - current path 解決
  - latest revision と最新本文取得
- `edit_page`
  - current path 解決
  - latest revision と最新本文取得
  - revision / instance_id 整合確認の前提取得
- `append_page`
  - current path 解決
  - latest revision と最新本文取得
  - amend 判定用の最新記録取得
- `rename_page`
  - current path 解決
  - current path の確定
  - draft / deleted 除外

この API を導入することで、
MCP サービス層は `get_page_id_by_path` → `get_page_index_by_id` → `get_page_source`
の定型列を繰り返し記述せずに済む。

#### 2.7.5 search 結果の path 解決 API

search では FTS が `page_id` と revision を返すが、
MCP 公開面では path ベース結果へ変換する必要がある。

既存実装でも `page_id` ごとに `PageIndex` から current path を引く流れはあるが、
MCP では後段フィルタ前提でこの処理をより頻繁に利用するため、
複数 `page_id` をまとめて current path 解決できる API を追加する。

概念上の API 名は以下を想定する。

- `get_current_page_paths_by_ids(page_ids)`

責務は以下とする。

- 複数 `page_id` に対して current path を一括取得する
- deleted page は current path を持たないため結果から除外するか、`None` として返す
- draft page は MCP 初期版 search 結果に含めない前提とし、結果から除外してよい
- 順序保証は必須とせず、`PageId -> CurrentPathInfo` の写像として返す

`CurrentPathInfo` が持つ項目は、少なくとも以下とする。

- `current_path`
- `deleted`
- `draft`

search の後段フィルタでは、
FTS 由来の `deleted` フラグだけでなく current path の有無も必要になるため、
結果整形用にはこの程度の情報を持てれば足りる。

#### 2.7.6 list に対する方針

list は既存 `list_page_entries_by_prefix(base_path, with_deleted)` を再利用し、
2.7.3 では新規 DB API を追加しない。

理由は以下の通りとする。

- MCP 初期版 list は current page 一覧だけを対象とし、削除済みページ一覧を要求しない
- `list_page_entries_by_prefix` は current path ベースの prefix 走査をすでに備える
- path prefix 制約に基づく要求 prefix 判定と結果後段フィルタはサービス層責務であり、DB API 側へ押し込む必要がない

MCP 側からの利用時は、
`with_deleted = false` を固定し、
取得後に path jail 判定を必ず再適用する。

#### 2.7.7 rename 支援に必要な読取 API

rename の事前確認に必要な読取は、専用 API を増やさず、
以下の組み合わせで足りるものとして設計する。

- current path 側
  - `get_current_page_state_by_path(path)`
- rename 先衝突確認
  - `get_page_id_by_path(rename_to)`
- ロック確認
  - `get_page_lock_info(page_id)`

追加の rename 専用読取 API を設けない理由は以下とする。

- 移動先が path jail 内かどうかはサービス層で文字列ベース判定できる
- 配下移動禁止や no-op 判定もサービス層で current path と `rename_to` を比較すれば足りる
- 実際の rename 可否の最終整合性は既存書込 API 側でも再検証すべきであり、読取 API 側で過剰に抱え込む必要がない

#### 2.7.8 2.7.3 時点で追加対象に含めない API

以下は 2.7.3 の対象外とする。

- 削除済み path から復元候補を解決する API
  - restore 非対応のため不要
- path prefix 制約込みで DB 側が結果を直接絞り込む API
  - 認可はサービス層責務とする
- MCP 専用の search API
  - 検索本体は既存 FTS を再利用する
- `append` の競合制御や amend 判定まで含めた API
  - 2.7.4 で書込 API として別途設計する

以上より、2.7.3 の設計結論は以下とする。

- list は既存 API をそのまま再利用する
- read / update / append / rename 向けに `get_current_page_state_by_path(path)` を追加する
- search 向けに `get_current_page_paths_by_ids(page_ids)` を追加する
- rename 先衝突確認は既存 `get_page_id_by_path` を再利用する

#### 2.7.9 2.7.4 の基本方針

2.7.4 では、`append` 実装に必要な DB 書込 API 拡張を整理する。

基本方針は以下とする。

- `append` の待機制御、再読込、amend 相当判定はサービス層責務とする
- DB 層は「保存直前に観測した前提がまだ成り立つか」を検証しつつ書き込む責務を持つ
- `put_page(page_id, user_name, source, amend)` をそのまま直接使うのではなく、前提 revision を受け取れる薄い専用 API を追加する
- DB 層は保存後の確定 revision と amend 実行有無を返し、サービス層が再推測しなくてよい形にする
- lock 待機そのものは DB 層へ持ち込まず、保存時点で lock が残っていれば conflict 系失敗として返す

この分離により、
サービス層は要求仕様どおりの短時間待機ループを実装でき、
DB 層は 1 回の保存試行における整合性確認へ責務を限定できる。

#### 2.7.10 既存 `put_page` の不足点

既存 `put_page(page_id, user_name, source, amend)` は、
通常の page 更新 API としては再利用価値があるが、
`append` の保存 API としては以下の不足がある。

- 保存直前の `latest revision` が、呼び出し元の想定と一致するかを検証できない
- amend 不可理由が「直前更新者不一致」なのか「latest revision 変化」なのかを区別できない
- 保存後に確定した revision 番号を返さない
- amend 実行有無を返さない
- lock 競合を `append` 用 compare-and-write の文脈で扱えない

特に `append` では、
「古い revision を読んで組み立てた全文を、最新が変わった後にそのまま保存しない」
ことが重要である。
この条件は `put_page` の現状インタフェースでは表現しきれないため、
専用 API の追加を前提とする。

#### 2.7.11 `append` 用書込 API の責務

`append` 用書込 API は、少なくとも以下を 1 回の書込トランザクション内で扱う。

- `page_id` に対応する current page の存在確認
- 対象ページが draft でないことの確認
- 保存直前の latest revision が期待値と一致することの確認
- lock 競合の最終確認
- amend 指定時の直前更新者一致確認
- source 書き込み
- `PageIndex.latest` 更新
- 保存結果 revision の返却

ここでいう「期待値」は、
サービス層が再読込後に観測した latest revision を指す。
DB API はその期待値を compare-and-write 条件として受け取り、
不一致であれば保存せず失敗を返す。

#### 2.7.12 想定する入力モデル

概念上の入力は以下を想定する。

```rust
struct AppendWriteRequest {
    page_id: PageId,
    user_name: String,
    source: String,
    expected_latest_revision: u64,
    allow_amend: bool,
}
```

各項目の意味は以下とする。

- `page_id`
  - current path 解決済みの対象ページ
- `user_name`
  - 保存主体のユーザ名
- `source`
  - 追記後の全文
- `expected_latest_revision`
  - 保存直前に一致していることを要求する latest revision
- `allow_amend`
  - サービス層が amend 相当条件を満たすと判定した場合のみ `true`

`allow_amend` は、
「必ず amend しろ」という意味ではなく、
「条件が一致していれば同 revision 更新を許可してよい」という意味で扱う。
ただし 2.7.4 の設計では、latest revision 一致確認後に
amend と通常新規 revision 追加のどちらを行うかは DB 層で確定してよい。

#### 2.7.13 想定する出力モデル

概念上の出力は以下を想定する。

```rust
struct AppendWriteResult {
    revision: u64,
    amended: bool,
}

enum AppendWriteFailure {
    PageNotFound,
    DraftPage,
    PageLocked,
    RevisionConflict,
    AmendForbidden,
    UserNotFound,
}
```

返却方針は以下とする。

- amend で保存した場合
  - `revision` は更新した既存 latest revision
  - `amended` は `true`
- 新規 revision を追加した場合
  - `revision` は新規 revision 番号
  - `amended` は `false`

失敗分類の意味は以下とする。

- `RevisionConflict`
  - `expected_latest_revision` と保存直前 latest revision が一致しない
- `AmendForbidden`
  - `allow_amend = true` だが、直前更新者不一致などで amend 条件を満たさない
- `PageLocked`
  - 保存時点で有効なロックが残っている

MCP サービス層では、これらの失敗を受けて
待機継続、再読込、または最終的な conflict 応答へ写像する。

#### 2.7.14 DB 層が担う revision 決定責務

revision の最終決定責務は DB 層に置く。

理由は以下の通りとする。

- latest revision は保存トランザクション内で最終確認する必要がある
- amend 成否によって返却 revision が変わる
- サービス層で revision を先計算すると、競合再試行時に整合を崩しやすい

決定規則は以下とする。

- 保存直前 latest revision が `expected_latest_revision` と一致しない場合
  - 書き込まず `RevisionConflict`
- 一致し、かつ `allow_amend = true` で直前更新者も一致する場合
  - latest revision を上書きし `amended = true`
- 一致し、`allow_amend = false` または amend 条件を満たさない場合
  - `expected_latest_revision + 1` の新規 revision を追加し `amended = false`

この規則により、同一 revision への上書きと新規 revision 追加を
単一 API の返却値で一貫して表現できる。

#### 2.7.15 lock 競合の扱い

lock 待機自体はサービス層責務だが、
保存直前に lock が復活または継続している可能性があるため、
DB API でも最終確認を行う。

DB 層の扱いは以下とする。

- 書込トランザクション開始後に対象 page の lock 状態を確認する
- 有効 lock が存在する場合は書き込まず `PageLocked` を返す
- expired lock の掃除は既存 lock 取得系と同等の整理に従う

これにより、サービス層の待機ループをすり抜けた
直前 lock 競合を DB 層で補足できる。

#### 2.7.16 API 形の候補

API 名は実装時に最終決定するが、
2.7.4 時点の候補は以下とする。

- `append_page_by_id(request)`
- `put_page_if_latest(request)`

意味論としてはどちらでもよいが、
初期版では `append` 専用であることが分かる
`append_page_by_id` を第一候補とする。

理由は以下の通りとする。

- MCP 初期版では `append` が REST API 非公開であり、責務を混同しにくい
- `put_page_if_latest` という汎用名より、要求仕様との対応が追いやすい
- 将来 `update` 側へ同様の compare-and-write を広げる場合でも、
  その時点で共通 API へ再抽象化できる

#### 2.7.17 既存 `put_page` との関係

2.7.4 の設計結論として、
既存 `put_page` は通常更新 API として維持しつつ、
`append` 用には別 API を追加する方針とする。

整理は以下の通りとする。

- `put_page`
  - 既存 update 系や内部共通処理で継続利用可能
- `append_page_by_id`
  - `append` 専用の compare-and-write API
  - `expected_latest_revision` と `allow_amend` を受け取る
  - `revision` と `amended` を返す

必要であれば実装時に内部で `put_page` の一部処理を再利用してよいが、
公開する DB API 境界としては分離する。

#### 2.7.18 2.7.4 時点で対象外とする事項

以下は 2.7.4 の対象外とする。

- DB 層内での待機ループや再試行制御
- `append` 以外の update 系へ compare-and-write を一般化すること
- 監査ログ書込まで DB API に含めること
- path ベース入力を DB API が直接受け取ること

以上より、2.7.4 の設計結論は以下とする。

- `append` 用書込 API はサービス層待機制御の下で呼ばれる compare-and-write API とする
- 入力には `expected_latest_revision` と `allow_amend` を含める
- 出力には `revision` と `amended` を含める
- latest revision 不一致は `RevisionConflict`、保存時 lock は `PageLocked` として返す
- 既存 `put_page` は流用候補ではあるが、MCP `append` の公開責務を満たす API 境界としては別 API を追加する

#### 2.7.19 2.7.5 の基本方針

2.7.5 では、監査ログ設定値の保存先と読込方法を整理する。

基本方針は以下とする。

- 監査ログ設定値は DB や redb へ保存しない
- サーバ起動時設定として `config.toml` と `run` コマンド入力から解決する
- 解決済み設定は HTTP サーバ統合層から監査ログ基盤初期化へ渡す
- 監査ログ writer / rotation / retention が同一の解決済み設定構造体を共有する

監査ログは Wiki コンテンツではなくサーバ運用設定に属するため、
トークン情報やユーザ属性のような永続化データとは分離して扱う。

#### 2.7.20 保存先の選定

監査ログ設定値の保存先は、初期実装では `config.toml` を正とする。

保存候補の比較は以下の通りとする。

- DB 保存
  - 実行中に変更しやすいが、サーバ起動前に必要な writer 初期化値を取り出しにくい
  - export / import やデータ互換性の論点を不要に増やす
- `config.toml` 保存
  - 既存の `run` / TLS / パス設定と同じ解決経路に乗せられる
  - relative path 解決や `--save-config` と整合を取りやすい
  - 起動時の監査ログ基盤初期化と相性がよい

このため、2.7.5 の設計結論として
監査ログ設定値は `config.toml` 保存とする。

#### 2.7.21 config 上の責務配置

監査ログ設定は初期用途が `run` サブコマンドでの MCP 公開に紐付くため、
`config.toml` 上では `run` セクション配下へ置く方針とする。

理由は以下の通りとする。

- 監査ログ基盤の初期化契機はサーバ起動時である
- CLI 反映経路も `run` サブコマンドへ自然に接続できる
- 既存 `run.bind_addr` / `run.bind_port` / `run.use_tls` と同列に扱うことで、
  「サーバ起動設定」のまとまりを維持できる
- `global.log_output` は通常ログ出力先であり、監査ログと混在させない方が責務分離が明確である

概念上の config 形は以下を想定する。

```toml
[run.audit]
enabled = true
path = "audit"
retention = "90d"
rotate_size = "10M"
```

ここでの項目名は内部設計上の仮名であり、
CLI オプション名や最終キー名の確定は 4.8.2 で行う。
2.7.5 では、少なくとも `run` 配下に独立した `audit` サブセクションを持てる構造とする方針だけを固定する。

#### 2.7.22 初期実装で保持する設定項目

2.7.5 時点で、監査ログ基盤が起動時に必要とする設定項目は以下とする。

- `enabled`
  - 監査ログ基盤を有効化するか
- `path`
  - 監査ログ出力ディレクトリ
- `retention`
  - 保持期間
- `rotate_size`
  - ローテーション閾値となるファイルサイズ

これらのうち、
`enabled` は MCP 有効化時の付随既定値として扱えるが、
将来的な監査基盤再利用や明示無効化余地を残すため独立項目として保持してよい。

一方で以下は 2.7.5 の保存対象に含めない。

- `append` 集約の 1 分窓
- flush 周期
- 起動時 retention 掃除の実行間隔

これらは初期実装では内部定数または実装内既定値とし、
設定面の複雑化を避ける。

#### 2.7.23 既定値の扱い

既定値は、入力文書で確定している運用前提に合わせて以下とする。

- `enabled`
  - MCP が有効な場合は既定で `true`
  - MCP 無効時は監査ログ基盤を起動しない
- `path`
  - 通常ログ用 `default_log_path()` とは分離し、
    データディレクトリ配下の専用サブディレクトリを既定とする
  - 初期案は `DEFAULT_DATA_PATH.join("audit")`
- `retention`
  - デフォルト 3 か月
- `rotate_size`
  - 固定サイズローテーションに必要な内部既定値を持つ
  - 具体値は 4.8.2 または `MCP_AUDIT_LOG_DESIGN.md` 側で最終確定してよい

`path` を通常ログの `global.log_output` と共有しない理由は以下の通りとする。

- 通常ログと監査ログで保存形式が異なる
- 保持削除やローテーション単位も分離したい
- 運用上、監査ログだけを保全・抽出しやすくする必要がある

#### 2.7.24 relative path の読込方針

監査ログ出力先 `path` は、
既存 `Config::resolve_path()` と同じ規則で解決する。

すなわち以下の通りとする。

- 絶対 path が指定された場合はそのまま利用する
- 相対 path が指定された場合は `config.toml` の親ディレクトリ基準で解決する
- `config.toml` のパスが不明な場合は相対 path のまま保持してよい

この方針により、
既存 `log_output` / `db_path` / `server_cert` と同様の使用感を保てる。

#### 2.7.25 読込と解決の責務分担

監査ログ設定の読込と解決は、以下の責務分担とする。

- `src/cmd_args/config.rs`
  - `run.audit` セクションの保存構造を定義する
  - raw 設定値のロードと relative path 解決を担う
- `src/cmd_args/run.rs`
  - CLI 入力値と config 値の優先順位解決を担う
  - 文字列表現のバリデーションを担う
- `src/command/run.rs`
  - 解決済みの監査ログ設定を起動コンテキストへ格納する
- `src/http_server/mod.rs`
  - サーバ起動時に監査ログ基盤を初期化する
  - 停止時 flush や retention 保守の起動契機を担う
- `src/audit/`
  - 解決済み設定構造体を受け取り、writer / rotation / retention を初期化する

このとき `Config` は raw 値の保管と path 解決までに留め、
「MCP が有効なら audit.enabled を暗黙 true にする」といった
運用ルールの最終解決は `run` 側で行う。

#### 2.7.26 解決済み設定モデル

監査ログ基盤へ渡す解決済み設定モデルは、
少なくとも以下の項目を持つ。

```rust
struct AuditLogConfig {
    enabled: bool,
    directory: PathBuf,
    retention: Duration,
    rotate_size_bytes: u64,
}
```

ここでの `Duration` や `u64` は解決済み内部表現であり、
CLI / config 上の文字列表現とは分離する。

これにより `src/audit/` 側は
設定文字列のパース責務を持たず、初期化に必要な値だけを受け取れる。

#### 2.7.27 `run` 起動経路への伝播

`run` コマンドから監査ログ基盤初期化までの伝播は以下の経路とする。

1. `src/cmd_args/run.rs`
   - CLI 値と config 値を統合し、監査ログ raw 設定を解決する
2. `src/command/run.rs`
   - `RunCommandContext` に `AuditLogConfig` を保持する
3. `src/http_server/mod.rs`
   - `AuditLogConfig` を受け取り、MCP 有効化判定と合わせて監査ログ基盤を初期化する
4. `src/audit/mod.rs`
   - writer / rotation / retention / buffer を構成する

監査ログ設定は `AppState` へ常駐保持してもよいが、
初期化後に変更しない設定であるため、
まずは HTTP サーバ統合層から監査基盤へ注入するだけで足りる。
`AppState` に必須で持たせるかは 4.8 以降の組み込み設計で最終判断する。

#### 2.7.28 `--save-config` との整合

監査ログ設定は `config.toml` 保存対象であるため、
`--save-config` 時にも既存の保存経路へ統合する。

2.7.5 の時点では、少なくとも以下の方針を固定する。

- `run` サブコマンドで指定した監査ログ設定は `run.audit` へ保存する
- 保存時は raw 文字列表現を保持してよい
- path は、既存設定保存方針に合わせて相対のまま保存してよい
- 解決済み絶対 path を config へ書き戻さない

これにより、config の可搬性と手編集しやすさを維持する。

#### 2.7.29 2.7.5 の設計結論

以上より、2.7.5 の設計結論は以下とする。

- 監査ログ設定値は DB ではなく `config.toml` に保存する
- config 上の配置は `run.audit` サブセクションを第一候補とする
- 保持項目は `enabled` / `path` / `retention` / `rotate_size` とする
- path は既存 `Config::resolve_path()` と同じ規則で解決する
- 解決済み設定は `AuditLogConfig` として `run` 起動経路から `src/audit/` へ渡す
- CLI 名や具体的な入力書式の最終確定は 4.8.2 で行う

### 2.8 path ベース橋渡しに関する設計判断

本章の path ベース橋渡し設計では、以下を基本方針として採用する。

- path ベース入力はすべてサービス層で検証・正規化する
- 認可判定は path ベースで先に行い、その後に必要な page_id 解決を行う
- DB 層は page_id / prefix ベース API を維持し、必要な path 解決 API のみ追加する
- list / search / create / update / append / rename ごとに橋渡し経路を明示する
- `append` は path ベース公開、page_id ベース保存の二段構成とする

## 3. 更新系操作の共通化単位

本章では、`append` を含む更新系操作の共通化単位として、
create、update、append、rename を個別実装として散在させず、
サービス層で共有できる内部モデルへ整理する。

初期実装の MCP 対象では delete を公開しないため、
本節の共通化対象は create、update、append、rename の 4 操作とする。

### 3.1 共通化の基本方針

更新系操作は外部から見ると差分が大きいが、
内部では少なくとも以下の共通軸を持つ。

- 操作主体
- 対象 path
- 必要スコープ
- path prefix 制約の判定対象
- 事前解決したページ状態
- 実行後に返す path / revision / summary 情報
- 監査ログへ渡す結果情報

このため、サービス層では
「操作ごとに別関数を持つが、内部で受け渡す入出力モデルは共有する」
方針を採る。

### 3.2 共通入力モデル

更新系操作の共通入力モデルとして、
少なくとも以下の情報を保持する内部構造を想定する。

- `actor`
  - 認証済みユーザ
  - Bearer スコープ
  - path prefix 制約
- `operation`
  - `create`
  - `update`
  - `append`
  - `rename`
- `target_path`
  - 操作対象の現在 path または作成先 path
- `payload`
  - 本文更新
  - 追記文字列
  - rename 先 path
- `options`
  - amend 可否
  - 将来拡張用の追加フラグ

この共通入力モデルは、MCP の公開入力と 1 対 1 対応させず、
公開層での検証後にサービス層へ受け渡す内部表現とする。

### 3.3 共通解決済みコンテキスト

更新系操作の実行前には、操作種別に応じて
共通の解決済みコンテキストを構築する。

解決済みコンテキストには以下を含める。

- 正規化済み path
- 操作対象ページの有無
- 必要時の `page_id`
- 現在のページ状態
  - draft / normal / deleted
- 現在 revision
- rename の場合の移動先 path

このコンテキストにより、
create と update / append / rename のように
前提状態が異なる操作でも、
実行前検証の手順を揃えられる。

### 3.4 操作別の差分

#### 3.4.1 create

- 対象は「まだ存在しない target path」
- `page_id` の事前解決は不要
- 本文は必須
- 実行結果として新規 `page_id` と初回 revision を得る

#### 3.4.2 update

- 対象は「既存の現在 path」
- `page_id` 解決が必要
- 本文は必須
- amend 指定は初期 MCP 設計では不要とし、通常更新として扱う

#### 3.4.3 edit

- 対象は「既存の現在 path」
- `page_id` 解決が必要
- `revision` と `instance_id` を受け取る
- 単一の編集操作を受け取り、最新本文へ部分編集を適用する
- サービス層で最新 revision および最新 instance_id との一致確認を行う
- 本文編集後の保存自体は update 系と同じ保存経路へ橋渡しできるようにする

#### 3.4.4 append

- 対象は「既存の現在 path」
- `page_id` 解決が必要
- 追記文字列を受け取る
- 内部では amend 相当判定を行う
- 公開面としては update と別操作だが、
  保存結果は revision 更新または既存 revision amend のいずれかになる

#### 3.4.5 rename

- 対象は「既存の現在 path」
- `page_id` 解決が必要
- rename 先 path を追加 payload として受け取る
- 認可判定対象は current path と rename_to path の両方
- 実行結果として新 path と revision を得る

### 3.5 共通出力モデル

更新系操作の結果は、監査ログと MCP 応答の双方で再利用できるよう、
共通出力モデルへ集約する。

少なくとも以下の項目を持つ想定とする。

- `operation`
- `target_path`
- `result_path`
- `revision`
- `instance_id`
- `summary`
- `audit_summary`

rename では `target_path` と `result_path` が異なり、
append では `summary` に amend 相当か新規 revision かを含める余地を持たせる。

### 3.6 サービス API の単位

サービス層の公開関数は、
操作別のエントリポイントを分けて可読性を維持する。

想定する単位は以下のとおりである。

- `create_page`
- `update_page`
- `edit_page`
- `append_page`
- `rename_page`

ただし内部では、

- 共通入力モデルの受理
- 共通解決済みコンテキストの構築
- 共通出力モデルの生成

を共有し、操作別差分のみを分岐させる。

`edit_page` は、
current page 解決と保存後応答生成は update 系共通処理を再利用しつつ、
編集操作の適用と revision / instance_id 整合確認を追加責務として持つ。

### 3.7 更新系操作の共通化に関する設計判断

本章の更新系操作共通化設計では、以下を基本方針として採用する。

- 更新系操作は操作別エントリポイントを維持する
- ただしサービス層の内部では共通入力モデル、解決済みコンテキスト、共通出力モデルを持つ
- create、update、append、rename を同一の更新系ファミリとして扱う
- `append` は公開面では独立操作、内部モデルでは更新系の一種として扱う
- 監査ログと MCP 応答へ渡す結果情報は共通出力モデルから組み立てる
