# MCP内部設計

本書は、MCPサーバ機能の内部設計を整理するための文書である。

本書では、`docs/REQUIREMENTS.md`、`docs/MCP_SPEC_DECISION_TASKS.md`、
`docs/MCP_DESIGN_INPUT_TASKS.md`、`docs/BEARER_AUTH_DESIGN.md` を前提とし、
MCP機能の責務配置、公開面、認証・認可、監査ログ、path ベースサービス層、
データモデル拡張、CLI / 設定 / 起動経路への反映方針を定義する。

本書は MCP 内部設計の共通部であり、
分冊後は「全体像、前提、責務境界、横断方針、関連仕様の参照入口」を担う。

詳細設計は以下の分冊を参照する。

- `docs/MCP_TOOL_SPECS.md`
  - MCPクライアント向けの正式なツール仕様、入力、出力、エラー契約
- `docs/MCP_ARCHITECTURE_DESIGN.md`
  - 責務配置、モジュール構成、既存 REST API 層との共通化方針
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - path ベースサービス層、永続化モデル拡張、DB API、更新系操作の共通化
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - MCP の公開条件、CLI / 設定反映、HTTP サーバ統合、transport / endpoint 構成
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - ツール一覧、入出力データモデル、エラー応答設計
- `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 監査ログの保存形式、集約、rotation、retention の詳細設計
- `docs/MCP_AUTHORIZATION_TEST_VIEWPOINTS.md`
  - 認証・認可のテスト観点を確認する場合に参照する
- `docs/MCP_APPEND_TEST_VIEWPOINTS.md`
  - `append` の競合制御および amend 挙動のテスト観点を確認する場合に参照する
- `docs/MCP_AUDIT_LOG_TEST_VIEWPOINTS.md`
  - 監査ログ基盤のテスト観点を確認する場合に参照する
- `docs/MCP_CLI_TEST_VIEWPOINTS.md`
  - CLI 拡張のテスト観点を確認する場合に参照する
- `docs/MCP_REGRESSION_TEST_SCOPE.md`
  - 既存機能への回帰確認範囲を確認する場合に参照する

読み進める際は、まず本書で対象範囲、設計前提、責務分割、横断責務を確認し、
必要な実装論点に応じて各分冊へ進む。
また、実装時のテスト実装や回帰確認の入口としては、上記のテスト観点書群を併せて参照する。

## 0. 文書配置と章立て

MCP 実装設計の文書配置は、
「共通部 1 冊」と「論点別の分冊群」で構成する。

- 共通部
  - `docs/MCP_INTERNAL_DESIGN.md`
  - 対象範囲、設計前提、全体構成、責務分割、横断責務、関連仕様への参照入口を保持する
- 分冊
  - `docs/MCP_TOOL_SPECS.md`
  - `docs/MCP_ARCHITECTURE_DESIGN.md`
  - `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 各論点の詳細設計と判断結果を保持する
- 補助資料
  - `docs/MCP_AUTHORIZATION_TEST_VIEWPOINTS.md`
  - `docs/MCP_APPEND_TEST_VIEWPOINTS.md`
  - `docs/MCP_AUDIT_LOG_TEST_VIEWPOINTS.md`
  - `docs/MCP_CLI_TEST_VIEWPOINTS.md`
  - `docs/MCP_REGRESSION_TEST_SCOPE.md`
  - テスト実装および回帰確認の入口として扱う

本タスクリストとの対応は以下の通りとする。

| タスクリスト | 配置先 | 章立て |
|:--|:--|:--|
| 4.2 全体アーキテクチャ設計 | `docs/MCP_ARCHITECTURE_DESIGN.md` | 2. 責務配置に関する設計判断 / 3. モジュール構成案 / 4. 既存 REST API 層との共通化方針 |
| 4.3 MCP 公開面の設計 | `docs/MCP_TOOL_SPECS.md`、`docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`、`docs/MCP_INTERFACE_AND_ERROR_DESIGN.md` | 1. 共通事項 / 2. ツール仕様 / 2. MCP の公開条件と起動条件 / 3. MCP の transport / endpoint 構成 / 2. MCP のツール一覧と各ツールの責務 / 3. MCP の入出力データモデル / 4. MCP エラー応答と内部エラー分類の対応 |
| 4.4 認証・認可設計 | `docs/MCP_INTERNAL_DESIGN.md`、`docs/MCP_SERVICE_AND_STORAGE_DESIGN.md` | 5.1 認証・認可 / 2. path ベースサービス層の橋渡し設計 / 2.4 Bearerトークン管理情報の拡張設計 / 2.5 ユーザ属性モデルの拡張設計 |
| 4.5 監査ログ設計 | `docs/MCP_INTERNAL_DESIGN.md`、`docs/MCP_AUDIT_LOG_DESIGN.md` | 5.2 監査ログ / 2. 責務配置 / 3. 監査ログレコードモデル / 4. `append` 集約ロジックの状態管理 / 5. JSONL 保存形式とファイル分割ルール / 6. 保持期間超過ログの自動削除 / 7. `tracing` と既存アクセスログとの役割分担 |
| 4.6 path ベースサービス層とデータモデル設計 | `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`、`docs/MCP_INTERFACE_AND_ERROR_DESIGN.md` | 2. path ベースサービス層の橋渡し設計 / 2.6 サービス層の入出力モデル / 2.7 追加が必要な DB API / 3. MCP の入出力データモデル |
| 4.7 永続化・DBアクセス・運用値設計 | `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`、`docs/MCP_AUDIT_LOG_DESIGN.md`、`docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md` | 2.4 Bearerトークン管理情報の拡張設計 / 2.5 ユーザ属性モデルの拡張設計 / 2.7 追加が必要な DB API / 5. JSONL 保存形式とファイル分割ルール / 6. 保持期間超過ログの自動削除 / 2.3 CLI と設定ファイルの優先順位 |
| 4.8 CLI / 設定 / 起動経路設計 | `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md` | 2. MCP の公開条件と起動条件 / 3. MCP の transport / endpoint 構成 |
| 4.9 テスト設計 | テスト観点書群 | 認可、`append`、監査ログ、CLI、回帰確認の各別紙 |
| 4.10 文書化とトレーサビリティ | `docs/MCP_INTERNAL_DESIGN.md`、本タスクリスト、必要に応じた別紙 | 共通部の参照入口、完了記録、後続タスクへの引き渡し整理 |

