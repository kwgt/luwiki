# Bearer認証 実装設計書

本書は、`docs/REQUIREMENTS.md` で定義された Bearer 認証要件、および `docs/REST_API_SPECS.md` と `docs/CLI_SPECS.md` に定義された外部仕様に基づき、実装に向けた内部設計を整理するための文書である。

Basic認証と Bearer認証は同一の REST API 群で併用されるが、本書では Bearer認証を中心に扱い、Basic認証については Bearer認証との接点に限って記述する。

---

## 1. 文書の目的

- Bearer認証の内部設計対象を明確化する
- 実装前に、データモデル、認証処理、認可判定、CLI管理操作との責務分担を整理する
- 既存仕様と内部実装の対応関係を説明可能にする

## 2. 対象範囲

本書の対象範囲は以下の通りとする。

- REST API における Basic / Bearer 共通の認証入口
- Bearerトークン管理情報の内部データモデル
- Bearerトークン照合、スコープ判定、スライディング期限延長の処理
- `token create` / `token revoke` / `token purge` / `token list` と整合する内部保持項目および運用ルール
- Bearer認証に関するエラー処理方針および主要テスト観点

本書の対象外は以下の通りとする。

- ブラウザUI向け Basic認証の詳細仕様そのもの
- リフレッシュトークン、外部IdP連携、OAuth/OIDC など将来拡張の詳細設計
- Bearerトークン管理操作を CLI 以外へ公開するための設計
- Bearer認証とは独立したページロック機能の詳細設計

## 3. 前提と設計上の扱い

### 3.1 参照仕様

本書は少なくとも以下の文書を前提とする。

- `docs/REQUIREMENTS.md`
- `docs/BASE_DESIGN.md`
- `docs/REST_API_SPECS.md`
- `docs/CLI_SPECS.md`
- `docs/openapi.yaml`
- `docs/BEARER_AUTH_DESIGN_INPUT_TASKS.md`

### 3.2 前提条件

- Bearer認証の目的はアクセス制御それ自体ではなく、APIクライアントおよび将来のMCPサーバ機能からの操作主体識別と権限付与にある
- Bearerトークンの管理操作は CLI 限定とし、REST API からは発行・失効・削除・一覧表示を行わない
- Bearerトークン平文は発行時のみ取得可能とし、DB には照合用ハッシュ値のみを保持する
- Bearerトークンは登録済みユーザに紐付け、対象ユーザ削除時には利用不能化される
- Basic認証は Bearer認証と同じ認証入口から受け付けるが、権限制御上は全スコープを持つものとして扱う

### 3.3 本書で扱う設計粒度

- `docs/BASE_DESIGN.md` では触れない Bearer認証固有の内部データと処理フローを対象とする
- 実装クラス名や関数名は固定しないが、責務分割と入出力の境界は明示する
- 外部仕様で確定済みの値は原則として再決定せず、内部設計への落とし込み方法を記述する

### 3.4 仕様トレーサビリティの記載方針

- 各章の冒頭に、その章が主に対応する外部仕様を `関連仕様` として明記する
- 要求仕様の根拠は `docs/REQUIREMENTS.md`、API外部仕様の根拠は `docs/REST_API_SPECS.md` と `docs/openapi.yaml`、CLI外部仕様の根拠は `docs/CLI_SPECS.md` を正とする
- 章末の整合確認結果では、章ごとの参照先を横断的に確認できるよう一覧化する

## 4. 設計方針

- 既存の Basic認証実装へ過度な抽象化を持ち込まず、Basic / Bearer 共通入口と方式別検証処理を分離する
- redb 上の保存構造は照合処理に必要な最小構成とし、一覧系は逐次走査を前提とする
- スコープは拡張可能性を確保しつつ、`read` / `write` / `create` / `update` / `append` / `delete` に対応する
- スライディング期限は保持済みの TTL と有効期限から判定し、不要な更新項目は増やさない
- 認証失敗と認可失敗を分離し、HTTP ステータスコードの責務を明確化する
- path prefix 制約とユーザ属性による認可入口の差分は、認証文脈と共通認可ガード側へ集約する

## 5. 章構成

本書は以下の章で構成する。

### 5.1 データモデル設計

本章では以下を記述対象とする。

- Bearerトークン管理情報の保持項目
- redb 上の主テーブル設計
- `token_id` 変換テーブル設計
- シリアライズ方針
- ユーザ情報との責務分離

関連仕様:

- `docs/REQUIREMENTS.md`
  - 11.5 Bearer認証
  - 11.6 Bearerトークンのスコープ
  - 11.7 ユーザ情報の管理
- `docs/CLI_SPECS.md`
  - `token create`
  - `token revoke`
  - `token purge`
  - `token list`

#### 5.1.1 Bearerトークン管理構造

Bearerトークン管理情報は、ユーザ情報とは独立した構造体として管理する。保存対象は要求仕様および設計インプットで確定した項目に限定し、認証時に毎回更新したくなる項目は安易に増やさない。

概念上の管理構造は以下を基本とする。

```rust
struct BearerTokenInfo {
    /// CLIおよび運用上の管理用識別子
    token_id: TokenId,

    /// 発行対象ユーザ
    user_id: UserId,

    /// Bearerトークンに付与されたスコープ集合
    scopes: BearerScopeSet,

    /// Bearerトークンに付与された path prefix 制約
    path_prefixes: PathPrefixSet,

    /// 作成日時
    created_at: Timestamp,

    /// 最終更新日時
    updated_at: Timestamp,

    /// トークンごとのTTL
    ttl: TokenTtl,

    /// 現在の有効期限
    expire_at: Timestamp,

    /// CLI失効操作による失効状態
    revoked: bool,

    /// 任意のトークン名
    name: Option<String>,
}
```

上記は設計上の責務を示す概念モデルであり、実装時の型名は固定しない。

#### 5.1.2 保持項目の責務

- `token_id`
  - 認証には使用しない
  - CLI の指定、一覧表示、失効、削除、ログ出力に用いる
  - 形式は ULID とする
  - ソースコード上では可読性確保のため `Id` の直接利用ではなく `TokenId` エイリアスを用いる
- `user_id`
  - Bearerトークンの発行対象ユーザを指す
  - 一覧表示時のユーザ名は、この `user_id` から最新のユーザ情報を解決して表示する
- `scopes`
  - Bearer認証成功時の認可判定に用いる
  - `read` / `write` / `create` / `update` / `append` / `delete` を保持対象とする
- `path_prefixes`
  - Bearer認証成功後の path 制約判定に用いる
  - 正規化済みの絶対パス集合として保持する
  - `/` を含む場合は全領域アクセス可として扱う
- `created_at`
  - 発行日時を表す
  - CLI の詳細表示および監査補助情報に用いる
- `updated_at`
  - 失効やTTL延長など、管理情報の更新が最後に行われた日時を表す
  - `last_used_at` の代替としては扱わず、あくまで管理レコードの更新時刻とする
- `ttl`
  - トークンごとの有効期間を保持する
  - スライディング期限の延長判定に必要な基準値として使用する
