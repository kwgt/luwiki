# MCPインタフェース・エラー設計

本書は、MCP内部設計のうち、
ツール一覧、入出力データモデル、エラー応答と内部エラー分類の対応を整理するための文書である。

本書は、共通部である `docs/MCP_INTERNAL_DESIGN.md` を前提とし、
現行 `docs/MCP_INTERNAL_DESIGN.md` の以下の章を移設する受け皿として用いる。

- 14. MCP のツール一覧と各ツールの責務
- 15. MCP の入出力データモデル
- 16. MCP エラー応答と内部エラー分類の対応

関連する設計文書は以下の通り。

- `docs/MCP_ARCHITECTURE_DESIGN.md`
  - ツール入口をどのモジュール責務へ配置するかを確認する場合に参照する
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - 各ツールが呼び出す path 解決、認可、DB API、更新系共通モデルを確認する場合に参照する
- `docs/MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - transport / endpoint、Actix 組み込み、起動条件との接続を確認する場合に参照する
- `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 公開エラーと監査結果分類の接続、および監査ログ記録対象を確認する場合に参照する

---

## 1. 対象範囲

本書では以下を対象とする。

- 初期版で公開するツール一覧
- ツールごとの責務
- 共通入力モデルと共通出力モデル
- ツール別入力モデルと出力モデル
- エラー表現と内部エラー分類の対応

## 2. MCP のツール一覧と各ツールの責務

本章では、MCP のツール一覧と各ツールの責務として、
初期版 MCP で公開する操作をツール単位へ落とし込み、
各ツールの役割と境界を明確にする。

外部契約としての正式なツール仕様は `docs/MCP_TOOL_SPECS.md` に集約し、
本書では責務分割、入力出力モデル、エラー分類との対応関係を内部設計として扱う。

### 2.1 基本方針

初期版 MCP のツールは、
`docs/REQUIREMENTS.md` および `docs/MCP_SPEC_DECISION_TASKS.md` で確定した
対象機能を、そのまま path ベースの操作単位へ対応付ける。

ツール設計では以下を守る。

- 外部へ `page_id` を公開しない
- ツール名は path ベース操作であることが分かる粒度にする
- read 系、write 系、補助系を分ける
- 対象外機能をツールへ混在させない

### 2.2 初期版で公開するツール

初期版では、少なくとも以下のツールを公開対象とする。

- `get_page`
  - 指定 path のページ内容またはページ情報を取得する
- `get_page_toc`
  - 指定 path のページから見出し構造を取得する
- `list_pages`
  - 指定 prefix 配下のページ一覧を取得する
- `search_pages`
  - 全文検索または prefix 制約付き検索を実行する
- `create_page`
  - 指定 path に新規ページを作成する
- `update_page`
  - 指定 path のページ本文を上書き更新する
- `append_page`
  - 指定 path のページ末尾へ追記する
- `rename_page`
  - 指定 path のページを別 path へ移動する
- `get_page_section`
  - 指定 path のページから特定セクションを取得する

### 2.3 各ツールの責務

#### 2.3.1 `get_page`

- path でページを指定する
- 現在ページの取得を行う
- 認可判定は current path 基準で行う
- deleted / restore は扱わない

#### 2.3.2 `list_pages`

- prefix path を入力とする
- prefix 自体の認可判定を先に行う
- 許可された範囲の一覧結果を返す
- path 昇順固定の forward 相当ページングのみを扱う
- `cursor` は `prefix` 配下 path の境界値として扱う
- deleted ページ一覧は扱わない

#### 2.3.3 `get_page_toc`

- current path を入力とする
- read スコープと current path への認可を要求する
- ページ本文全体ではなく、見出し構造と各節規模だけを返す
- `get_page_section` の前段で使う軽量な補助取得出口とする

#### 2.3.4 `search_pages`

- 検索式を入力とする
- 任意で prefix 指定を受け付ける
- prefix 指定がある場合は prefix 自体の認可判定を行う
- path 制約外の結果は current path 解決後に後段フィルタする
- 検索結果は path ベースで返す
- cursor ページングは持たず、上位 `limit` 件取得として扱う

#### 2.3.5 `create_page`

- target path と初期本文を受け取る
- create スコープと target path への認可を要求する
- 作成後ページの path と revision 情報を返す

#### 2.3.6 `update_page`

