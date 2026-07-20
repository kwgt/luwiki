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
- `docs/MCP_RESOURCE_SPECS.md`
  - MCP標準resources操作の外部契約、URI、認可、エラー、監査方針を確認する場合に参照する

---

## 1. 対象範囲

本書では以下を対象とする。

- 初期版で公開するツール一覧
- ツールごとの責務
- 共通入力モデルと共通出力モデル
- ツール別入力モデルと出力モデル
- MCP標準prompts/resources操作の入出力モデルとprotocol error変換
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
- `edit_page`
  - 指定 path のページ本文に対して部分編集を適用する
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
- 検索対象 `target` を必須入力とする
- `target` は `headings` / `body` / `code` / `front_matter` から 1 件以上を受け付ける
- 任意で prefix 指定を受け付ける
- prefix 指定がある場合は prefix 自体の認可判定を行う
- path 制約外の結果は current path 解決後に後段フィルタする
- 検索結果は path ベースで返す
- 受け取った `target` 群だけを FTS へ流し、複数指定時のみスコアマージを行う
- cursor ページングは持たず、上位 `limit` 件取得として扱う

#### 2.3.5 `create_page`

- target path と初期本文を受け取る
- create スコープと target path への認可を要求する
- 作成後ページの path と revision 情報を返す

#### 2.3.6 `update_page`

- current path と更新本文を受け取る
- update スコープと current path への認可を要求する
- 全文上書き更新を行う
- 部分編集や差分適用は扱わず、必要な場合は `edit_page` を使う

#### 2.3.7 `edit_page`

- current path と単一の編集操作を受け取る
- update スコープと current path への認可を要求する
- revision と instance_id による内容整合性確認を行う
- セクション単位またはテキスト単位の部分編集を行う
- 全文置換は扱わず、本文全体を置き換えたい場合は `update_page` を使う
- 末尾への単純追記は扱わず、追記専用経路が必要な場合は `append_page` を使う

#### 2.3.8 `append_page`

- current path と追記文字列を受け取る
- append スコープと current path への認可を要求する
- 単純末尾追記のみを扱う
- amend 相当判定は内部処理で行う
- セクション編集や任意位置の書き換えは扱わず、必要な場合は `edit_page` を使う

#### 2.3.9 `rename_page`

- current path と rename_to path を受け取る
- update スコープを要求する
- current path と rename_to path の双方で認可判定を行う
- restore は扱わない

#### 2.3.10 `get_page_section`

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

- 初期版の公開ツールは `get_page`、`get_page_toc`、`list_pages`、`search_pages`、`create_page`、`update_page`、`edit_page`、`append_page`、`rename_page`、`get_page_section` とする
- ツールは path ベースで定義し、`page_id` は完全非公開とする
- `append_page` は `update_page` から独立したツールとする
- `edit_page` は `update_page` から独立した部分編集ツールとして扱う
- `edit_page` は `append_page` からも独立し、任意位置の編集と単純末尾追記を分離する
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
- `instance_id`
  - ページ内容の一意性を表す内容識別子
- `content`
  - 取得または更新後の本文
- `summary`
  - 人間向けの簡潔な結果説明

`edit_page` へ安全に接続できるよう、
ページ内容を返す read 系および更新結果を返す write 系では
`instance_id` を必須出力として返す。

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
- `target`
  - `headings` / `body` / `code` / `front_matter` から 1 件以上必須
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

#### 3.4.7 `edit_page`

入力:

- `path`
- `revision`
- `instance_id`
- `operation`
  - 単一の編集操作を指定する
  - 初期版では `replace_section`、`insert_section`、`delete_section`、`replace_text` を受け付ける

`operation` 内のセクション指定は、
少なくとも `by=id` と `by=title` を受け付ける selector とし、
文字列単独指定は `by=title` の省略形として扱う。

#### 3.4.8 `append_page`

入力:

- `path`
- `content`

`append_page` の `content` は追記文字列を意味し、
全文置換ではないことをツール説明で明示する。

#### 3.4.9 `rename_page`

入力:

- `path`
- `rename_to`

#### 3.4.10 `get_page_section`

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
- `instance_id`
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
- `instance_id`
- `sections`
  - 各項目は `id`、`title`、`level`、`ordinal`、`parent_id`、`section_chars` を持つ

`sections` は文書順の平坦配列とし、
`parent_id` により親子関係を復元可能とする。

`section_chars` は、
`get_page_section.content` と同じ返却範囲の Unicode 文字数を表す。

#### 3.5.4 `search_pages`

- `invalid_input`
  - `query` が空
  - `target` が未指定または空
  - `prefix` が不正
  - `limit` が範囲外