- `expire_at`
  - 現在の有効期限を表す
  - 認証時の期限切れ判定および TTL 延長判定に使用する
- `revoked`
  - CLI の `token revoke` による明示失効を表す
  - 期限切れとは別軸の状態として保持する
- `name`
  - 運用上の識別補助として保持する任意名称であり、省略可能とする

`last_used_at` は保持しない。認証成功のたびに更新が発生すると書き込み頻度が高くなり、初期実装に対して利益が小さいためである。

Bearerトークン平文は保持しない。また、照合用ハッシュ値は主テーブルのキーとして保持するため、管理構造体の値側へ重複保持しない。

#### 5.1.3 redb上の主テーブル設計

Bearer認証の照合処理で最初に参照する主テーブルは、照合用ハッシュ値をキーとし、Bearerトークン管理情報を値とする。

```text
Table<TokenHash, BearerTokenInfo>
```

- key: Bearerトークン平文から算出した SHA256 ハッシュ値
- value: Bearerトークン管理情報を MessagePack でシリアライズしたバイナリ
- 用途: Bearer認証時の照合、失効状態確認、期限確認、TTL延長判定、ユーザ解決の起点

この設計により、認証処理は Bearerトークン平文を受け取った後、同じハッシュ計算を行うだけで主テーブルから直接管理情報へ到達できる。

照合用ハッシュ値は固定長のバイト列であり、redb のキーとして自然に扱える。認証時の検索効率を優先し、主テーブルのキーには `token_id` ではなく照合用ハッシュ値を採用する。

#### 5.1.4 `token_id` 変換テーブル設計

CLI の `token revoke` / `token purge` / `token list` では、管理用識別子として `token_id` を扱う。このため、`token_id` から主テーブルのキーである照合用ハッシュ値へ変換する補助テーブルを別途設ける。

```text
Table<TokenId, TokenHash>
```

- key: `token_id`
- value: 主テーブルのキーと同一の照合用ハッシュ値
- 用途: CLI 指定の `token_id` から Bearerトークン管理情報を特定するための逆引き

`token_id` 変換テーブルは認証そのものには使用しない。あくまで CLI 管理操作と監査補助のための補助索引として扱う。

Bearerトークンの作成、失効、物理削除などで両テーブルを更新する場合は、同一トランザクションで整合的に更新する。

#### 5.1.5 一覧・抽出時の索引方針

Bearerトークン数は多くならない前提とし、初期実装では以下の方針を採用する。

- 認証処理に必要な索引は主テーブルと `token_id` 変換テーブルに限定する
- `token list`、`token revoke --user`、`token purge --expired`、`token purge --revoked` などの抽出は主テーブルの逐次走査で対応する
- ユーザ別、状態別、期限別の専用索引は追加しない

これにより、書き込み時の整合維持コストを抑えつつ、初期実装に必要な操作を満たす。

#### 5.1.6 シリアライズ方針

Bearerトークン管理情報の値は、既存の `PageIndex`、`PageSource`、`UserInfo` などと同様に、serde ベースで MessagePack へシリアライズして保存する。

- redb の値型は既存実装に合わせて MessagePack バイナリを採用する
- シリアライズ処理は `rmp_serde` による named field 形式を前提とする
- 将来の互換対応が必要になった場合は、既存データモデルと同様にデシリアライズ時の後方互換処理を個別に持てる構造とする

Bearer認証専用の保存形式を新設せず、既存データベース層と同じ流儀に揃えることで、保守コストと実装の一貫性を確保する。

#### 5.1.7 ユーザ情報との責務分離

Bearerトークン管理情報は `UserInfo` に内包しない。要求仕様どおり、ユーザ情報と Bearerトークン管理情報は別テーブルで独立管理する。

- ユーザの認証基盤である Basic認証用パスワード情報は `UserInfo` 側の責務とする
- Bearerトークンの発行、失効、期限、スコープ、任意名は Bearerトークン管理情報側の責務とする
- Bearerトークンの path prefix 制約も Bearerトークン管理情報側の責務とする
- 一覧表示時のユーザ名は `user_id` から都度解決し、Bearerトークン側へユーザ名文字列を重複保存しない

ユーザ情報側には属性集合を保持し、少なくとも `NoBasicAuth` と `ReadOnly` を扱えるものとする。`NoBasicAuth` による Basic認証可否の判定、および `ReadOnly` による write 系操作禁止の基礎情報は `UserInfo` 側の責務とし、Bearerトークン管理情報へ重複保持しない。

`ReadOnly` は Bearerトークン側のスコープや path prefix 制約とは別軸の、ユーザ単位の上位認可制約として扱う。すなわち Bearerトークンが `write` または各書き込み系スコープを保持していても、対象ユーザが `ReadOnly` を持つ場合は write 系操作を許可しない。したがって `ReadOnly` は Bearerトークン管理情報の保持項目へ取り込まず、認証文脈と後段認可ガードで参照できるユーザ属性として扱う。

ユーザ削除時は、そのユーザに紐付く Bearerトークン管理情報と `token_id` 変換情報も同一処理単位で削除する。これにより、対象ユーザ削除完了時点で関連トークンを即時に利用不能とする。

#### 5.1.8 日時項目の内部表現

Bearerトークン管理情報が保持する日時項目は、既存のユーザ情報やロック情報と同様にローカルタイムの日時型で扱う。

- 内部保持: ローカルタイム
- 外部仕様で返す日時: ISO8601 のタイムゾーン無し表記

この章で扱う `Timestamp` は概念表現であり、実装では既存データモデルとの整合を優先して既存日時型に揃える。

### 5.2 認証フロー設計

本章では以下を記述対象とする。

- `Authorization` ヘッダ件数確認
- Basic / Bearer の分岐
- Bearerトークン照合とユーザ解決
- 認証成功時の共通認証文脈生成
- 認証失敗時の応答責務

関連仕様:

- `docs/REQUIREMENTS.md`
  - 11.5 Bearer認証
- `docs/REST_API_SPECS.md`
  - 共通事項 / 認証
  - 共通事項 / 認証失敗・認可失敗
- `docs/openapi.yaml`
  - `security`
  - `components.securitySchemes`
  - `components.responses.Unauthorized`
  - `components.responses.Forbidden`

#### 5.2.1 認証入口の基本方針

REST API の認証入口は Basic / Bearer 共通とし、`/api` 配下の全エンドポイントで同一の入口を通す。現行実装では Basic認証専用の検証関数が存在するが、Bearer対応後は以下の責務を持つ共通入口へ整理する。

- `Authorization` ヘッダ件数の検証
- 認証 scheme の判定
- Basic / Bearer それぞれの方式別検証関数の呼び出し
- 認証成功時の共通認証文脈の生成とリクエスト格納
- Bearer認証で TTL 延長が発生した場合のレスポンス後処理への引き継ぎ

今回の設計では、将来拡張を先取りした汎用認証フレームワーク化は行わない。共通入口と方式別検証関数の2段構成に留める。

#### 5.2.2 `Authorization` ヘッダ解析フロー

認証処理の先頭では、まず `Authorization` ヘッダの件数と形式を検証する。判定ルールは以下の通りとする。