- current path と更新本文を受け取る
- update スコープと current path への認可を要求する
- 全文上書き更新を行う

#### 2.3.7 `append_page`

- current path と追記文字列を受け取る
- append スコープと current path への認可を要求する
- 単純末尾追記のみを扱う
- amend 相当判定は内部処理で行う

#### 2.3.8 `rename_page`

- current path と rename_to path を受け取る
- update スコープを要求する
- current path と rename_to path の双方で認可判定を行う
- restore は扱わない

#### 2.3.9 `get_page_section`

- current path とセクション識別子を受け取る
- read スコープと current path への認可を要求する
- ページ全体取得とは別に、セクション単位の参照を提供する
- `get_page_toc` が返す `section_id` による指定を第一の機械向け経路とする
- 見出し文字列指定は人間向けの補助入口として扱う

### 2.4 セクション取得補助ツールの設計方針

`get_page_section` はトークン消費削減を意図した部分取得ツールだが、
見出し文字列だけに依存すると AI エージェント利用時の安定性が不足しやすい。

そのため、初期版では `get_page_toc` を補助ツールとして追加し、
以下の役割分担を採る。

- `get_page`
  - ページ全文取得
- `get_page_toc`
  - 見出し構造と各節規模の把握
- `get_page_section`
  - 特定セクション本文の取得

この分離により、
ページ全体を取得せずに対象節候補を確認し、
必要な節だけを追加取得できる流れを成立させる。

`get_page_toc` が返す `section_id` は、
DB へ永続保存する識別子ではなく、
Markdown 解析結果から動的生成する revision-local な識別子とする。

動的 `section_id` を採る理由は以下とする。

- セクション識別情報を既存ページ保存モデルへ追加しないため
- `revision` ごとの本文構造へ自然に追従できるため
- `get_page_toc` と `get_page_section` を同一 `revision` で連携させれば十分なため

初期案として、`section_id` は文書順番号ベースの文字列を採る。
形式は `s-001` のような単純な連番でよい。

また、`get_page_toc` では各節に `section_chars` を含める。
これはモデル依存トークン数ではなく、
`get_page_section.content` と同じ返却範囲の Unicode 文字数とする。

`section_chars` を返す理由は以下とする。

- AI エージェントが対象節の重さを概算できるようにするため
- ページ構造だけでは判別しにくい長文節を避けやすくするため
- モデル固有トークン数をサーバ側仕様へ持ち込まないため

### 2.5 見出し指定方式の設計方針

`get_page_section.section` は、
人間向け指定と機械向け指定の両方を受け付ける。

初期版では、少なくとも以下を許可する。

- `by=id`
  - `get_page_toc` が返した `section_id` を使う
- `by=title`
  - 見出し文字列を使う
- 文字列単独指定
  - `by=title` の省略形として扱う

設計上の優先順位は以下とする。

- 機械向け経路
  - `by=id`
- 人間向け補助経路
  - `by=title`

`by=title` は利便性のために残すが、
完全一致のみを許可し、
編集距離ベースの曖昧一致は初期版では採用しない。

理由は以下とする。

- 短い見出しでは曖昧一致が別節へ衝突しやすいため
- 誤った節を成功扱いで返す方が `not_found` より危険なため
- AI エージェント向けには `get_page_toc` と `by=id` の組み合わせの方が安定するため

同名見出しが複数ある場合、
`by=title` は一意に解決できない入力として `invalid_input` とする。

### 2.6 初期版から除外するツール

以下は初期版から除外し、ツールとして公開しない。

- deleted ページ参照
- restore
- アセット操作
- ロック操作
- テンプレート指定作成
- リンク先一覧取得
- 被リンク一覧取得

これらは将来拡張候補として扱い、
初期版のツール定義や入出力モデルへ混在させない。

### 2.7 ツール粒度の判断

初期版では、複数の操作を 1 つの汎用ツールへ詰め込まず、
操作ごとにツールを分ける。

この方針により、以下の利点がある。

- 必要スコープをツール単位で固定しやすい
- path prefix 制約判定対象を明示しやすい
- 監査ログの `operation` を外部操作粒度へ揃えやすい
- `append` を `update` と分離した要求仕様を自然に表現できる

### 2.8 ツール一覧と責務に関する設計判断

本章のツール設計では、以下を基本方針として採用する。