以後、本書は「共通前提と参照入口」を保持するハブとして扱い、
各論点の本文追記は原則として上記の配置先へ集約する。

## 1. 対象範囲

本書の対象範囲は、初期実装の MCP サーバ機能である。

- ページ参照
- ページ一覧
- ページ検索
- ページ作成
- ページ更新
- ページ追記
- ページリネーム
- セクション取得

以下は初期実装の対象外とし、本書でも詳細設計の対象に含めない。

- 削除済みページ参照
- restore
- アセット操作
- ロック操作

## 2. 設計前提

- MCP は `run` コマンドで明示的に有効化された場合のみ公開する
- MCP は path ベースで公開し、外部へ `page_id` を露出しない
- MCP は Bearer 認証前提とし、Basic 認証は MCP の認証方式として扱わない
- 認可は Bearer スコープと path prefix 制約の両方で判定する
- 既存永続化データとの互換性は原則維持する
- Bearerトークン管理情報については例外とし、必要な保存形式変更を許容する
- MCP は既存 REST API の内部呼び出しではなく、共通処理を利用する専用ハンドラ層として構成する
- `append` は MCP 専用操作として扱い、既存 REST API へは露出させない
- 監査ログは MCP ハンドラおよび関連する認可失敗経路を含めて記録対象とする

## 3. 実装対象外事項と将来拡張

初期実装で対象外とする事項と、
将来拡張余地として残す事項は分けて扱う。

対象外事項は「初期版では公開せず、既存ツールや補助引数にも混在させないもの」を指す。
将来拡張項目は「初期版では実装しないが、責務配置やデータモデル設計で拡張余地を阻害しないようにするもの」を指す。

### 3.1 初期実装の対象外事項

以下は初期実装の対象外とする。

- 削除済みページ参照
- restore
- アセット操作
- ロック操作

各対象外事項の扱いは以下の通りとする。

- 削除済みページ参照
  - 現在 path を解決できない通常 read 系要求では `not_found` として扱う
  - 削除済みページ専用の参照要求は初期版では提供しない
- restore
  - rename とは別操作として扱い、初期版では公開しない
  - rename 系入力に restore 相当の意味を持ち込まない
- アセット操作
  - MCP ツールとして公開しない
  - ページ path ベース設計の対象に含めない
- ロック操作
  - ロック取得・更新・解除・参照は公開しない
  - ただし内部実装では、競合検出や待機判定のため既存ロック基盤を再利用してよい

### 3.2 将来拡張項目

以下は将来拡張候補として扱う。

- 削除済みページ専用参照
- restore
- アセット操作
- ロック操作
- transport の Streaming / SSE / session 管理拡張
- 監査ログ基盤の他機能への横展開
- Bearer ユーザ属性の追加

これらは初期版の実装対象には含めないが、
以下の設計原則で拡張余地を確保する。

- 公開面はツール単位を分離し、対象外機能を既存ツールへ混在させない
- サービス層は path 解決、認可、更新系共通化を中心に責務を固定し、対象外機能専用 API を先行追加しない
- 監査ログ基盤は `src/mcp/` に閉じず、将来の横断利用に耐える配置とする
- ユーザ属性と Bearer 管理情報は列挙・集合の拡張を前提に保持する
- transport は初期版で stateless / non-streaming を採るが、将来拡張のため責務境界を adapter 層へ閉じ込める

### 3.3 詳細設計の参照先

対象外事項および将来拡張の個別詳細は、以下を参照する。

- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 初期版から除外するツール、および `unsupported` / `not_found` の公開面整理
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - 対象外機能の拒否方針、path 解決時の扱い、既存ロック基盤の内部利用
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - transport / endpoint の将来拡張余地
- `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 監査ログ基盤の将来横展開余地

## 4. 全体構成

MCP 機能は、既存 HTTP サーバおよびデータベース基盤の上に、
専用の公開層と path ベースサービス層を追加する構成とする。

責務の大枠は以下の 4 層へ分ける。

1. HTTPサーバ統合層
2. MCP公開層
3. 共通サービス層
4. 既存永続化・DBアクセス層

監査ログは横断的関心事として、MCP公開層および共通サービス層に接続する。

## 5. 責務分割

### 5.1 HTTPサーバ統合層

HTTPサーバ統合層は、既存の Actix ベース HTTP サーバへ
MCP 公開口を組み込む責務を持つ。

主な責務は以下とする。

- `run` コマンドおよび設定に基づく MCP 有効 / 無効の切り替え
- MCP transport / endpoint の公開有無の決定
- 共有状態 `AppState` など既存サーバ資源との接続
- サーバ起動時の補助初期化処理との統合

この層は、MCP の業務ロジックや path 解決ロジックを持たず、
公開口の有効化と依存注入に責務を限定する。

### 5.2 MCP公開層

MCP公開層は、MCP クライアントから見えるツール群と、
その入出力境界を担当する。

主な責務は以下とする。

- MCP のツール定義
- ツール入力の構文検証
- Bearer 認証入口との接続
- サービス層の呼び出し
- サービス層の結果を MCP 応答へ整形
- 認証失敗、認可失敗、入力不正、競合などのエラー応答整形
- 監査ログイベント生成の起点管理

この層では `page_id` を扱わず、外部入力は path ベースで統一する。
また、既存 REST API を内部 HTTP 呼び出しする構成は採らない。

### 5.3 共通サービス層

共通サービス層は、MCP の path ベース要求を
既存のページ管理処理へ橋渡しする責務を持つ。

主な責務は以下とする。

- path 正規化と path 妥当性検証
- path からページ実体および内部 ID を解決する処理
- Bearer スコープ判定と path prefix 制約判定
- list / search の prefix 検証と結果フィルタ
- create / update / append / rename の業務フロー制御
- `append` の amend 相当判定および競合制御
- 監査ログへ渡す業務結果情報の組み立て

この層は、MCP 固有の transport 表現を持たず、
HTTP や MCP の応答形式から独立した内部処理単位として設計する。

また、将来的に REST API 側の path ベース共通処理が必要になった場合に、
再利用の中心となる層として位置付ける。

### 5.4 既存永続化・DBアクセス層

既存永続化・DBアクセス層は、redb を用いた保存処理と
既存 `DatabaseManager` を中心とした低水準操作を担当する。

主な責務は以下とする。

- ページ、リビジョン、ユーザ、トークンなどの永続化
- Bearer トークン照合、TTL 延長、トークン管理情報の取得
- ページ読取・更新・削除・リネームなど既存 DB API の提供
- 必要に応じた path ベース支援 API や `append` 支援 API の追加

この層は、MCP 専用の公開仕様を意識せず、
永続化と低水準整合性の維持に責務を限定する。

## 6. 横断責務

### 6.1 認証・認可

認証は既存 Bearer 認証基盤を流用しつつ、
MCP で必要な認可判定を共通サービス層へ接続する。

- 認証そのものは既存 Bearer 認証処理の再利用を第一候補とする
- MCP 固有の認可単位は、共通サービス層で操作種別ごとに判定する
- path prefix 制約違反およびスコープ不足は、認可失敗として扱う

#### 5.1.1 MCP 用 Bearer 認証入口の責務

MCP 用 Bearer 認証入口は、既存 REST API の Bearer 認証設計を流用しつつ、
MCP 固有の transport 文脈を扱う薄い入口として設計する。

責務は、「REST API と共有する認証コア」と
「MCP 固有の入力文脈を扱う入口処理」に分ける。

共有する認証コアの責務は以下とする。

- Bearer トークン平文の抽出後に行う照合用ハッシュ値の計算
- Bearer トークン管理情報の取得
- 失効状態、期限切れ、対象ユーザ未解決の検証
- 認証成功時の TTL 延長判定と必要時の更新
- 認証成功時に利用できる共通認証文脈の生成
- 監査ログ連携用の `token_id` を認証文脈へ保持できる形で返すこと

MCP 固有入口の責務は以下とする。

- MCP transport から Bearer トークンを抽出すること
- Basic 認証を MCP の認証方式として受理しないこと
- transport レベルの入力不備と認証失敗を区別して応答へ写像すること
- 認証成功後に、MCP ハンドラへ共通認証文脈を引き渡すこと

MCP 用入口では、既存 REST API の `Authorization` ヘッダ多重指定や
Basic / Bearer 併用判定そのものをそのまま持ち込まない。
MCP 側では「Bearer トークンが提示されたか」「提示値が正しいか」を
入口で扱い、方式分岐や Bearer 固有の照合は共有認証コアへ委譲する。

MCP 用入口が受け取る入力文脈は、少なくとも以下を含めて扱える前提とする。

- Bearer トークン平文
- リモートアドレス
- transport 種別
- 要求された MCP ツール名

このうち Bearer トークン平文だけが認証成立の必須入力であり、
リモートアドレス、transport 種別、ツール名は監査ログや診断補助に用いる
付随文脈として扱う。

認証成功時に MCP ハンドラへ渡す認証文脈は、
REST API 側の共通認証文脈と同型を基本とし、少なくとも以下を含む。

- 操作主体ユーザ
- Bearer スコープ集合
- path prefix 制約集合
- `token_id`

MCP 用入口は、認証成功後も操作対象 path を見て認可判定しない。
操作種別ごとの required scope 判定および path prefix 制約判定は、
MCP ハンドラから共通サービス層を呼び出す過程で行う。

失敗の責務分離は以下の通りとする。

- Bearer トークン欠落
- Bearer トークン形式不正
- Bearer トークン照合失敗
- Bearer トークン失効
- Bearer トークン期限切れ
- Basic 認証の持ち込み

上記はすべて認証入口の失敗として扱い、
MCP ツール実行エラーへ混在させない。

一方で以下は認可または業務失敗として、認証入口の責務から分離する。

- 必要スコープ不足
- path prefix 制約違反
- 対象 path 不正
- 対象ページ不存在
- 競合、ロック待機失敗

この分離により、MCP 用 Bearer 認証入口は
「Bearer 資格の真正性確認と認証文脈生成」に責務を限定し、
MCP 固有の path ベース認可や監査ログ詳細判定は後段へ委ねる。

#### 5.1.2 分解後 Bearer スコープ体系の内部実装方針

MCP 用認可では、`read` / `write` / `create` / `update` / `append` / `delete`
の分解後スコープ体系を、そのまま内部実装へ反映する。

スコープ列挙は、外部仕様上の名前と 1 対 1 に対応する列挙として保持する。
概念上は以下を前提とする。

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

保存時の方針は以下とする。

- Bearer トークン管理情報にはスコープ集合をそのまま保存する
- `write` を指定された場合でも、保存時に `read` / `create` / `update` / `append` / `delete` へ展開しない
- `create` / `update` / `append` / `delete` を指定された場合でも、保存時に `read` を追加しない
- 入力値の重複除去は行うが、包含関係に基づく自動正規化は行わない

この方針により、DB に保持される値は CLI 発行時の指定内容と一致し、
`write` の後方互換性は判定ロジック側へ局所化できる。

判定時の方針は以下とする。

- required scope が `read` の場合
  - 付与スコープに `read` があれば許可する
  - 付与スコープに `write` があれば許可する
- required scope が `create` / `update` / `append` / `delete` の場合
  - 対応する同名スコープがあれば許可する
  - 付与スコープに `write` があれば許可する
- required scope が `write` の場合
  - 付与スコープに `write` がある場合のみ許可する
  - 分解済みスコープ群だけでは `write` を満たしたとは扱わない

この包含規則により、`write` は後方互換スコープとして維持しつつ、
分解済みスコープ同士の非包含性を保つ。

MCP 側の required scope はツール単位で固定し、
MCP ハンドラまたは共通サービス呼び出しの近傍で明示する。
初期実装の対応は以下とする。

- `get_page`
  - `read`
- `list_pages`
  - `read`
- `search_pages`
  - `read`
- `get_section`
  - `read`
- `create_page`
  - `create`
- `update_page`
  - `update`
- `append_page`
  - `append`
- `rename_page`
  - `update`

delete は Bearer スコープ体系には含めるが、
初期版 MCP ツールとしては公開しない。
そのため、本章では保存・判定の対象に含めつつ、
MCP 公開面の required scope 対応表には現れないものとして扱う。

Basic 認証は MCP では受理しないため、
MCP のスコープ判定は Bearer 認証成功時の文脈だけを前提に設計する。
ただし、内部実装としては REST API 側と共有しやすいよう、
スコープ集合型と包含判定 API は認証方式非依存で再利用可能にしてよい。

CLI 表示との対応方針は以下とする。

- `token create` の完了表示では、指定スコープと実効権限を分けて表示する
- `token list` の一覧表示では、`SCOPE` 欄に実効権限を 1 文字ずつ表示する
- 実効権限の並び順は `read`, `create`, `delete`, `update`, `append` とする
- `write` を保持する場合の `SCOPE` 欄は `rcdua` と表示する
- 分解済みスコープのみを保持する場合は、付与された実効権限だけを表示する

この表示方針により、保存値としての `write` を維持しつつ、
運用者には「そのトークンで何ができるか」を一覧で示せる。

MCP 設計として重要なのは、
サービス層および監査ログが「保存値としてのスコープ」と
「required scope 判定結果としての実効権限」を混同しないことである。
監査ログやエラー判定で参照するのは、常に required scope に対する判定結果とする。

#### 5.1.3 path prefix 制約の内部表現と判定 API

path prefix 制約は、Bearer トークンに付与される
「操作可能な path 範囲」を表す認可データとして扱う。
MCP と REST API の双方で同じ保持形式と判定 API を利用できる構成を前提とする。

内部表現は、正規化済み絶対パスの集合を保持する専用型とする。
概念上は以下を前提とする。

```rust
struct PathPrefixSet {
    prefixes: BTreeSet<NormalizedPath>,
}
```

ここで `NormalizedPath` は、
既存の page path 妥当性規則を満たした正規化済み絶対パスを表す値型とする。
path prefix 制約では文字列生値を直接持ち回さず、
検証済み path 型を経由して保持・判定する。

保持時の方針は以下とする。

- Bearer トークン管理情報には複数の path prefix を保持できる
- 保持対象は正規化済み絶対パスのみとする
- `/` は全領域アクセス可を表す特別値として扱う
- `/` が 1 件でも含まれる場合は、保持内容を `/` のみへ縮約してよい
- `/docs` と `/docs/spec` のような包含関係がある場合は、より広い側だけを保持してよい
- 重複 prefix は保持しない

この縮約は保存時または更新時に行ってよいが、
外部仕様上の意味を変えない範囲に限る。
縮約の目的は、判定コストと表示の複雑さを抑えることである。

全領域アクセス可の表現は、
「prefix 未設定」と「`/` を保持している状態」を同義として扱う。
ただし内部実装では、判定 API を単純化するため、
読み込み時点でいずれか一方の正規表現へ寄せてよい。
初期設計では、認証文脈上は `/` のみを保持する正規形へ寄せる方針とする。

判定 API は少なくとも以下の責務を持つ。

- target path が許可された prefix 群のいずれかに一致するかを返す
- `/` を全領域一致として扱う
- 単純な文字列前方一致ではなく、パス境界を考慮した prefix 判定を行う
- 認可失敗時に、どの target path に対する判定だったかを後続へ渡せる

概念上の API は以下を前提とする。

```rust
fn allows_path(
    prefixes: &PathPrefixSet,
    target_path: &NormalizedPath,
) -> bool;
```

境界考慮 prefix 判定の規則は以下とする。

- prefix が `/` の場合
  - 常に一致とする
- target path が prefix と完全一致する場合
  - 一致とする
- target path が `prefix + \"/\"` で始まる場合
  - 一致とする