1. `Authorization` ヘッダが 0 件の場合は 401 Unauthorized を返す
2. `Authorization` ヘッダが 2 件以上ある場合は 400 Bad Request を返す
3. `Authorization` ヘッダが 1 件だけ存在する場合に限り、scheme を解析する
4. scheme が `Basic` の場合は Basic認証検証へ進む
5. scheme が `Bearer` の場合は Bearer認証検証へ進む
6. 未対応 scheme またはヘッダ形式不正の場合は 400 Bad Request を返す

この判定は認証ミドルウェアで完結させ、後続ハンドラへ不正な `Authorization` ヘッダを持ち込まない。

#### 5.2.3 Basic認証フロー

Basic認証時の処理は、現行のユーザ認証基盤を利用して資格情報を検証する。

- `Authorization: Basic <credentials>` からユーザ名とパスワードを取り出す
- ユーザ名から `UserInfo` を解決し、Argon2 ハッシュでパスワードを検証する
- `NoBasicAuth` 属性を持つユーザである場合は 401 Unauthorized を返す
- 検証に成功した場合は、操作主体ユーザを共通認証文脈へ格納する
- Basic認証は Bearerスコープ制限を受けないため、共通認証文脈上は全スコープを保持しているものとして扱う

`ReadOnly` は Basic認証自体の拒否条件ではないため、認証入口では 401 Unauthorized の理由に含めない。Basic認証成功後に生成する共通認証文脈へ `ReadOnly` 判定に必要なユーザ属性情報を載せ、後段の共通認可ガードで write 系操作を拒否できる構成とする。

Basic認証では Bearerトークンに関するメタ情報を扱わないため、TTL延長判定や `X-Bearer-Expire` 付与対象にはならない。

#### 5.2.4 Bearer認証フロー

Bearer認証時の処理は、Bearerトークン平文の照合、トークン状態確認、対象ユーザ解決、TTL延長判定までを認証処理内で一貫して行う。

フローは以下の通りとする。

1. `Authorization: Bearer <token>` からトークン平文を抽出する
2. トークン平文から SHA256 による照合用ハッシュ値を算出する
3. 主テーブル `Table<TokenHash, BearerTokenInfo>` から管理情報を取得する
4. 管理情報が存在しない場合は 401 Unauthorized を返す
5. `revoked` が真の場合は 401 Unauthorized を返す
6. `expire_at` が現在時刻以前の場合は 401 Unauthorized を返す
7. `user_id` に対応するユーザ情報を解決する
8. 対象ユーザが存在しない場合は認証失敗として 401 Unauthorized を返す
9. 認証成功後、TTL 延長要件を満たすか判定する
10. 必要な場合のみ `expire_at` と `updated_at` を同一トランザクションで更新する
11. 共通認証文脈へ操作主体ユーザとトークンスコープ集合を格納する

Bearer認証成功時でも、トークンID、トークン平文、TTL延長有無などの Bearer 固有メタ情報は後続ハンドラへ無制限には公開しない。Bearer固有の更新やレスポンスヘッダ付与は認証処理側で閉じる。一方で、監査ログおよび path 制約違反記録のために `token_id` を認証文脈の補助情報として保持できる構成とする。

また、対象ユーザが `ReadOnly` を持つかどうかは Bearerスコープ解釈より優先して後段認可へ伝播する必要がある。このため Bearer認証成功時の共通認証文脈には、少なくとも write 系操作可否を導出できるだけのユーザ属性情報を含める。

#### 5.2.5 共通認証文脈

認証成功時にリクエストへ格納する認証文脈は、少なくとも以下を含む。

```rust
struct AuthContext {
    user: AuthUser,
    scopes: BearerScopeSet,
    path_prefixes: PathPrefixSet,
    user_attributes: UserAttributeSet,
    token_id: Option<TokenId>,
}
```

- `AuthUser`
  - 操作主体ユーザを表す
  - Bearer認証時はトークンから解決したユーザを設定する
  - Basic認証時は資格情報検証に成功したユーザを設定する
- `scopes`
  - Bearer認証時はトークンに付与されたスコープ集合を設定する
  - Basic認証時は全スコープを持つものとして設定する
- `path_prefixes`
  - Bearer認証時はトークンに付与された path prefix 制約を設定する
  - Basic認証時は全領域アクセス可を表す状態を設定する
- `user_attributes`
  - Basic認証時 / Bearer認証時のいずれでも、解決した `UserInfo` の属性集合を設定する
  - `NoBasicAuth` 自体は Basic認証入口で消費されるが、`ReadOnly` のような後段認可制約はこの情報を使って判定する
- `token_id`
  - Bearer認証時は監査ログ連携のため `Some(TokenId)` を設定できるようにする
  - Basic認証時は `None` とする

認証種別そのもの、更新前後の有効期限、TTL延長判定結果は認証文脈へ含めない。

`NoBasicAuth` と `ReadOnly` の責務差分はここで明確に分ける。`NoBasicAuth` は Basic認証入口で 401 判定に使用する属性であり、`ReadOnly` は認証成功後の write 系認可を抑止する属性である。したがって、両者は同じ属性集合に属していても、消費地点は同一ではない。

#### 5.2.6 認証失敗時の応答責務

認証失敗時のレスポンス生成は、Basic認証と Bearer認証のいずれも認証ミドルウェアで完結させる。

- 認証に必要なヘッダが存在しない
- 資格情報の形式が不正
- Basic認証の資格情報が不正
- `NoBasicAuth` 属性を持つユーザによる Basic認証
- Bearerトークンが未発行、失効済み、期限切れ、照合失敗
- Bearerトークンに紐付くユーザが解決できない

上記はいずれも認証失敗として扱い、後続ハンドラへ処理を委譲しない。

一方で、必要スコープ不足、path prefix 制約違反、`ReadOnly` 属性による write 系操作拒否はいずれも認証失敗ではなく認可失敗として別責務で扱う。

#### 5.2.7 Bearer認証とロック認証の順序

Bearer認証と `X-Lock-Authentication` によるロック解除トークン確認は独立した判定とする。判定順序は以下の通りとする。

1. `Authorization` ヘッダに対する Basic / Bearer 認証を完了する
2. Bearer認証時は必要スコープを満たすかを判定する
3. Bearer認証時は必要に応じて path prefix 制約を満たすかを判定する
4. ここまでを満たした後で、必要な場合のみ `X-Lock-Authentication` を検証する

この順序により、認証不成立またはスコープ不足のリクエストでロック解除トークン検証を先行させない。

#### 5.2.8 Bearer認証成功時の副作用

Bearer認証成功時の副作用は、TTL延長が発生した場合に限定する。

- TTL延長が不要な場合
  - 管理情報は更新しない
  - `X-Bearer-Expire` は付与しない
- TTL延長が必要な場合
  - `expire_at` と `updated_at` を更新する
  - レスポンス後処理で `X-Bearer-Expire` に更新後の有効期限を設定する

後続ハンドラは Bearer認証成功後の副作用を意識しない構成とし、通常の認証済みリクエストとして処理する。