- 初期版の公開ツールは `get_page`、`get_page_toc`、`list_pages`、`search_pages`、`create_page`、`update_page`、`append_page`、`rename_page`、`get_page_section` とする
- ツールは path ベースで定義し、`page_id` は完全非公開とする
- `append_page` は `update_page` から独立したツールとする
- `rename_page` は restore と分離し、初期版では rename のみ提供する
- `get_page_toc` は `get_page_section` の前段で使う補助ツールとして追加する
- `get_page_section` の機械向け指定は `section_id` を基本とし、見出し文字列指定は補助入口として残す
- `list_pages` と `search_pages` のページング方式は同一化せず、`list_pages` は cursor 継続取得、`search_pages` は top-N 取得として分ける
- 初期版対象外機能はツール定義へ含めない

## 3. MCP の入出力データモデル

本章では、初期版 MCP ツールに共通する入出力データモデルを定義する。

ここで扱うのは transport レベルの HTTP / JSON-RPC 表現ではなく、
ツール入出力として外部へ見せる論理モデルである。

### 3.1 基本方針

- すべてのツール入力は path ベースで表現する
- `page_id`、内部 lock token、DB 内部状態は外部へ露出しない
- 同種の情報はツール間で同じフィールド名を使う
- read 系、write 系、補助系で応答形を大きく崩さない
- transport エラーとツール実行エラーを分離する

### 3.2 共通入力モデル

各ツール入力は、必要に応じて以下の共通フィールド群から構成する。

- `path`
  - 現在ページを指す絶対 path
- `prefix`
  - 一覧・検索の起点となる絶対 path
- `content`
  - ページ全文または追記文字列
- `rename_to`
  - rename 先の絶対 path
- `section`
  - セクション取得対象を表す識別子
- `revision`
  - 対象 revision を表す整数
- `query`
  - 全文検索式

入力モデルでは、各ツールに不要なフィールドは持たせず、
汎用の巨大入力構造を作らない。

### 3.3 共通出力モデル

各ツール出力は、次の共通的な情報単位から構成する。

- `path`
  - 結果対象の現在 path
- `revision`
  - 結果に対応する revision
- `content`
  - 取得または更新後の本文
- `summary`
  - 人間向けの簡潔な結果説明

write 系では `path` と `revision` を必須の基本出力とし、
read 系では取得内容に応じて `content` または一覧項目群を返す。

### 3.4 ツール別入力モデル

#### 3.4.1 `get_page`

入力:

- `path`
- `revision` 任意
  - 未指定時は最新 revision

#### 3.4.2 `list_pages`

入力:

- `prefix`
- `limit` 任意
  - 未指定時は 50 、上限は 100
- `cursor` 任意
  - `prefix` 配下の path のみ許可する
  - 当該 path 自身は返却範囲に含めない

#### 3.4.3 `get_page_toc`

入力:

- `path`
- `revision` 任意
  - 未指定時は最新 revision

#### 3.4.4 `search_pages`

入力:

- `query`
- `prefix` 任意
- `limit` 任意
  - 未指定時は 20 、上限は 100

#### 3.4.5 `create_page`

入力:

- `path`
- `content`

#### 3.4.6 `update_page`

入力:

- `path`
- `content`

#### 3.4.7 `append_page`

入力:

- `path`
- `content`

`append_page` の `content` は追記文字列を意味し、
全文置換ではないことをツール説明で明示する。

#### 3.4.8 `rename_page`

入力:

- `path`
- `rename_to`

#### 3.4.9 `get_page_section`

入力:

- `path`
- `section`
- `revision` 任意

`section` は、
少なくとも `by=id` と `by=title` を受け付ける selector とし、
文字列単独指定は `by=title` の省略形として扱う。

### 3.5 ツール別出力モデル

#### 3.5.1 `get_page`

出力:

- `path`
- `revision`
- `content`

必要に応じて、後続詳細設計でメタ情報追加の余地を残す。

#### 3.5.2 `list_pages`

出力:

- `items`
  - 各項目は `path`、`revision`、`updated_at`、`updated_by` を持つ
- `has_more`
- `next_cursor` 任意

一覧項目には `page_id` を含めない。
一覧は `path` 昇順で返し、
`next_cursor` は返却 `items` の最後の `path` を用いる。

#### 3.5.3 `get_page_toc`

出力:

- `path`
- `revision`
- `sections`
  - 各項目は `id`、`title`、`level`、`ordinal`、`parent_id`、`section_chars` を持つ

`sections` は文書順の平坦配列とし、
`parent_id` により親子関係を復元可能とする。

`section_chars` は、
`get_page_section.content` と同じ返却範囲の Unicode 文字数を表す。

#### 3.5.4 `search_pages`

出力:

- `items`
  - 各項目は `path`、`revision`、`score`、`snippet` を持つ

検索結果にも `page_id` を含めない。
結果はスコア降順で返し、
同点時は `path` 昇順で安定化する。
`has_more` と `next_cursor` は持たない。

#### 3.5.5 `create_page`

出力:

- `path`
- `revision`
- `summary`

#### 3.5.6 `update_page`

出力:

- `path`
- `revision`
- `summary`

#### 3.5.7 `append_page`

出力:

- `path`
- `revision`
- `summary`

`summary` には、必要に応じて amend 相当で処理されたかどうかを含められるようにする。

#### 3.5.8 `rename_page`

出力:

- `path`
  - 変更後 path
- `revision`
- `summary`

rename 前 path は `summary` または監査ログ側で扱い、
基本出力には `path` を結果 path として載せる。

#### 3.5.9 `get_page_section`

出力:

- `path`
- `revision`
- `section`
  - 解決後のセクション識別情報
- `content`

`content` は、対象見出し行を含めず、
次に現れる同レベル以上の見出し直前までを返す。
子見出し配下の本文は含める。

### 3.6 エラー表現

ツール実行エラーは、transport レベルの HTTP エラーとは別に、
MCP ツールの失敗として扱う。

初期実装では、少なくとも以下の論理区分を持つ。

- `not_found`
  - 対象ページや対象セクションが見つからない
- `forbidden`
  - スコープ不足または path prefix 制約違反
- `conflict`
  - 競合、ロック待機失敗、rename 衝突
- `invalid_input`
  - path 不正、検索式不正、section 指定不正
- `unsupported`
  - 初期版対象外操作
- `internal_error`
  - 内部失敗

エラー応答には、少なくとも以下の情報を含める。

- `code`
- `message`

必要に応じて、後続詳細設計で `details` を追加できる余地を残す。

### 3.7 監査ログとの接続

write 系および認可失敗系では、
ツール出力モデルと完全一致しない監査ログ向け情報が必要になる。

そのため、ツール出力モデルとは別に、
内部では以下を保持できる前提とする。

- 操作種別
- 対象 path
- 結果 path
- revision
- 監査用 summary

この情報はサービス層の共通出力モデルから組み立てる。

### 3.8 入出力モデルに関する設計判断

本章の入出力モデル設計では、以下を基本方針として採用する。

- 入力は path ベースで統一する
- 出力は `path`、`revision`、`content`、`summary`、`items` を基本単位として揃える
- 一覧・検索結果から `page_id` を排除する
- write 系出力は `path` と `revision` を基本とする
- ツール実行エラーは `not_found`、`forbidden`、`conflict`、`invalid_input`、`unsupported`、`internal_error` の論理区分で扱う

## 4. MCP エラー応答と内部エラー分類の対応

本章では、MCP のツール実行時に発生する内部エラーを、
外部へ返す論理エラー区分へどう対応付けるかを定義する。

ここで扱うのは transport レベルの HTTP エラーではなく、
認証成功後にツール実行過程で発生する失敗である。

### 4.1 基本方針

- transport レベルの失敗とツール実行エラーは分離する
- 認証失敗は transport / 認証入口の失敗として扱い、ツール実行エラーへ混在させない
- 認可失敗はツール実行エラーとして `forbidden` へ写像する
- DB 層やサービス層の詳細エラーは、そのまま外部へ露出しない
- 外部へは論理区分 `code` と説明 `message` を返す

### 4.2 エラー処理の段階

MCP endpoint の失敗は、以下の 3 段階に分けて扱う。

1. transport レベル
   - メソッド不正
   - `Content-Type` 不正
   - JSON 不正
   - `MCP-Protocol-Version` 不正
   - `Origin` 不正
2. 認証レベル
   - Bearer トークン欠落
   - Bearer トークン不正
   - Bearer トークン失効 / 期限切れ
   - Basic 認証の持ち込み