- `forbidden`
  - `read` スコープ不足
  - 要求 `prefix` が path prefix 制約違反
- `internal_error`
  - FTS 実行、current path 解決、または内部処理で想定外の失敗が発生した

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
- `instance_id`
- `summary`

#### 3.5.6 `update_page`

出力:

- `path`
- `revision`
- `instance_id`
- `summary`

#### 3.5.7 `edit_page`

出力:

- `path`
- `revision`
- `instance_id`
- `summary`

#### 3.5.8 `append_page`

出力:

- `path`
- `revision`
- `instance_id`
- `summary`

`summary` には、必要に応じて amend 相当で処理されたかどうかを含められるようにする。

#### 3.5.9 `rename_page`

出力:

- `path`
  - 変更後 path
- `revision`
- `instance_id`
- `summary`

rename 前 path は `summary` または監査ログ側で扱い、
基本出力には `path` を結果 path として載せる。

#### 3.5.10 `get_page_section`

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
  - `create_page` `update_page` `append_page` に渡された
    raw source 内の front matter 構文不正またはスキーマ不正
- `not_latest_revision`
  - 更新要求の `revision` が最新 revision と一致しない
- `instance_id_not_match`
  - 更新要求の `instance_id` が最新内容の識別子と一致しない
- `unsupported`
  - 初期版対象外操作
- `internal_error`
  - 内部失敗

エラー応答には、少なくとも以下の情報を含める。

- `code`
- `message`

必要に応じて、後続詳細設計で `details` を追加できる余地を残す。

front matter 起因の write 系失敗は、初期版では `invalid_input` として扱う。
対象は少なくとも `create_page`、`update_page`、`append_page` とし、
`unsupported` や `conflict` へは分類しない。

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
- ツール実行エラーは `not_found`、`forbidden`、`conflict`、`invalid_input`、`not_latest_revision`、`instance_id_not_match`、`unsupported`、`internal_error` の論理区分で扱う

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
  - front matter 構文不正
  - front matter スキーマ不正
- `not_latest_revision`
  - 更新要求の `revision` が最新 revision と一致しない
- `instance_id_not_match`
  - 更新要求の `instance_id` が最新内容の識別子と一致しない
- `unsupported`
  - 初期版対象外機能
- `internal_error`
  - 上記へ分類できない内部失敗

初期版では、`edit_page` の公開エラーとして
`not_latest_revision` と `instance_id_not_match` を追加する。
これらは部分編集の前提となる内容整合性確認の失敗を表す。

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
| `NotLatestRevision` | `not_latest_revision` | `revision is not latest` |
| `InstanceIdNotMatch` | `instance_id_not_match` | `instance_id does not match latest content` |
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

同様に、write 系ツールで front matter の構文またはスキーマが不正な場合も、
「そのツールで受け付ける入力だが値内容が不正」であるため `invalid_input` とする。

### 4.8 `not_latest_revision` と `instance_id_not_match` の位置付け

`not_latest_revision` と `instance_id_not_match` は、
どちらも更新系ツールにおける内容整合性の失敗を表す公開エラーとして扱う。

初期版では、これらを特に `edit_page` の固有公開エラーとして扱う。
将来、同じ内容整合性確認を要求する更新系ツールが追加された場合は再利用してよい。

- `invalid_input`
  - 入力形式や値域そのものが不正な場合に使う
  - 例: path 形式不正、section selector 不正、未定義 operation 指定
- `not_latest_revision`
  - 入力形式は正しいが、呼び出し側が見ていた revision がすでに最新ではない場合に使う
- `instance_id_not_match`
  - 入力形式は正しいが、呼び出し側が見ていた内容識別子と最新内容が一致しない場合に使う
- `conflict`
  - ロック中、rename 先衝突、amend 不許可など、内容照合以外の競合に使う
- `internal_error`
  - 上記いずれにも分類できない内部失敗に限定する

この区別により、呼び出し側は
「入力を直すべき失敗」と「再取得してやり直すべき失敗」を分けて扱える。

### 4.9 エラー応答の基本形

ツール実行エラーの応答は、少なくとも以下の形を取る。

- `code`
- `message`

必要に応じて後続詳細設計で `details` を追加できるが、
初期実装では `details` を必須にしない。

ただし front matter 起因失敗については、REST API で採用している
`detail.type = syntax | validation`、`line`、`column`、`property_path`
に相当する情報を、将来 JSON-RPC `error.data.detail` へ搭載できる余地を残す。
この場合も論理区分 `code` 自体は `invalid_input` のままとし、
詳細分類は `error.data.detail.type` 側で表現する方針とする。

例:

```json
{
  "code": "conflict",
  "message": "page already exists"
}
```

### 4.10 監査ログとの関係

外部エラー区分は簡潔に保つ一方で、
監査ログではより細かい内部結果を保持できる前提とする。

そのため、内部では次の 2 系統を分けて扱う。

- 外部応答用の論理エラー区分
- 監査ログ用の詳細結果分類

これにより、セキュリティ上過剰な詳細を外部へ返さずに、
運用上必要な監査粒度を維持する。

### 4.11 エラー応答に関する設計判断

本章のエラー応答設計では、以下を基本方針として採用する。

- transport レベルの失敗とツール実行エラーを分離する
- 認可失敗は `forbidden`、競合系は `conflict`、対象未発見は `not_found` へ写像する
- `invalid_input` と `unsupported` を区別する
- `not_latest_revision` と `instance_id_not_match` を更新系の公開整合性エラーとして扱う
- 既存 DB / service 層の詳細失敗は外部へ直接露出しない
- 外部応答は `code` と `message` を基本形とし、監査ログ用詳細分類とは分離する

## 5. MCP promptsの入出力とprotocol error

promptsの外部契約の正本は`docs/MCP_PROMPT_SPECS.md`とし、
本章ではLuWiki内部モデルとrmcp公開型の変換境界を定義する。

### 5.1 `prompts/list`

service層とserver層の境界には`PromptListArgument`、`PromptListItem`、
`ListPromptsServiceResult`を使用する。

- `mcp.name`を変換せず`Prompt.name`へ設定する
- `mcp.description`を`Prompt.description = Some(...)`として返す
- argumentsはfront matterの定義順を維持する
- `required`の`None`、`Some(false)`、`Some(true)`を正規化しない
- arguments未指定時は空配列ではなく`Prompt.arguments = None`とする
- `Prompt.title`、`Prompt.icons`、`Prompt.meta`、
  `PromptArgument.title`、`ListPromptsResult.meta`は`None`とする
- `mcp.system`を一覧へ独自公開しない

公開可能候補をcase-sensitiveな`str::cmp`相当で昇順に並べる。
cursor未指定時は先頭から、指定時は`name > cursor`の候補を対象とし、
cursor自身を含めない。cursor名の実在は要求せず、snapshotは保証しない。

最大51件を取得し、先頭50件を返す。続きがある場合は返却する50件目の
prompt名を`nextCursor`とする。候補なし、またはcursor適用後に0件となる場合は、
空配列、`nextCursor = None`、`meta = None`の正常結果を返す。

### 5.2 `prompts/get`

primitive共通名前索引からpage IDを解決し、latest sourceのfront matterを
共通parserで再検証する。front matter除去後のraw Markdown本文を使用し、
prompt候補テーブルの欠損だけでは取得不能にしない。

要求argumentsは一意キーのJSON objectとして受け取る。

- required引数の不足を拒否する
- 未知引数を拒否する
- JSON string以外の値を拒否する
- optional未指定は空文字列へ展開する
- `{{@name}}`、`{{@@name}}`をsystemと本文へそれぞれ一回適用する
- 挿入値は再展開しない

system未指定時は本文だけを使用する。system指定時は展開後system、
LF 2文字、展開後本文を連結する。trimと改行正規化を行わず、
1件のUser text messageとして返す。system role、Assistant role、
複数messageは使用しない。

`GetPromptResult.description`には`mcp.description`だけを設定し、
systemを連結しない。

### 5.3 protocol error変換

| 条件 | JSON-RPC code | `error.data.code` | message |
|:--|--:|:--|:--|
| cursor不正 | `-32602` | `invalid_input` | `cursor is invalid` |
| prompt不存在・非公開 | `-32602` | `not_found` | `prompt not found` |
| scope不足 | `-32600` | `forbidden` | `operation is not allowed` |
| DB失敗・候補・正本不整合 | `-32603` | `internal_error` | `internal error` |

引数不正は`-32602` / `invalid_input`とし、次の安全なmessageだけを返す。

- `required prompt argument is missing: <name>`
- `unknown prompt argument: <name>`
- `prompt argument must be a string: <name>`

front matter不正、用途・名前不一致、未宣言placeholderなどの最新正本不整合は
`-32603` / `internal_error` / `internal error`へ固定変換する。

protocol errorへcursor検証詳細、DB内部エラー、重複prompt名、ページpath、
page ID、ローカルファイルpath、本文、system、引数値を含めない。

### 5.4 読み取り専用性

`prompts/list`と`prompts/get`は読み取り専用であり、成功・失敗にかかわらず
ページ正本、front matter、名前索引、prompt候補を変更しない。
read scopeを要求するが、ページ用path prefix制約は適用しない。