### 5.3 スコープ判定設計

本章では以下を記述対象とする。

- スコープ列挙の内部表現
- スコープ集合型の責務
- `write` が各更新系スコープと `read` を包含する判定規則
- ハンドラ単位で必要スコープを定義する方針
- path prefix 制約判定との接続

関連仕様:

- `docs/REQUIREMENTS.md`
  - 11.6 Bearerトークンのスコープ
- `docs/REST_API_SPECS.md`
  - 共通事項 / 認証失敗・認可失敗
  - Bearer認証時の必要スコープを記載している各API定義
- `docs/openapi.yaml`
  - 各 operation の `x-required-scope`
  - `components.responses.Forbidden`

#### 5.3.1 スコープの内部表現

Bearerトークンのスコープ種別は、拡張可能な列挙として表現する。初期実装では `read` / `write` / `create` / `update` / `append` / `delete` を扱う。

概念上の表現は以下を基本とする。

```rust
enum BearerScope {
    Read,
    Write,
    Create,
    Update,
    Append,
    Delete,
}
```

実装時の列挙名やバリアント名は固定しないが、以下の責務を満たすことを前提とする。

- 外部仕様上のスコープ名と相互変換できる
- Bearerトークン管理情報へ保存できる
- 認証文脈へ格納できる
- 必要スコープ判定に利用できる

#### 5.3.2 スコープ集合型の責務

Bearerトークン管理情報および認証文脈では、単一スコープではなくスコープ集合型を扱う。これにより、保存、受け渡し、包含判定の責務を1か所へ集約する。

概念上の表現は以下を基本とする。

```rust
struct BearerScopeSet {
    scopes: BTreeSet<BearerScope>,
}
```

スコープ集合型は少なくとも以下の責務を持つ。

- Bearerトークンに付与されたスコープ群を保持する
- スコープ追加時の重複除去を行う
- 必要スコープを満たすかの包含判定を提供する
- Basic認証時に全スコープ相当を表現できる
- 将来スコープ追加時に判定ロジックの変更点を局所化する

認証文脈やハンドラは、生の配列や文字列集合ではなくこの集合型を通じてスコープを扱う。

#### 5.3.3 保存時の扱い

スコープ集合は保存時に自動正規化しない。付与されたスコープ内容をそのまま保持し、認可判定時に必要スコープを満たすか評価する。

たとえば `write` を持つトークンへ保存時に `read` を追加して冗長化することは行わない。この方針により、保存データは発行時指定内容をそのまま保持し、包含関係の知識は判定ロジック側へ閉じ込められる。

#### 5.3.4 `write` が各要求スコープを包含する判定規則

初期実装では、`write` は後方互換スコープとして `read` / `create` / `update` / `append` / `delete` を包含する。判定は保存時の正規化ではなく、スコープ集合型の包含判定インタフェースで扱う。

判定ルールは以下の通りとする。

- 要求スコープが `read` の場合
  - 付与スコープに `read` があれば許可する
  - 付与スコープに `write` があっても許可する
- 要求スコープが `create` / `update` / `append` / `delete` の場合
  - 対応する同名スコープがあれば許可する
  - 付与スコープに `write` があっても許可する
- 要求スコープが `write` の場合
  - 付与スコープに `write` がある場合のみ許可する
  - 分解済みスコープだけでは `write` を満たしたとは扱わない

この判定により、保存内容は単純な集合のまま保ちつつ、外部仕様で定義された `write` の上位互換性と、分解済みスコープの非包含性を両立できる。

#### 5.3.5 必要スコープ定義の配置方針

各 API の必要スコープは、ルーティング定義側で一律に固定せず、ハンドラ単位で明示する。これは `docs/REST_API_SPECS.md` に記載されたエンドポイントごとの要件と1対1で対応させるためである。

- 参照系ハンドラは `read` を要求する
- 作成系ハンドラは `create` を要求する
- 上書き更新および rename 系ハンドラは `update` を要求する
- 削除系ハンドラは `delete` を要求する
- Basic認証経由では全スコープを持つため常に通過可能とする

この方針により、各ハンドラの近傍に必要スコープが明示され、レビュー時に外部仕様との差分を追いやすくなる。

#### 5.3.6 判定ロジックの注入方法

実際のスコープ判定は、共通関数または共通ガードとしてハンドラへ注入する。各ハンドラがスコープ比較の詳細を個別実装しないようにする。

概念上は以下のような形を想定する。

```rust
fn require_scope(
    auth: &AuthContext,
    required: BearerScope,
) -> Result<(), HttpResponse>;
```

この共通判定は少なくとも以下を行う。

- 認証文脈からスコープ集合を取得する
- 必要スコープを満たすかを集合型インタフェースで判定する
- 満たさない場合は 403 Forbidden を生成する

Basic認証時は全スコープを保持している前提のため、この判定を常に通過する。

path prefix 制約判定も同様に共通関数または共通ガードとして提供し、Bearer認証時のみ適用する。

```rust
fn require_path_prefix(
    auth: &AuthContext,
    target_path: &NormalizedPath,
) -> Result<(), HttpResponse>;
```

#### 5.3.7 403 Forbidden の責務

スコープ不足は認証失敗ではなく認可失敗である。したがって、Bearerトークン自体の照合成功後に、必要スコープを満たさない場合のみ 403 Forbidden を返す。

- Bearerトークンが不正、失効済み、期限切れ、未発行
  - 401 Unauthorized
- Bearerトークンは有効だが必要スコープを満たさない
  - 403 Forbidden
- Bearerトークンは有効だが path prefix 制約を満たさない
  - 403 Forbidden

この責務分離により、認証エラーと認可エラーを混同せず、REST API 仕様のステータスコード定義と整合させる。

#### 5.3.8 APIごとの必要スコープの読み替え

`docs/REST_API_SPECS.md` で定義された各 API の「Bearer認証時の必要スコープ」は、実装上はハンドラごとの `required_scope` 指定へ読み替える。

- ページ・アセット・ロックの参照系 API は `read`
- ページ作成とアセット追加は `create`
- ページ更新と rename は `update`
- ページ削除とアセット削除は `delete`
- `GET /api/users/me` は `read`

個別 API の列挙を設計書内へ重複転記して管理し始めると同期漏れの原因になるため、詳細な対応表は `docs/REST_API_SPECS.md` を正とし、本章では実装反映の原則のみを定義する。

### 5.4 スライディング期限設計

本章では以下を記述対象とする。

- TTL と有効期限の保持方法
- 延長要否の判定規則
- `expire_at` と最終更新日時の更新規則
- `X-Bearer-Expire` 付与条件と付与責務

関連仕様:

- `docs/REQUIREMENTS.md`
  - 11.5 Bearer認証
- `docs/REST_API_SPECS.md`
  - 共通事項 / 認証
- `docs/CLI_SPECS.md`
  - `token create`
- `docs/openapi.yaml`
  - `info.description`

#### 5.4.1 基本方針

Bearerトークンは固定期限ではなく、スライディング期限を採用する。延長判定は Bearer認証成功時にのみ行い、認証失敗時や CLI 操作時には行わない。