3. ツール実行レベル
   - 認可失敗
   - 入力不正
   - 競合
   - 対象未発見
   - 内部失敗

本章で定義する写像は 3 の範囲を対象とする。

### 4.3 論理エラー区分への写像

内部エラーは、少なくとも以下の論理区分へ写像する。

- `not_found`
  - 対象ページが存在しない
  - 対象セクションが存在しない
- `forbidden`
  - 必要スコープ不足
  - path prefix 制約違反
- `conflict`
  - ページ競合
  - rename 先衝突
  - ロック待機失敗
  - amend 不許可
- `invalid_input`
  - path 不正
  - 検索式不正
  - セクション指定不正
  - rename 先指定不正
- `unsupported`
  - 初期版対象外機能
- `internal_error`
  - 上記へ分類できない内部失敗

### 4.4 既存 `DbError` との対応

既存 `DbError` および既存 REST API の異常系を踏まえ、
初期案として以下の対応を採る。

| 内部エラー候補 | MCP 論理エラー区分 | 外部メッセージ例 |
|:--|:--|:--|
| `PageNotFound` | `not_found` | `page not found` |
| `PageDeleted` | `not_found` | `page not found` |
| `PageAlreadyExists` | `conflict` | `page already exists` |
| `PageLocked` | `conflict` | `page is locked` |
| `InvalidMoveDestination` | `invalid_input` | `invalid destination path` |
| `InvalidPath` | `invalid_input` | `path is invalid` |
| `InvalidRevision` | `invalid_input` | `revision is invalid` |
| `AmendForbidden` | `conflict` | `append amend is not allowed` |
| `RootPageProtected` | `forbidden` | `operation is not allowed for root page` |
| `UserNotFound` | `internal_error` | `user resolution failed` |
| その他未分類失敗 | `internal_error` | `internal error` |

削除済みページについては、MCP では参照対象外であるため、
`PageDeleted` を `not_found` 相当として扱い、
deleted ページの存在を外部へ積極的には示さない。

### 4.5 認可失敗の対応

認可失敗は、内部の詳細理由に関わらず、外部の論理区分としては `forbidden` に統一する。

ただし内部では、少なくとも以下の区別を保持する。

- スコープ不足
- path prefix 制約違反
- `ReadOnly` 属性による write 系操作禁止

この内部区別は監査ログの `result` や `summary` に利用し、
外部応答では不要に詳細を漏らさない。

### 4.6 競合系の対応

競合系は `conflict` へ統一する。

初期実装でこの区分に含める主なものは以下とする。

- `PageAlreadyExists`
- `PageLocked`
- `AmendForbidden`
- rename 先衝突
- `append` の待機タイムアウト

これにより、呼び出し側は「再試行または入力変更が必要な失敗」として一貫して扱える。

### 4.7 入力不正と未対応の区別

`invalid_input` と `unsupported` は明確に分ける。

- `invalid_input`
  - そのツールで受け付けるが、値が不正
- `unsupported`
  - 初期版でその操作自体を提供しない

例えば restore を要求した場合は `unsupported`、
rename 先 path が不正な場合は `invalid_input` とする。

### 4.8 エラー応答の基本形

ツール実行エラーの応答は、少なくとも以下の形を取る。

- `code`
- `message`

必要に応じて後続詳細設計で `details` を追加できるが、
初期実装では `details` を必須にしない。

例:

```json
{
  "code": "conflict",
  "message": "page already exists"
}
```

### 4.9 監査ログとの関係

外部エラー区分は簡潔に保つ一方で、
監査ログではより細かい内部結果を保持できる前提とする。

そのため、内部では次の 2 系統を分けて扱う。

- 外部応答用の論理エラー区分
- 監査ログ用の詳細結果分類

これにより、セキュリティ上過剰な詳細を外部へ返さずに、
運用上必要な監査粒度を維持する。

### 4.10 エラー応答に関する設計判断

本章のエラー応答設計では、以下を基本方針として採用する。

- transport レベルの失敗とツール実行エラーを分離する
- 認可失敗は `forbidden`、競合系は `conflict`、対象未発見は `not_found` へ写像する
- `invalid_input` と `unsupported` を区別する
- 既存 DB / service 層の詳細失敗は外部へ直接露出しない
- 外部応答は `code` と `message` を基本形とし、監査ログ用詳細分類とは分離する