## 6. MCP resourcesの入出力とprotocol error

resourcesの外部契約の正本は`docs/MCP_RESOURCE_SPECS.md`とし、
本章ではLuWiki内部モデルとrmcp公開型の変換境界を定義する。

`resources/list`と`resources/read`はLuWiki独自toolではなく、
MCP標準resources操作として扱う。LuWiki独自toolsの一覧や入力モデルには
含めず、rmcp標準handlerからservice層へ接続する。

### 6.1 `resources/list`

service層とserver層の境界には`ResourceListItem`、
`ListResourcesServiceResult`を使用する。

- LuWiki内部のresource URIを`Resource.uri`へ設定する
- resource名を`Resource.name`へ設定する
- descriptionを`Resource.description = Some(...)`として返す
- MIME typeを`Resource.mimeType = Some(...)`として返す
- `Resource.title`、`Resource.icons`、`Resource.annotations`、
  `Resource.meta`は初期版では設定しない
- ページpath、page ID、path prefix判定情報を公開しない

固定組み込みresourceとページ由来resourceを同じ一覧へ合流し、
resource URIのcase-sensitiveな`str::cmp`相当で昇順に並べる。
cursor未指定時は先頭から、指定時は`uri > cursor`のresourceを対象とし、
cursor自身を含めない。cursor URIの実在は要求せず、snapshotは保証しない。

最大51件を取得し、先頭50件を返す。続きがある場合は返却する50件目の
resource URIを`nextCursor`とする。候補なし、またはcursor適用後に
0件となる場合は、空配列、`nextCursor = None`、`meta = None`の
正常結果を返す。

resourcesにはページ用path prefix制約を適用しない。
ページ由来resourceは、read scope、ページ状態、resource ACLを満たす場合、
current pathがBearerのpath prefix範囲外でも一覧対象とする。
固定組み込みresourceにもページ用path prefix制約を適用しない。

### 6.2 `resources/read`

URIから固定組み込みresourceまたはページ由来resourceを判定する。

- 固定組み込みresourceは実装内の固定IDから本文を取得する
- ページ由来resourceはresource URI逆引き索引からpage IDを解決する
- ページ由来resourceはlatest page state、latest revision、latest sourceを取得する
- latest sourceのfront matterを再検証し、resource path一致を確認する
- 本文はfront matter除去後のraw Markdown本文とする
- 空または空白だけの本文もresource本文として返す

server層ではservice結果を`ResourceContents.text`へ変換する。
`contents.uri`には解決済みresource URI、`contents.mimeType`には
resourceのMIME typeを設定する。ページ由来resourceのlatest revisionは
監査ログ内部で扱い、MCP結果へは公開しない。

ページ由来resourceがresource ACLで非許可の場合は、
存在有無を秘匿するため`resource not found`へ変換する。

### 6.3 protocol error変換

| 条件 | JSON-RPC code | `error.data.code` | message |
|:--|--:|:--|:--|
| `resources/list`のcursor不正 | `-32602` | `invalid_input` | `cursor is invalid` |
| `resources/read`のURI形式不正 | `-32602` | `invalid_input` | `resource uri is invalid` |
| resource不存在・非公開 | `-32602` | `not_found` | `resource not found` |
| scope不足 | `-32600` | `forbidden` | `operation is not allowed` |
| DB失敗・URI索引不整合・latest source欠落・front matter再検証失敗 | `-32603` | `internal_error` | `internal error` |

authority不一致、未知の固定組み込みresource、存在しないページ由来resource、
draft、soft delete、hard delete、resource ACL非許可は、
いずれも`-32602` / `not_found` / `resource not found`へ秘匿する。

resource URI形式として成立しない入力、境界空白、制御文字、
ページ由来resource pathの値制約違反は`invalid_input`とする。
ただしauthorityが異なるURI、未知の`/builtin/` ID、
予約pathまたは存在しない既知authority配下pathは、
形式ではなく非公開resourceとして`not_found`へ寄せる。

protocol errorへcursor検証詳細、DB内部エラー、resource本文、
front matter本文、Bearer token、Authorization header、ページpath、page ID、
resource ACL非許可であることを示す詳細を含めない。

### 6.4 読み取り専用性

`resources/list`と`resources/read`は読み取り専用であり、成功・失敗にかかわらず
ページ正本、front matter、resource候補、resource URI逆引き索引を変更しない。
read scopeを要求する。

固定組み込みresourceにはページ用path prefix制約を適用しない。
ページ由来resourceにもページ用path prefix制約を適用しない。
同じBearerを使うpathベースtoolsには従来どおりpath prefix制約を適用する。