- トークン発行時に `ttl` と `expire_at` を設定する
- Bearer認証成功時にのみ延長要否を判定する
- 判定条件を満たす場合のみ `expire_at` を更新する
- Basic認証時は本章の処理対象外とする

#### 5.4.2 保持項目の役割

スライディング期限判定に使用する保持項目は以下の2つを基本とする。

- `ttl`
  - トークンごとの有効期間
  - CLI の `token create --ttl` で指定した期間を保持する
- `expire_at`
  - 現在有効な期限
  - 発行時または直近延長時点から `ttl` を加算した時刻を表す

専用の `last_used_at` や `last_extended_at` は保持しない。基準時刻は既存保持項目から導出する。

#### 5.4.3 基準時刻の導出

TTL 延長判定に必要な「発行時刻または直近延長時刻に相当する基準時刻」は、以下の式で導出する。

```text
base_time = expire_at - ttl
```

この `base_time` は、以下のいずれかに対応する。

- 発行直後でまだ一度も延長されていない場合
  - 発行時刻
- 過去に延長が行われた場合
  - 直近延長時刻

これにより、専用項目を追加せずにスライディング期限の基準時刻を再構築できる。

#### 5.4.4 延長要否の判定規則

延長判定は Bearer認証成功後に現在時刻を用いて行う。判定ルールは以下の通りとする。

```text
elapsed = now - base_time
threshold = ttl / 2
```

- `elapsed` が `threshold` 未満の場合
  - 延長しない
- `elapsed` が `threshold` 以上の場合
  - 延長する

延長時の新しい有効期限は以下で求める。

```text
new_expire_at = now + ttl
```

この判定は認証成功時にのみ実行する。期限切れ判定はこの前段で行い、すでに `expire_at <= now` の場合は認証失敗として 401 Unauthorized を返す。

#### 5.4.5 `last_used_at` 非採用方針

`last_used_at` は保持しない。理由は以下の通りとする。

- 認証成功のたびに更新が走ると Bearerトークン管理レコードの書き込み頻度が高くなる
- 初期実装では、期限延長が発生したときだけ更新が必要な構造に留めた方が簡潔である
- スライディング期限の判定は `ttl` と `expire_at` から十分に導出できる

そのため、管理レコード上の時刻更新は `updated_at` に一本化し、これは管理情報の最終更新時刻としてのみ扱う。

#### 5.4.6 `expire_at` と `updated_at` の更新規則

TTL延長が発生した場合は、`expire_at` と `updated_at` を同一トランザクションで更新する。

- `expire_at`
  - `now + ttl` に更新する
- `updated_at`
  - 同じ `now` に更新する

これにより、期限情報と管理レコード更新時刻の不整合を防ぐ。

TTL延長が発生しなかった場合は、`expire_at` も `updated_at` も更新しない。単なる認証成功を記録するための書き込みは行わない。

#### 5.4.7 時刻取得と比較の責務

延長判定に使う現在時刻 `now` は Bearer認証処理の中で取得する。認証処理内で同一の `now` を用いて、少なくとも以下の比較と更新を行う。

- 期限切れ判定
- 延長要否判定
- 延長時の `expire_at` 計算
- 延長時の `updated_at` 設定

同一認証処理内で基準時刻がぶれないよう、複数回 `Local::now()` を取り直すのではなく、1回取得した時刻を使い回す前提とする。

#### 5.4.8 `X-Bearer-Expire` 付与条件

`X-Bearer-Expire` は Bearer認証で TTL 延長が実際に発生した場合にのみ付与する。

- Bearer認証で延長が発生した場合
  - 付与する
- Bearer認証で延長が発生しなかった場合
  - 付与しない
- Basic認証の場合
  - 付与しない

ヘッダ値には、更新後の `expire_at` を外部仕様に合わせた ISO8601 のタイムゾーン無し表記で設定する。

#### 5.4.9 `X-Bearer-Expire` の付与責務

`X-Bearer-Expire` の付与は認証ミドルウェアのレスポンス後処理で行う。後続ハンドラでは付与有無や値の設定を扱わない。

この責務分離により、各 API ハンドラは通常のレスポンス生成に専念でき、Bearer認証特有の副作用を個別に意識しなくてよい。

#### 5.4.10 認証フローとの接続点

スライディング期限処理は、5.2 で定義した Bearer認証フローの中で以下の位置に入る。

1. トークン照合
2. 失効・期限切れ・ユーザ存在確認
3. 認証成功確定
4. TTL延長要否判定
5. 必要時のみ `expire_at` / `updated_at` 更新
6. 必要時のみ `X-Bearer-Expire` 付与情報をレスポンス後処理へ渡す
7. 共通認証文脈を後続処理で利用する

この順序により、延長判定は「認証成功を伴うアクセス」に対してのみ実行されるという要求仕様を満たす。

### 5.5 CLI整合・運用設計

本章では以下を記述対象とする。

- `token create` / `token revoke` / `token purge` / `token list` と内部保持項目の対応
- `token add_path` / `token remove_path` と内部保持項目の対応
- ログ出力方針
- 期限切れトークンの清掃方針
- ユーザ削除時の Bearerトークン連動
- 暫定運用事項として残す `token list` 表示仕様の扱い

関連仕様:

- `docs/REQUIREMENTS.md`
  - 11.5 Bearer認証
  - 11.7 ユーザ情報の管理
- `docs/CLI_SPECS.md`
  - `token create`
  - `token add_path`
  - `token remove_path`
  - `token revoke`
  - `token purge`
  - `token list`

#### 5.5.1 基本方針

Bearerトークンの作成、失効、削除、一覧表示は CLI のみから行う。REST API は Bearerトークンを利用する側であり、管理操作の公開入口にはしない。

CLI 管理操作は、Bearerトークン平文の再表示や自動清掃のような過剰機能を持たせず、運用上必要な最小管理機能に限定する。

#### 5.5.2 `token create` と内部保持項目の対応

`token create` は、登録済みユーザに対して新しい Bearerトークンを発行し、DB へ管理情報を保存する。

- 入力
  - 対象ユーザ名
  - スコープ指定
  - path prefix 指定
  - TTL 指定
  - 任意名
- 内部生成
  - `token_id`
  - トークン平文
  - 照合用ハッシュ値
  - `created_at`
  - `updated_at`
  - `expire_at`

保存時の対応関係は以下の通りとする。

- CLI の対象ユーザ名
  - `user_id` へ解決して保存する
- `--scope`
  - `BearerScopeSet` として保存する
- `--path-prefix`
  - `PathPrefixSet` として保存する
- `--ttl`
  - `ttl` として保存する
- `--name`
  - `name` として保存する
- 発行時刻
  - `created_at` および初期 `updated_at` に同じ値を設定する
- 初期有効期限
  - `expire_at = created_at + ttl`

成功時に標準出力へ返す項目は、少なくとも以下と内部保持項目を対応付ける。

- `token_id`
- 対象ユーザ名
- 指定スコープ
- 実効権限
- path制約
- 作成日時
- 有効期限
- トークン文字列