- それ以外
  - 不一致とする

この規則により、
`/docs` は `/docs` および `/docs/spec` には一致するが、
`/docs2` や `/docs-spec` には一致しない。

MCP 設計上、判定対象は必ず path 正規化後の値とし、
未正規化入力に対して直接 path prefix 判定を行わない。
入力 path の検証と正規化は公開層または共通サービス層の入口で先行して行い、
path prefix 判定関数は正規化済み path 同士の比較だけに責務を限定する。

認証文脈との接続方針は以下とする。

- Bearer 認証成功時は、トークンに設定された `PathPrefixSet` を認証文脈へ格納する
- 全領域アクセス可トークンでは、認証文脈上も `/` のみを保持する正規形を用いる
- MCP ハンドラおよび共通サービス層は、認証文脈から `PathPrefixSet` を取得して判定する

MCP 側で必要になる判定 API は、単一 path 判定だけでは足りない。
そのため後続設計では、少なくとも以下の派生 API をこの基本 API の上に定義する。

- 単一 target path の許可判定
- rename 用の移動元 path / 移動先 path の両方判定
- list / search 用の要求 prefix 自体の許可判定
- list / search 結果の後段フィルタ判定

本節では、その前提となる保持形式と単体判定規則を確定対象とする。

#### 5.1.4 操作種別ごとの認可判定対象 path