トークン文字列の平文は発行時にのみ表示し、後から再表示しない。DB には保存せず、照合用ハッシュ値のみを保持する。

#### 5.5.2A `token add_path` / `token remove_path` と内部保持項目の対応

`token add_path` と `token remove_path` は Bearerトークン管理情報の `path_prefixes` のみを更新する。

- `token add_path`
  - 指定された正規化済み絶対パスを `path_prefixes` へ追加する
  - 追加後に包含関係がある場合は、より広い prefix へ縮約して保持してよい
- `token remove_path`
  - 指定された正規化済み絶対パスを `path_prefixes` から削除する
  - 削除後に prefix が空になった場合は、全領域アクセス可の状態へ戻る

いずれも更新時は `updated_at` を更新する。

#### 5.5.3 `token revoke` の状態変更ルール

`token revoke` は Bearerトークンの物理削除ではなく、失効状態への遷移を担当する。

- 対象トークンが有効な場合
  - `revoked = true` へ更新する
  - `updated_at` を更新する
- 対象トークンが既に失効済みの場合
  - 状態変更しない
  - 警告対象として扱う
- 対象トークンが期限切れの場合
  - 状態変更しない
  - 警告対象として扱う

この操作では主テーブル上の管理情報を更新するが、`token_id` 変換テーブルは維持する。失効後も一覧表示や `token purge` の対象特定に利用するためである。

成功時の CLI 出力では、少なくとも以下を区別できるようにする。

- 実際に失効状態へ変更した件数
- 既に失効済みまたは期限切れで警告対象となった件数

#### 5.5.4 `token purge` の物理削除ルール

`token purge` は Bearerトークン管理情報の物理削除を担当する。これは意図的な破壊的操作として扱い、監査上の残置を前提としない。

- `TOKEN-ID` 指定時
  - 対象1件を削除する
- `--expired` 指定時
  - 期限切れトークンを削除対象に含める
- `--revoked` 指定時
  - 失効済みトークンを削除対象に含める
- `--expired` と `--revoked` を併用した場合
  - 和集合を削除対象とする

物理削除時は、以下を同一トランザクションで削除する。

- 主テーブル上の Bearerトークン管理情報
- `token_id` 変換テーブル上の対応エントリ

これにより、管理情報と補助索引の不整合を防ぐ。

#### 5.5.5 `token list` の内部情報との対応

`token list` は Bearerトークンの現在の管理状態を表示するが、トークン平文は表示しない。

一覧表示の基礎情報は以下とする。

- `token_id`
  - 主識別子として表示する
- 実効権限表示
  - `scopes` から導出する
- path制約有無
  - `path_prefixes` が全領域アクセス可か否かから導出する
- 対象ユーザ名
  - `user_id` から最新のユーザ名を解決して表示する
- 有効期限
  - `expire_at` を表示する

詳細表示ではさらに以下を扱う。

- 作成日時
  - `created_at`
- 最終更新日時
  - `updated_at`
- 状態表示
  - `revoked` と `expire_at` から導出する
- 任意名
  - `name`

期限切れ判定は、コマンド実行時点の現在時刻と `expire_at` の比較で求める。専用の状態保持項目は追加しない。

#### 5.5.6 `token list` の暫定事項

`token list` の状態表示欄については、`docs/CLI_SPECS.md` の記述に従って設計を進める。

- path制約有無の表示位置
- `updated_at` をどの書式で見せるか

現時点では、実装が必要とする内部情報として以下を保持すれば十分とする。

- スコープ集合
- `revoked`
- `expire_at`
- `updated_at`

`last_used_at` は保持しないため、一覧表示にも含めない。

暫定事項の整理として、表示仕様上の論点と内部情報の対応は以下の通りとする。

- 状態表示欄
  - 実効権限 `r` / `c` / `d` / `u` / `a` は `scopes` から導出する
  - `write` を保持する場合は `rcdua` として表示する
  - path制約有無は `path_prefixes` から導出する
- 詳細表示
  - 最終更新日時表示は `updated_at` を用いる
  - 状態表示は `revoked` と `expire_at` から導出する
  - 期限切れ状態は保持項目ではなく、その場での判定結果として補助的に扱う

#### 5.5.7 `token list` 使用感評価の観点

`token list` の表示仕様は初回実装後に使用感評価を行い、CLI仕様として固定する前に以下の観点を確認する。

- 状態表示欄の判読性
  - `r` / `c` / `d` / `u` / `a` の並びで実効権限が即座に分かるか
  - `write` が `rcdua` として表示されることが誤解なく伝わるか
- 表示順と情報量
  - 実効権限表示、path制約有無、`token_id`、対象ユーザ名、有効期限の並びが運用上見やすいか
  - 一覧表示に `updated_at` を含めない判断で十分か
- 詳細表示の実用性
  - `updated_at` が「最終使用日時」ではなく「管理情報の最終更新日時」であることが誤解なく伝わるか
  - `revoked` の表示方法が期限切れ状態と混同されないか
  - 任意名を含めたときに列幅や視認性が破綻しないか
- フィルタリングとの整合
  - `--revoked` と `--expired` の和集合表示時に、各行の状態が見分けやすいか
  - ユーザ別フィルタリング時にも省略表示の意味が変わらないか

上記の評価結果により、状態表示欄の短縮表現、期限切れ表示の有無、`updated_at` の表示形式を確定する。

#### 5.5.8 一覧・抽出時のフィルタリング

`token list`、`token revoke --user`、`token purge --expired`、`token purge --revoked` などの抽出は、主テーブルの逐次走査で対応する。

- Bearerトークン数は多くならない前提とする
- ユーザ別や状態別の専用索引は追加しない
- フィルタ条件の評価は走査中に行う

これにより、索引追加による書き込み時の整合維持コストを避ける。

#### 5.5.9 ログ出力方針

Bearer認証および CLI 管理操作に関するログでは、トークン平文を出力しない。

- 認証ログ
  - 平文トークンは出力しない
  - 特定可能な場合は `token_id` を出力対象に含める
  - 失敗理由は、未発行、照合失敗、失効済み、期限切れ、ユーザ未解決などの粒度に留める
- 認可失敗
  - スコープ不足による 403 Forbidden は認証失敗ログの責務に含めない
  - path prefix 制約違反は認可失敗として扱い、監査ログ連携対象に含める
- CLI 管理操作
  - `token_id`、対象件数、操作種別を記録対象とする
  - 平文トークンは出力しない

#### 5.5.10 期限切れトークンの清掃方針

期限切れトークンは認証では利用不能として扱うが、自動削除は行わない。清掃は `token purge` のみで行う。

- 認証時
  - `expire_at <= now` なら利用不能として 401 Unauthorized を返す
- 保存上
  - 期限切れでもレコードは残る
- 清掃
  - 明示的な CLI の `token purge --expired` でのみ物理削除する

この方針により、自動バックグラウンド処理や起動時清掃を導入せず、運用上の制御点を CLI に集約する。

#### 5.5.11 ユーザ削除時の Bearerトークン連動

ユーザ削除時は、そのユーザに紐付く Bearerトークン管理情報も同時に削除する。

- 対象ユーザの `user_id` に紐付く主テーブル上のレコードを削除する
- 対応する `token_id` 変換テーブルのエントリも削除する
- 削除後は認証・一覧表示・CLI 指定のいずれからも到達できない状態にする

この処理により、対象ユーザ削除完了時点で関連トークンを即時に利用不能とする。

#### 5.5.12 ユーザ名変更時の表示

Bearerトークン管理情報は `user_id` に紐付けて保持し、ユーザ名文字列は重複保存しない。

そのため、ユーザ名変更後の `token list` では、保持済み `user_id` から最新のユーザ名を解決して表示する。これにより、トークン管理レコード自体の一括更新を不要とする。

### 5.6 エラー処理・テスト方針

本章では以下を記述対象とする。

- 400 / 401 / 403 の責務分担
- Bearer認証に関する主要異常系
- 主要テスト観点
- 外部仕様との整合確認観点

関連仕様:

- `docs/REQUIREMENTS.md`
  - 11.5 Bearer認証
  - 11.6 Bearerトークンのスコープ
- `docs/REST_API_SPECS.md`
  - 共通事項 / 認証失敗・認可失敗
  - 共通事項 / エラー時のレスポンス
  - Bearer認証時の必要スコープを記載している各API定義
- `docs/openapi.yaml`
  - `components.responses.Unauthorized`
  - `components.responses.Forbidden`

#### 5.6.1 基本方針

Bearer認証のエラー処理は、認証ミドルウェアが扱う領域と、認可・業務条件チェックを行うハンドラ側の領域を分離する。エラー応答は既存 REST API と同様に JSON 形式の `reason` フィールドを持つレスポンスへ統一する。

#### 5.6.2 ステータスコードの責務分担

認証・認可関連のステータスコードの責務は以下の通りとする。

- 400 Bad Request
  - `Authorization` ヘッダが複数件存在する
  - `Authorization` ヘッダの scheme が未対応
  - `Authorization` ヘッダ形式が不正
- 401 Unauthorized
  - `Authorization` ヘッダが存在しない
  - Basic認証の資格情報が不正
  - `NoBasicAuth` 属性を持つユーザによる Basic認証
  - Bearerトークンが未発行
  - Bearerトークンが失効済み
  - Bearerトークンが期限切れ
  - Bearerトークン照合に失敗した
  - Bearerトークンに紐付くユーザが解決できない
- 403 Forbidden
  - Bearer認証は成功したが必要スコープを満たさない
  - Bearer認証は成功したが path prefix 制約を満たさない
  - 認証済みだが業務条件を満たさない
  - 例: ロック取得者と異なるユーザによる更新
- 423 Locked
  - ロック状態そのものにより操作が禁止される

この責務分担は `docs/REST_API_SPECS.md` の共通事項に合わせる。

#### 5.6.3 エラー応答生成の責務

認証失敗に関するエラー応答生成は認証ミドルウェアで完結させる。後続ハンドラは、認証済みの文脈だけを前提に実装できる状態にする。

- ミドルウェアが担当するもの
  - `Authorization` ヘッダ件数・形式不正
  - Basic認証失敗
  - Bearer認証失敗
  - Bearer認証成功後の TTL 延長副作用の制御
- ハンドラまたは共通認可ガードが担当するもの
  - 必要スコープ不足
  - path prefix 制約違反
  - ロック認証や業務条件違反

#### 5.6.4 主要異常系

Bearer認証の主要異常系は少なくとも以下を含む。

- `Authorization` ヘッダが無い
- `Authorization` ヘッダが複数ある
- `Authorization` ヘッダの scheme が `Basic` / `Bearer` 以外
- `Authorization: Bearer` だがトークン文字列が欠落または形式不正
- 照合用ハッシュ値に一致する管理情報が存在しない
- `revoked = true`
- `expire_at <= now`
- `user_id` に対応するユーザ情報が存在しない
- Bearer認証は成功したが必要スコープ不足
- Bearer認証は成功したが path prefix 制約違反
- Bearer認証とロック解除トークン確認の両方が必要な操作で、ロック認証が別途失敗する

これらの異常系は、どこまでを 400 / 401 / 403 / 423 へ振り分けるかが分かるようにテスト可能な粒度で維持する。

#### 5.6.5 ログ上の扱い

認証失敗時のログは、平文トークンを含めず、運用上必要な範囲の失敗理由だけを残す。

- 出力しない情報
  - Bearerトークン平文
  - Base64 展開後の資格情報
- 出力対象に含めてよい情報
  - `token_id` が特定できる場合の `token_id`
  - 失敗理由の区分
  - ステータスコード種別

スコープ不足による 403 Forbidden は認可処理の失敗であり、認証失敗ログの責務へ混在させない。

#### 5.6.6 認証処理の主要テスト観点

認証ミドルウェアおよび認証入口に対するテスト観点は以下を基本とする。

- Basic認証成功で認証文脈が格納されること
- Bearer認証成功で認証文脈が格納されること
- `Authorization` ヘッダ 0 件で 401 になること
- `Authorization` ヘッダ複数件で 400 になること
- 未対応 scheme で 400 になること
- Basic認証失敗で 401 になること
- Bearerトークン未発行で 401 になること
- Bearerトークン失効済みで 401 になること
- Bearerトークン期限切れで 401 になること
- Bearerトークン照合失敗で 401 になること
- Bearerトークンの対象ユーザ削除済みで 401 になること

#### 5.6.7 認可・スコープ判定の主要テスト観点

スコープ判定およびハンドラ側認可に対するテスト観点は以下を基本とする。

- `read` 要求 API に `read` トークンでアクセスできること
- `read` 要求 API に `write` トークンでアクセスできること
- `create` 要求 API に `create` または `write` トークンでアクセスできること
- `update` 要求 API に `update` または `write` トークンでアクセスできること
- `delete` 要求 API に `delete` または `write` トークンでアクセスできること
- `append` 要求 API に `append` または `write` トークンでアクセスできること
- `read` のみでは `create` / `update` / `append` / `delete` 要求を通過できないこと
- path prefix 制約外の path では 403 になること
- Basic認証では `read` / `create` / `update` / `append` / `delete` の各要求を通過すること
- Bearer認証成功後でもロック条件違反があれば 403 または 423 が返ること

#### 5.6.8 TTL延長の主要テスト観点

スライディング期限に対するテスト観点は以下を基本とする。

- `ttl / 2` 未満のアクセスでは延長しないこと
- `ttl / 2` ちょうどで延長すること
- `ttl / 2` 超過時に延長すること
- 延長時に `expire_at` と `updated_at` が同一トランザクション相当で更新されること
- 延長時にのみ `X-Bearer-Expire` が付与されること
- 延長不要時は `X-Bearer-Expire` が付与されないこと
- Basic認証時は `X-Bearer-Expire` が付与されないこと

#### 5.6.9 CLI管理操作の主要テスト観点

CLI 管理操作に対するテスト観点は以下を基本とする。