MCP の path ベース認可では、
「入力された path をそのまま見る操作」と
「ページ実体を解決した後の current path を見る操作」を区別する。
要求仕様に従い、各操作種別ごとの判定対象 path は以下の通りとする。

- `get_page`
  - 判定対象は対象ページの current path とする
  - 入力 path からページ実体を解決した後、current path に対して `read` と path prefix 制約を判定する
  - 旧 path 指定で解決できなかった場合は認可判定へ進まず not found とする
- `get_section`
  - 判定対象は対象ページの current path とする
  - section 指定は path 制約判定対象に含めず、ページ解決後の current path に対して `read` を判定する
- `update_page`
  - 判定対象は対象ページの current path とする
  - 入力 path からページ実体を解決した後、current path に対して `update` と path prefix 制約を判定する
- `append_page`
  - 判定対象は対象ページの current path とする
  - 入力 path からページ実体を解決した後、current path に対して `append` と path prefix 制約を判定する
- `create_page`
  - 判定対象は新規作成要求の target path とする
  - 既存ページ解決前提ではなく、正規化済み target path に対して `create` と path prefix 制約を判定する
  - path 衝突は認可判定通過後の業務検証で扱う
- `rename_page`
  - 判定対象は移動元 current path と移動先 rename_to path の両方とする
  - 入力 path からページ実体を解決した後、current path を取得し、正規化済み rename_to path と合わせて双方に `update` と path prefix 制約を判定する
  - どちらか一方でも制約外であれば forbidden とする
- `list_pages`
  - prefix 指定なしの場合
    - 一覧結果の各 current path を後段フィルタ対象とする
    - required scope は `read` とする
  - prefix 指定ありの場合
    - 入力された要求 prefix 自体を先に path prefix 制約で判定する
    - その後、取得結果の各 current path を後段フィルタする