- `token create` で管理情報が保存され、平文トークンが一度だけ表示されること
- `token create` で指定したスコープ、path制約、TTL、任意名が保持されること
- `token add_path` で path制約が追加されること
- `token remove_path` で path制約が削除されること
- `token revoke` で `revoked` が更新されること
- `token revoke` で既失効・期限切れ対象を警告扱いできること
- `token purge` で主テーブルと `token_id` 変換テーブルがともに削除されること
- `token list` で `user_id` から最新ユーザ名が解決されること
- `token list` で実効権限表示と path制約有無が正しく導出されること
- `token list --expired` と `token list --revoked` の条件評価が正しいこと

#### 5.6.10 ユーザ管理連動の主要テスト観点

ユーザ管理との連動に対するテスト観点は以下を基本とする。

- ユーザ削除時に関連する Bearerトークン管理情報が削除されること
- ユーザ削除後に対象トークンで認証できないこと
- ユーザ削除後に `token list` へ残存表示されないこと
- ユーザ名変更後に `token list` が最新ユーザ名を表示すること

#### 5.6.11 外部仕様との整合確認観点

設計と実装の確認では、少なくとも以下のトレーサビリティを確認対象とする。

- `docs/REQUIREMENTS.md`
  - TTL、スライディング期限、平文非保存、ユーザ削除連動、スコープ定義
- `docs/REST_API_SPECS.md`
  - Basic / Bearer 併用、401 / 403 / 423 の責務、`X-Bearer-Expire`
- `docs/CLI_SPECS.md`
  - `token create` / `revoke` / `purge` / `list` の出力と挙動
- `docs/openapi.yaml`
  - Bearer認証方式の入口定義

#### 5.6.12 テスト文書との切り分け

本章では主要観点の整理に留め、網羅的なケース一覧は別紙へ切り出す前提とする。詳細な観点一覧は `docs/BEARER_AUTH_TEST_VIEWPOINTS.md` で管理する。

## 6. 実装対象外事項

本設計で明示的に実装対象外とする事項を以下に示す。

- Bearerトークンのリフレッシュトークン化
- Bearerトークン平文の再表示機能
- Bearerトークン管理 REST API の追加
- Bearer認証専用の複雑な索引追加
- 将来拡張を見越した過度な認証抽象化

## 7. 未確定事項の扱い

- `token list` の表示仕様は `docs/CLI_SPECS.md` に従って実装設計を進める
- `updated_at` の見せ方など使用感評価に依存する部分は暫定事項として扱い、見直し可能とする
- それ以外の主要論点は `docs/BEARER_AUTH_DESIGN_INPUT_TASKS.md` の確定事項に従う

## 8. 整合確認結果

本書について、以下の観点で横断確認を実施した。

- 用語
  - `token_id` は管理用識別子、認証用秘密値はトークン平文、保存上の照合情報は照合用ハッシュ値として用語を統一した
  - ソースコード上の型名としては `TokenId` エイリアスを用いる方針で統一した
- 時刻表現
  - 内部保持時刻はローカルタイム
  - 外部仕様へ返す日時は ISO8601 のタイムゾーン無し表記
  - `last_used_at` は非採用、管理更新時刻は `updated_at` に統一した
- 管理項目名
  - Bearerトークン管理情報の主要項目は `token_id` / `user_id` / `scopes` / `path_prefixes` / `created_at` / `updated_at` / `ttl` / `expire_at` / `revoked` / `name` で統一した
  - `X-Bearer-Expire` は TTL 延長発生時のみ認証ミドルウェアが付与する方針で統一した
- 外部仕様整合
  - `docs/REQUIREMENTS.md` の Bearer認証要件、`docs/REST_API_SPECS.md` の認証共通事項、`docs/CLI_SPECS.md` の token 管理操作、`docs/openapi.yaml` の Bearer 認証入口と矛盾がないことを確認した

### 8.1 章ごとの仕様トレーサビリティ

- 5.1 データモデル設計
  - `docs/REQUIREMENTS.md` 11.5, 11.6, 11.7
  - `docs/CLI_SPECS.md` `token create`, `token revoke`, `token purge`, `token list`
- 5.2 認証フロー設計
  - `docs/REQUIREMENTS.md` 11.5
  - `docs/REST_API_SPECS.md` 共通事項 / 認証、認証失敗・認可失敗
  - `docs/openapi.yaml` `security`, `components.securitySchemes`, `components.responses.Unauthorized`, `components.responses.Forbidden`
- 5.3 スコープ判定設計
  - `docs/REQUIREMENTS.md` 11.6
  - `docs/REST_API_SPECS.md` Bearer認証時の必要スコープを記載している各API定義
  - `docs/openapi.yaml` 各 operation の `x-required-scope`
- 5.4 スライディング期限設計
  - `docs/REQUIREMENTS.md` 11.5
  - `docs/REST_API_SPECS.md` 共通事項 / 認証
  - `docs/CLI_SPECS.md` `token create`
  - `docs/openapi.yaml` `info.description`
- 5.5 CLI整合・運用設計
  - `docs/REQUIREMENTS.md` 11.5, 11.7
  - `docs/CLI_SPECS.md` `token create`, `token revoke`, `token purge`, `token list`
- 5.6 エラー処理・テスト方針
  - `docs/REQUIREMENTS.md` 11.5, 11.6
  - `docs/REST_API_SPECS.md` 共通事項 / 認証失敗・認可失敗、エラー時のレスポンス
  - `docs/openapi.yaml` `components.responses.Unauthorized`, `components.responses.Forbidden`

### 8.2 追加仕様補足の再判定結果

Bearer認証設計書ドラフト、`token list` の暫定事項整理、およびテスト観点別紙の作成を踏まえて、`docs/REQUIREMENTS.md` と `docs/CLI_SPECS.md` への追加補足要否を再判定した。

判定結果は以下の通りとする。

- 現時点では追加の仕様補足は不要
- 実装着手に必要な主要論点は、既存仕様と本設計書群で吸収できている

再判定時に確認した論点は以下の通りである。

- 要求仕様側
  - Bearer認証の目的、CLI限定管理、TTL、スライディング期限、平文非保存、保持項目、スコープ、ユーザ情報分離は `docs/REQUIREMENTS.md` 11.5 から 11.7 で充足している
- CLI仕様側
  - `token create` / `token revoke` / `token purge` / `token list` の入出力、エラー条件、暫定表示要件は `docs/CLI_SPECS.md` に既に定義されている
- 設計書側
  - 内部データモデル、認証フロー、スコープ判定、TTL延長、CLI整合、エラー処理、テスト観点は `docs/BEARER_AUTH_DESIGN.md` と `docs/BEARER_AUTH_TEST_VIEWPOINTS.md` に整理済みである

なお、引き続き未確定として扱うのは `token list` の状態表示欄の具体的な短縮表現や `updated_at` の見せ方であり、これは仕様欠落ではなく使用感評価で確定する運用上の暫定事項と位置付ける。

本章までで `5.1` から `5.5` までの設計書整備範囲は完了とし、以後は実装着手と、その過程で必要になった具体的な表示体裁やテストケース詳細化を進める。