- `search_pages`
  - prefix 指定なしの場合
    - 検索結果の各 current path を後段フィルタ対象とする
    - required scope は `read` とする
  - prefix 指定ありの場合
    - 入力された要求 prefix 自体を先に path prefix 制約で判定する
    - その後、検索結果の各 current path を後段フィルタする

list / search における「要求 prefix 自体の判定」と
「結果 path の後段フィルタ」は、役割が異なるため両方を行う。

- 要求 prefix 自体の判定
  - 制約外領域を明示的に検索・列挙しようとする要求を早期拒否する
- 結果 path の後段フィルタ
  - prefix 未指定の場合や、検索結果に複数 path が混在する場合でも、許可範囲外の項目を返さない

認可判定の実行順序は、操作種別ごとに以下を原則とする。

1. 入力 path または prefix の妥当性検証と正規化
2. 必要に応じたページ実体の解決
3. required scope 判定
4. path prefix 制約判定
5. 業務検証および DB 操作

ただし create は 2 を持たず、list / search の prefix ありケースでは
2 より前に「要求 prefix 自体の制約判定」を行う。

この整理により、MCP の認可判定対象 path は以下の 4 類型へ集約できる。

- current path 判定
  - `get_page`, `get_section`, `update_page`, `append_page`
- target path 判定
  - `create_page`
- 複数 path 判定
  - `rename_page`
- 要求 prefix 判定と結果 path フィルタの併用
  - `list_pages`, `search_pages`

MCP の path ベース認可は、入力文字列ベースではなく、
最終的に公開仕様上の対象とみなす path に対して行う。
そのため、page_id 解決を内部で利用する場合でも、
認可基準そのものは current path / target path / requested prefix のいずれかで説明可能な形を維持する。

#### 5.1.5 `NoBasicAuth` / `ReadOnly` の反映箇所

`NoBasicAuth` および `ReadOnly` は MCP 専用属性ではなく、
ユーザ情報に付与される共通属性として設計する。
MCP 自体は Basic 認証を受理しないが、
MCP 機能と同時に導入される Bearer 前提運用を成立させるため、
ユーザ属性モデル、認証・認可、CLI 管理表示の 3 箇所で反映を要する。

ユーザ属性モデルへの反映方針は以下とする。

- `UserInfo` 側に、将来拡張可能な属性集合を保持する
- 初期実装で導入する属性は `NoBasicAuth` と `ReadOnly` とする
- Basic 認証可否は `UserInfo` 側の責務とし、Bearer トークン管理情報へ重複保持しない
- `ReadOnly` による write 系操作可否も `UserInfo` 側の責務とし、Bearer トークン管理情報へ重複保持しない
- export / import では、ユーザ属性をユーザ情報の一部として扱う

この方針により、
「どのユーザが Basic を使えないか」はトークン単位ではなく
ユーザ単位で一貫して説明できる。また、
「どのユーザが MCP の write 系操作を行えないか」も
同じくユーザ単位で一貫して説明できる。

Basic 認証拒否への反映方針は以下とする。

- REST API の Basic 認証入口では、資格情報検証後に `NoBasicAuth` を判定する
- `NoBasicAuth` を持つユーザが Basic 認証を試行した場合は 401 Unauthorized とする
- Bearer 認証時には `NoBasicAuth` を拒否条件として用いない
- MCP 認証入口では Basic 認証自体を受理しないため、`NoBasicAuth` 判定は行わない
- `ReadOnly` は MCP 認証入口で認証失敗理由には使わず、認証成功後の write 系認可で判定する
- `ReadOnly` は required scope および path prefix 制約と並ぶ上位認可制約として扱い、write 系操作では Bearer スコープより優先して拒否できるようにする

この整理により、`NoBasicAuth` は
「Basic 認証を禁止する共通ユーザ属性」であり、
MCP では直接判定しないが、
MCP 前提運用で使うユーザを UI / REST 側の Basic 経路から明示的に排除する役割を持つ。

一方で `ReadOnly` は
「write 系操作を禁止する共通ユーザ属性」であり、
MCP では read 系操作を妨げず、write 系操作に対してのみ
`forbidden` を返すための認可属性として扱う。

CLI 表示および管理操作への反映方針は以下とする。

- `user add` では属性指定を受け付けられるようにする
- `user edit` では属性の追加・削除・変更を扱えるようにする
- `user list` には属性表示を詰め込まず、現状の一覧責務を維持する
- `user info` を新設し、属性集合を含む完全表示の出口とする
- `token info` や `token list` にはユーザ属性を表示しない

この表示方針により、トークン管理情報とユーザ属性情報の責務が混線しない。
Bearer トークンの一覧や詳細はトークン自身の情報に専念し、
`NoBasicAuth` や `ReadOnly` を含むユーザ属性は user 系コマンドで確認する。

MCP 実装設計としての依存関係は以下の通りとする。

- 本書のユーザ属性モデル設計で保存形式を定義する
- 本書の CLI / 設定 / 起動経路設計で `user add` / `user edit` / `user info` への反映を具体化する
- 本書のテスト設計で Basic 拒否、ReadOnly による write 拒否、および CLI 表示の観点へ接続する

本節で確定するのは、`NoBasicAuth` を
「MCP で直接判定する属性」ではなく、
「MCP と同時導入される認証運用のために周辺経路へ反映する共通属性」
として扱うこと、ならびに `ReadOnly` を
「MCP の write 系操作で直接判定する共通認可属性」
として扱う責務境界である。

#### 5.1.6 認可失敗時の監査ログ連携点

MCP では、認証失敗は監査ログ対象に含めず、
認可失敗のみを監査ログ対象に含める。
そのため、監査ログ連携点は
「Bearer 認証が成功して `token_id` を取得済みであり、
その後の required scope 判定または path prefix 制約判定で失敗した地点」
として定義する。

認可失敗時に監査ログへ渡すべき最小情報は以下とする。

- `operation`
  - 実行しようとした MCP ツールに対応する外部操作種別
- `user_id`
  - 認証済みユーザ
- `token_id`
  - Bearer 認証で確定したトークン識別子
- `address`
  - transport 入口で取得したリモートアドレス
- `target_path`
  - 判定対象として扱った path
- `result`
  - `scope_denied` または `path_prefix_denied` 相当の内部結果分類
- `timestamp`
  - 認可失敗を確定した時刻
- `summary`
  - 必要スコープや制約違反理由を補足できるサーバ生成文言
- `revision`
  - 認可失敗時は `null`

`token_id` の扱いは以下とする。

- Bearer 認証が成功している限り、スコープ不足および path prefix 制約違反の両方で `token_id` を監査ログへ記録する
- MCP は Basic 認証を受理しないため、MCP の認可失敗ログで `token_id` が欠損するのは設計上は例外系に限る
- `token_id` を特定できない失敗は、認可失敗ではなく認証失敗または内部失敗として別責務で扱う

連携責務の分割は以下とする。

- MCP 認証入口
  - Bearer 認証成功時に `token_id` を認証文脈へ格納する
  - 認証失敗時は監査ログへ渡さない
- MCP ハンドラ
  - ツール名、入力 path、要求スコープなど監査ログ起票に必要な文脈を持つ
  - 共通サービス層または認可補助関数から返された認可失敗を監査イベントへ写像する起点となる
- 共通サービス層
  - どの path に対する判定で失敗したか
  - 失敗理由がスコープ不足か path prefix 制約違反か
  - rename や list / search のような複合判定で、どの判定対象が失敗したか
  - 上記を監査ログ向け詳細情報として返せるようにする

認可失敗の判定位置ごとの扱いは以下とする。

- 単一 path 判定操作
  - 失敗した target path をそのまま `target_path` として記録する
- rename
  - 移動元 current path と移動先 path のどちらで失敗したかを `summary` に含める
  - `target_path` には失敗した側の path を記録する
- list / search の要求 prefix 判定
  - 入力された要求 prefix を `target_path` として記録する
- list / search の結果後段フィルタ
  - 許可範囲外の結果は単に除外し、個別失敗としては監査ログへ起票しない

この区別により、
「制約外領域を要求した失敗」と
「許可結果だけを返すために内部的に除外した結果」を分離できる。
監査対象とするのは前者であり、後者は通常動作として扱う。

MCP ハンドラから監査ログ基盤へ渡す概念上の入力は、
少なくとも以下を含める前提とする。

```rust
struct AuditAuthorizationFailure {
    operation: AuditOperation,
    user_id: UserId,
    token_id: TokenId,
    address: Option<IpAddr>,
    target_path: Option<NormalizedPath>,
    result: AuthorizationFailureKind,
    summary: Option<String>,
}
```

ここで `AuthorizationFailureKind` は少なくとも以下を区別できるものとする。

- `ScopeDenied`
- `PathPrefixDenied`

外部の MCP エラー応答では、これらはいずれも `forbidden` へ統一してよい。
一方で監査ログでは内部原因を保持する。

この連携点設計により、
認可失敗は「認証済み文脈を持つ失敗」として一貫して監査でき、
特に path prefix 制約違反やスコープ不足で `token_id` を失わず記録できる。

### 6.2 監査ログ

監査ログの詳細設計は、
[MCP_AUDIT_LOG_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_AUDIT_LOG_DESIGN.md)
へ切り出す。

本書では、MCP と監査ログ基盤の接続点だけを保持する。

- MCP 公開層でイベント起点を捕捉し、共通サービス層で確定した業務結果と組み合わせて監査ログへ渡す
- 認証失敗は監査ログではなく既存ログ責務に残し、認可失敗は監査ログ対象に含める
- `append` は集約ルールを考慮した記録を行う
- 監査ログ基盤は `src/mcp/` 配下ではなく独立した `src/audit/` モジュールとして扱う

別紙 [MCP_AUDIT_LOG_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_AUDIT_LOG_DESIGN.md)
では、以下を定義する。

- 監査ログモジュールの責務配置
- 監査ログレコードモデル
- `append` 集約ロジックの状態管理
- JSONL 保存形式とファイル分割ルール
- 保持期間超過ログの自動削除
- `tracing` と既存アクセスログとの役割分担

## 7. 責務配置に関する設計判断

責務配置、モジュール構成、既存 REST API 層との共通化方針の詳細は、
[MCP_ARCHITECTURE_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_ARCHITECTURE_DESIGN.md)
を参照する。

本書では、5章の責務分割を全体方針として保持し、
具体的なモジュール境界や分離判断は分冊側へ委ねる。

## 8. モジュール構成案

`src/mcp/` の初期構成案、`src/http_server/` / `src/database/` /
`src/cmd_args/` への反映箇所、監査ログ基盤の配置方針は、
[MCP_ARCHITECTURE_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_ARCHITECTURE_DESIGN.md)
を参照する。

## 9. 既存 REST API 層との共通化方針

再利用対象、共通サービス層へ引き上げる対象、
MCP 専用に残す公開面、REST API 側へ残す HTTP 依存処理の切り分けは、
[MCP_ARCHITECTURE_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_ARCHITECTURE_DESIGN.md)
を参照する。

## 10. path ベースサービス層の橋渡し設計

path 妥当性検証、正規化、認可、`page_id` 解決、DB API 接続、
Bearer トークン管理情報とユーザ属性の保存モデル拡張は、
[MCP_SERVICE_AND_STORAGE_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_SERVICE_AND_STORAGE_DESIGN.md)
を参照する。

本書では、5章および 6章で定義した責務境界と横断方針を前提に、
path ベース要求を共通サービス層で吸収する位置付けのみを保持する。

## 11. 更新系操作の共通化単位

create / update / append / rename の共通入力モデル、
解決済みコンテキスト、共通出力モデル、`append` の内部位置付けは、
[MCP_SERVICE_AND_STORAGE_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_SERVICE_AND_STORAGE_DESIGN.md)
を参照する。

本書では、`append` を MCP 専用公開操作として維持しつつ、
内部では更新系ファミリの一部として扱う方針だけを保持する。

## 12. 関連仕様

本章は、MCP 内部設計を読む際の参照ハブである。

最初に確認すべき外部仕様・前提文書は以下とする。

- `docs/REQUIREMENTS.md`
  - MCP 機能の正式な要求仕様
- `docs/MCP_SPEC_DECISION_TASKS.md`
  - Bearer スコープ分解、path prefix 制約、`append`、`rename`、監査ログの決定根拠
- `docs/MCP_DESIGN_INPUT_TASKS.md`
  - 監査ログ詳細、CLI 表示、保持運用などの設計インプット整理結果
- `docs/BEARER_AUTH_DESIGN.md`
  - Bearer 認証、`token_id`、path prefix 制約、`NoBasicAuth` の既存設計
- `docs/BASE_DESIGN.md`
  - サーバ全体の責務配置と既存構成の前提

共通部から分冊へ進む際の参照先は以下とする。

- `docs/MCP_ARCHITECTURE_DESIGN.md`
  - 責務配置、モジュール構成、既存 REST API 層との共通化を確認したい場合
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - path ベースサービス層、永続化モデル、DB API、更新系共通化を確認したい場合
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - `run` コマンド、CLI / config、起動経路、transport / endpoint を確認したい場合
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - ツール一覧、入出力モデル、エラー応答を確認したい場合
- `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 監査ログの保存形式、集約、rotation、retention を確認したい場合

実装観点での推奨参照順は、まず本書の 1章から6章で全体前提を確認し、
次に必要な論点に応じて上記の分冊へ進む。

## 13. MCP の公開条件と起動条件

`run` コマンドでの MCP 有効化、CLI / config への反映、
HTTP サーバ起動時の設定伝播と監査ログのグローバル設定連動方針は、
[MCP_RUNTIME_AND_TRANSPORT_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md)
を参照する。

本書では、2章の設計前提として
「MCP は明示有効化時のみ公開する」方針だけを保持する。

## 14. MCP の transport / endpoint 構成

`/mcp` endpoint の構成、Actix への組み込み位置、
Streamable HTTP 前提の transport adapter 責務は、
[MCP_RUNTIME_AND_TRANSPORT_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md)
を参照する。

## 15. MCP のツール一覧と各ツールの責務

初期版で公開する `get_page`、`list_pages`、`search_pages`、
`create_page`、`update_page`、`append_page`、`rename_page`、
`get_page_section` の責務整理は、
[MCP_INTERFACE_AND_ERROR_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_INTERFACE_AND_ERROR_DESIGN.md)
を参照する。

## 16. MCP の入出力データモデル

path ベースの共通入力モデル、ツール別入出力、
`page_id` 非公開を前提とした応答モデルは、
[MCP_INTERFACE_AND_ERROR_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_INTERFACE_AND_ERROR_DESIGN.md)
を参照する。

## 17. MCP エラー応答と内部エラー分類の対応

公開エラー表現、内部エラー分類との写像、transport エラーと
ツール実行エラーの切り分けは、
[MCP_INTERFACE_AND_ERROR_DESIGN.md](/home/kgt/dlp/private/sandbox/luwiki/docs/MCP_INTERFACE_AND_ERROR_DESIGN.md)
を参照する。
