# MCPツール仕様

本書では、MCPクライアントへ公開するツール仕様を定義する。

`docs/MCP_INTERFACE_AND_ERROR_DESIGN.md` が内部設計として
ツール責務、入出力モデル、エラー分類の設計意図を整理するのに対し、
本書は外部契約としてのツール名、入力、出力、エラー、および注記を定義する。

MCP標準`prompts/list`と`prompts/get`はtoolではなく標準prompts操作である。
promptsの外部契約は`docs/MCP_PROMPT_SPECS.md`を参照する。
MCP標準`resources/list`と`resources/read`はtoolではなく標準resources操作である。
resourcesの外部契約は`docs/MCP_RESOURCE_SPECS.md`を参照する。

初期版MCPの外部仕様は、`docs/REQUIREMENTS.md`および
各設計分冊で確定した内容を本書へ集約する。

---

## ツール一覧

  | ツール | 用途
  |:--|:--
  | `get_page` | [ページ全体の取得](#tool-get-page)
  | `get_agent_instructions` | [エージェント用運用指示の取得](#tool-get-agent-instructions) |
  | `get_page_state` | [最新ページ状態の取得](#tool-get-page-state)
  | `wait_page_change` | [ページ状態変更待ち](#tool-wait-page-change)
  | `get_page_toc` | [ページ見出し構造の取得](#tool-get-page-toc)
  | `list_pages` | [ページ一覧の取得](#tool-list-pages)
  | `search_pages` | [ページ検索の実行](#tool-search-pages)
  | `create_page` | [ページ作成](#tool-create-page)
  | `update_page` | [ページ更新](#tool-update-page)
  | `edit_page` | [ページ編集](#tool-edit-page)
  | `append_page` | [ページ追記](#tool-append-page)
  | `rename_page` | [ページリネーム](#tool-rename-page)
  | `get_page_section` | [特定セクション本文の取得](#tool-get-page-section)
  | `list_page_assets` | [ページアセット一覧の取得](#tool-list-page-assets)
  | `resolve_page_asset` | [ページアセットの解決](#tool-resolve-page-asset)
  | `get_page_asset_metadata` | [ページアセットメタ情報の取得](#tool-get-page-asset-metadata)
  | `get_page_asset_data` | [ページアセット本体の取得](#tool-get-page-asset-data)
  | `upload_page_asset` | [ページアセットのアップロード](#tool-upload-page-asset)
  | `delete_page_asset` | [ページアセットの削除](#tool-delete-page-asset)

---

## 1. 共通事項

### 保存後の全文索引同期

`create_page` / `update_page` / `edit_page` / `append_page` / `rename_page`は、
正本の保存後に影響ページの全文索引を同期してから成功を返す。
amendしたrevisionは置換し、再帰renameでは子ページも同期する。
入力/認可/競合で保存前に拒否した要求では索引を更新しない。
索引更新失敗は固定の`internal_error`として保存済みであることを伝えるため、
未保存と推測せず最新本文を再取得する。既存の入力/出力schema・認可条件は変更しない。

### 1.1 基本方針

- MCP は `run` コマンドで明示的に有効化された場合のみ公開する
- MCP は Bearer 認証を前提とし、Basic 認証は受け付けない
- tool成功時のJSON payloadは、
  JSON objectをserializeした文字列として
  `result.content[0].text`へ格納し、`structuredContent`は返さない
- 本書で定義するページtoolはpathベースで公開し、外部へ`page_id`を露出しない
- pathベースtoolの認可はBearerスコープとpath prefix制約の両方で判定する
- 本書で定義するアセットtoolは`path + file_name`を主入力として公開し、初期版では`asset_id`単独入力のtoolを公開しない
- アセットtoolの認可はBearerスコープとpath prefix制約の両方で判定し、path prefix制約は正規化済みの入力`path`を対象とする
- 一覧、解決、メタ情報取得系のアセットtoolはアセット本体を返さない
- アセット本体は`get_page_asset_data`のみが`data_base64`として返す
- `upload_page_asset`はローカルファイルパスを受け付けず、排他的な`text`/`data_base64`入力で完結する
- アセットuploadと本体取得で扱うraw binary sizeの上限は10MiBとする
- `data_base64`、アセット本体bytes、Bearer token、Authorization header、lock token、内部保存パス、DB内部詳細は監査ログ、tracing、エラーsummaryに含めない
- MCP promptsはread scopeを要求するが、ページ用path prefix制約を適用しない
- 監査ログは MCP 操作および関連する認可失敗を対象として記録する

### 1.1.1 入力ページパス

ページ/アセットtoolのpath・prefix・cursor・rename_toはREQUIREMENTS.md 2.2の共通規則を使う。
絶対パスのraw検証→dot解決→最終検証とし、root以外の末尾/、連続/、空白（全角を含む）・
制御文字・バックスラッシュを拒否する。`.`・`..`は解決し、root越えは拒否する。
MCP renameにREST配置先の末尾/特例を適用しない。不正入力はInvalidInput、認可拒否と区別する。
JSON文字列の%表記をWikiパス処理でURLデコードしない。

### 1.2 公開するツール

初期版では、少なくとも以下のツールを公開する。

- `get_page`
- `get_agent_instructions`
- `get_page_state`
- `wait_page_change`
- `get_page_toc`
- `list_pages`
- `search_pages`
- `create_page`
- `update_page`
- `edit_page`
- `append_page`
- `rename_page`
- `get_page_section`
- `list_page_assets`
- `resolve_page_asset`
- `get_page_asset_metadata`
- `get_page_asset_data`
- `upload_page_asset`
- `delete_page_asset`

### 1.3 初期版から除外する機能

以下は初期版ではツールとして公開しない。

- 削除済みページ参照
- restore
- ロック操作
- `asset_id` 単独入力によるアセット解決、メタ情報取得、本体取得、削除
- ローカルファイルパスを指定するアセット upload
- 既存アセットの上書き upload
- lock token 入力を受け付けるアセット upload / delete
- 削除済みアセットの直接参照、列挙、復元、再削除
- ゾンビアセットまたは所属ページ解決不能アセットの直接操作
- 他ページ本文中の `asset:` 参照をたどって実体解決する専用 tool
- REST API の redirect や条件付き GET をそのまま MCP tool として公開する操作
- アセットのリネーム、コピー、移動、MIME 種別のみの更新
- テンプレート指定作成
- リンク先一覧取得
- 被リンク一覧取得

### 1.4 共通エラー区分

ツール実行失敗時は、少なくとも以下の論理区分を用いる。

- `not_found`
- `forbidden`
- `conflict`
- `invalid_input`
- `unsupported`
- `internal_error`

各ツール節では、必要に応じて主な発生条件を補足する。

front matter 起因のwrite系失敗は`invalid_input`として扱う。
対象は`create_page`、`update_page`、`edit_page`、`append_page`とする。

### 1.5 front matter 仕様導線

- `create_page`、`update_page`、`append_page` の `content` は、
  front matter を含む raw source 全体を受け付ける
- `initialize.instructions`は、すべてのHTTP要求でBearer認証を使用する旨を
  案内する
- `initialize.instructions`は、`edit_page.replace_section` の `content` に対象見出し行を含めず、見出し変更は `title`、サブセクション込み置換は `include_subsection: true` を使う旨を案内する
- front matter詳細仕様およびMCP prompts仕様は、
  MCP標準resourcesの固定組み込みresourceとして参照できる
- MCP promptsのfront matter、field、cursor、message、errorの外部契約は
  `docs/MCP_PROMPT_SPECS.md`を参照する
- MCP resourcesのURI、field、cursor、contents、errorの外部契約は
  `docs/MCP_RESOURCE_SPECS.md`を参照する

---

## 2. ツール仕様

<a id="tool-get-page"></a>
### 2.1 `get_page`

#### 概要

指定した current path の Markdown 本文全体を取得する。

初期版では current path を基準とした通常ページ参照のみを扱い、
削除済みページ参照や restore 前提の取得は行わない。

#### 認可

- Bearer 認証が必要
- 必要スコープは `read`
- 認可判定対象 path は対象ページの current path とする

#### 入力

```yaml
type: object
required:
  - path
properties:
  path:
    description: >-
      対象ページの絶対 path 。
    type: string
  revision:
    description: >-
      対象 revision 。未指定時は最新 revision を参照する。
    type: integer
```

#### 出力

```yaml
type: object
required:
  - path
  - revision
  - instance_id
  - content
properties:
  path:
    description: >-
      解決後の current path 。
    type: string
  revision:
    description: >-
      実際に参照した revision 。
    type: integer

  instance_id:
    description: >-
      ページ内容の一意性を表すインスタンスID。
    type: "string"

  content:
    description: >-
      対象 revision の Markdown 本文全体。
    type: string
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 指定 revision が存在しない
  - 対象 path が通常ページとして解決できない
- `forbidden`
  - `read` スコープ不足
  - path prefix 制約違反
- `invalid_input`
  - `path` が不正
  - `revision` が不正
- `internal_error`
  - 本文取得や内部処理で想定外の失敗が発生した

#### 注記

- 出力はページ本文全体を返し、見出し構造やセクション単位の切り出しは行わない
- ページ内の一部分だけを取得したい場合は `get_page_toc` と `get_page_section` を利用する

<a id="tool-get-page-toc"></a>
### 2.2 `get_page_toc`

#### 概要

指定した current path の Markdown 本文から、
見出し構造および各セクションの概算規模を返す。

#### 認可

- Bearer 認証が必要
- 必要スコープは `read`
- 認可判定対象 path は対象ページの current path とする

#### 入力

```yaml
type: object
required:
  - path
properties:
  path:
    description: >-
      対象ページの絶対 path 。
    type: string
  revision:
    description: >-
      対象 revision 。未指定時は最新 revision を参照する。
    type: integer
```

#### 出力

```yaml
type: object
required:
  - path
  - revision
  - instance_id
  - sections
properties:
  path:
    description: >-
      解決後の current path 。
    type: string
  revision:
    description: >-
      実際に参照した revision 。
    type: integer

  instance_id:
    description: >-
      ページ内容の一意性を表すインスタンスID。
    type: "string"

  sections:
    description: >-
      セクション一覧。文書順の平坦配列として返し、`parent_id` により
      親子関係を復元できる。
    type: array
    items:
      type: object
      required:
        - id
        - title
        - level
        - ordinal
        - section_chars
      properties:
        id:
          description: >-
            同一 `revision` 内でのみ有効な動的 `section_id` 。
          type: string
        title:
          description: >-
            見出し文字列。
          type: string
        level:
          description: >-
            見出しレベル。
          type: integer
        ordinal:
          description: >-
            文書順の出現番号。
          type: integer
        parent_id:
          description: >-
            親見出しがある場合の `section_id` 。
          type: string
        section_chars:
          description: >-
            当該セクション本文の文字数。`get_page_section.content` と同じ
            返却範囲の Unicode 文字数を表す。
          type: integer
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 指定 revision が存在しない
- `forbidden`
  - `read` スコープ不足
  - path prefix 制約違反
- `invalid_input`
  - `path` が不正
  - `revision` が不正
- `internal_error`
  - Markdown解析や内部処理で想定外の失敗が発生した

#### 注記

- 見出しを1件も持たないページでは `sections=[]` を返す
- TOC は本文全体ではなく見出し構造と各節規模だけを返す

<a id="tool-list-pages"></a>
### 2.3 `list_pages`

#### 概要

指定した `prefix` 配下のページ一覧を、
path 昇順で取得する。

本ツールのページングは、
既存 REST API の `GET /api/pages?prefix=...` における
`forward` 指定相当の意味論に限定する。

#### 認可

- Bearer 認証が必要
- 必要スコープは `read`
- 要求 `prefix` 自体に対して認可判定を行う
- 返却候補の各 current path に対しても後段フィルタを行う

#### 入力

```yaml
type: object
required:
  - prefix
properties:
  prefix:
    description: >-
      一覧対象の絶対 path prefix 。
    type: string
  limit:
    description: >-
      取得する最大件数。未指定時は 50 、上限は 100 とする。
    type: integer
    minimum: 1
    maximum: 100
  cursor:
    description: >-
      次ページ取得の開始位置を表す path 。`prefix` 配下の path のみ許可し、
      当該 path 自身は返却範囲に含めない。
    type: string
```

#### 出力

```yaml
type: object
required:
  - items
  - has_more
properties:
  items:
    description: >-
      ページ一覧。`path` 昇順で返す。
    type: array
    items:
      type: object
      required:
        - path
        - revision
        - updated_at
        - updated_by
      properties:
        path:
          description: >-
            ページの current path 。
          type: string
        revision:
          description: >-
            最新 revision 。
          type: integer
        updated_at:
          description: >-
            最終更新日時。
          type: string
        updated_by:
          description: >-
            最終更新ユーザ名。
          type: string
  has_more:
    description: >-
      同じ `prefix` と並び順で継続取得可能かを表す。
    type: boolean
  next_cursor:
    description: >-
      次回の `cursor` に指定すべき値。`has_more=true` の場合のみ返す。
    type: string
```

#### ページング規則

- 並び順は `path` 昇順固定とする
- `cursor` 未指定時は、`prefix` の直後から取得する
- `cursor` 指定時は、`cursor` より後ろの path から取得する
- `cursor` 自身は結果に含めない
- `next_cursor` は返却 `items` の最後の `path` とする
- 続きが存在しない場合は `has_more=false` とし、`next_cursor` を返さない

#### エラー

主な失敗区分は以下とする。

- `forbidden`
  - `read` スコープ不足
  - `prefix` に対する path prefix 制約違反
- `invalid_input`
  - `prefix` が不正
  - `cursor` が不正
  - `cursor` が `prefix` 配下の path ではない
  - `limit` が 1 未満または上限超過
- `internal_error`
  - 一覧取得や内部処理で想定外の失敗が発生した

#### 注記

- `cursor` は境界値として扱い、実在ページであることまでは要求しない
- フィルタ後に結果 0 件でも正常結果とする
- 対象が存在しない場合は `items=[]` を返す

<a id="tool-search-pages"></a>
### 2.4 `search_pages`

#### 概要

全文検索インデックスに対して `query` を実行し、
その時点の上位一致結果を返す。

`search_pages` は `list_pages` と異なり、
継続取得向けの cursor ページングは持たず、
上位 `limit` 件を返す top-N 取得として扱う。

#### 認可

- Bearer 認証が必要
- 必要スコープは `read`
- `prefix` 指定がある場合は、要求 `prefix` 自体に対して認可判定を行う
- 返却候補の各 current path に対して path 制約の後段フィルタを行う

#### 入力

```yaml
type: object
required:
  - query
  - target
properties:
  query:
    description: >-
      全文検索式。
    type: string
  target:
    description: >-
      検索対象一覧。少なくとも 1 件を必須とし、`headings`、`body`、`code`、
      `front_matter` から 1 件以上を明示指定する。
    type: array
    minItems: 1
    items:
      type: string
      enum:
        - headings
        - body
        - code
        - front_matter
  prefix:
    description: >-
      検索対象を絞り込む絶対 path prefix 。未指定時は全体検索とする。
    type: string
  limit:
    description: >-
      返却する最大件数。未指定時は 20 、上限は 100 とする。
    type: integer
    minimum: 1
    maximum: 100
```

#### 出力

```yaml
type: object
required:
  - items
properties:
  items:
    description: >-
      検索結果一覧。スコア降順で返し、同点時は `path` 昇順で安定化する。
    type: array
    items:
      type: object
      required:
        - path
        - revision
        - score
        - snippet
      properties:
        path:
          description: >-
            ページの current path 。
          type: string
        revision:
          description: >-
            対応 revision 。
          type: integer
        score:
          description: >-
            検索スコア。
          type: number
        snippet:
          description: >-
            一致箇所の抜粋。
          type: string
```

#### 検索規則

- 並び順は FTS スコア降順とする
- 同点時は `path` 昇順で安定化する
- 受け取った `target` 群だけを検索対象とし、複数指定時のみ結果マージを行う
- `target` 省略時の既定値は持たず、クライアントが検索意図を明示する
- `cursor` は持たない
- `has_more` / `next_cursor` は持たない
- `limit` は返却する最大件数を表す
- path 制約または `prefix` 条件に合わない結果は返却前に除外する
- フィルタ後に結果 0 件でも正常結果とする

#### エラー

主な失敗区分は以下とする。

- `forbidden`
  - `read` スコープ不足
  - `prefix` 指定時に `prefix` 自体が path prefix 制約違反
- `invalid_input`
  - `query` が不正
  - `target` が未指定または空
  - `prefix` が不正
  - `limit` が 1 未満または上限超過
- `internal_error`
  - 検索実行、path 解決、内部処理で想定外の失敗が発生した

#### 注記

- 内部では path 制約後段フィルタ後に `limit` 件を満たすため、
  追加読取を行うことがある
- 検索結果はその時点の上位一致結果であり、継続取得の安定性は保証しない

<a id="tool-create-page"></a>
### 2.5 `create_page`

#### 概要

指定した target path に通常ページを新規作成し、
初期本文を保存する。

ブラウザ UI 向けのドラフト作成フローとは分離し、
MCP では単一要求・単一トランザクションで完結する通常ページ作成として扱う。

#### 認可

- Bearer 認証が必要
- 必要スコープは `create`
- 認可判定対象 path は作成先 `path` とする

#### 入力

```yaml
type: object
required:
  - path
  - content
properties:
  path:
    description: >-
      作成先ページの絶対 path 。
    type: string
  content:
    description: >-
      初期 Markdown 本文。
      front matter を含む raw source 全体を指定してよい。
    type: string
```

#### 出力

```yaml
type: object
required:
  - path
  - revision
  - instance_id
  - summary
properties:
  path:
    description: >-
      作成後ページの current path 。
    type: string
  revision:
    description: >-
      作成後の revision 。初期作成では 1 を返す。
    type: integer
  instance_id:
    description: >-
      ページ内容の一意性を表すインスタンスID。
    type: "string"
  summary:
    description: >-
      実行結果の要約。
    type: string
```

#### エラー

主な失敗区分は以下とする。

- `conflict`
  - 同一 current path のページが既に存在する
- `forbidden`
  - `create` スコープ不足
  - path prefix 制約違反
  - root path `/` への新規作成
- `invalid_input`
  - `path` が不正
  - `content` の指定形式が不正
  - `content` に含まれる front matter の構文またはスキーマが不正
- `internal_error`
  - 作成処理や内部処理で想定外の失敗が発生した

#### 注記

- `path` は最終作成先 path を明示する
- front matter を含む場合、`content` は本文だけでなくページ全体の raw source を渡す
- restore は初期版対象外であり、削除済み path の復元操作としては扱わない
- 親 path の存在有無は初期版 MCP の追加制約とせず、既存保存系の振る舞いに従う

<a id="tool-update-page"></a>
### 2.6 `update_page`

#### 概要

指定した current path のページ本文全体を、
与えられた `content` で置き換える。

本ツールは全文上書き更新を行い、
末尾追記専用の `append_page` や部分編集を行う `edit_page` とは別ツールとして扱う。

#### 認可

- Bearer 認証が必要
- 必要スコープは `update`
- 認可判定対象 path は対象ページの current path とする

#### 入力

```yaml
type: object
required:
  - path
  - content
properties:
  path:
    description: >-
      更新対象ページの絶対 path 。
    type: string
  content:
    description: >-
      更新後の Markdown 本文全体。
      front matter を含む raw source 全体を指定してよい。
    type: string
```

#### 出力

```yaml
type: object
required:
  - path
  - revision
  - instance_id
  - summary
properties:
  path:
    description: >-
      更新後ページの current path 。
    type: string
  revision:
    description: >-
      更新後の revision 。
    type: integer
  instance_id:
    description: >-
      ページ内容の一意性を表すインスタンスID。
    type: "string"
  summary:
    description: >-
      実行結果の要約。
    type: string
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が通常ページとして解決できない
- `conflict`
  - 対象ページがロック中
- `forbidden`
  - `update` スコープ不足
  - path prefix 制約違反
- `invalid_input`
  - `path` が不正
  - `content` の指定形式が不正
  - `content` に含まれる front matter の構文またはスキーマが不正
- `internal_error`
  - 更新処理や内部処理で想定外の失敗が発生した

#### 注記

- 初期版では amend 指定引数は公開せず、通常更新として扱う
- front matter を含む場合、`content` は本文だけでなくページ全体の raw source を渡す
- path 自体は変更せず、結果 `path` は解決後の current path を返す

<a id="tool-edit-page"></a>
### 2.7 `edit_page`

#### 概要

指定した current path のページ本文を、与えられた `operation` で編集する。

#### 認可

- Bearer 認証が必要
- 必要スコープは `update`
- 認可判定対象 path は対象ページの current path とする

#### 入力
```yaml
type: "object"
required:
  - "path"
  - "revision"
  - "instance_id"
  - "operation"
properties:
  path:
    description: >-
      編集対象のページパスを指定する
    type: "string"

  revision:
    description: >-
      編集対象のリビジョン番号を指定する。pathで指定したページの最新リビジョンが
      revisionで指定したリビジョン番号と一致する場合のみoperationで指定した編集
      が有効となる(一致しない場合はエラー)
    type: "integer"

  instance_id:
    description: >-
      ページの固有のインスタンスIDを指定する。サーバ側で編集対象ページ内容が一致
      しているか否かをチェックするために用いる (本Wikiシステムはユーザ編集におい
      て一定の条件のもとにリビジョンを変更させない amendアップデートをサポートし
      ているため)。 サーバ側で管理しているpathで指定したページのインスタンスIDが
      instance_idで指定したインスタンスIDと一致する場合のみoperationで指定した編
      集が有効となる（一致しない場合はエラー）。
    type: "string"

  operation:
    description: >-
      配列順に適用する編集操作。1件でも配列で指定する。
      sectionとanchorは初期本文の対象を参照する。
    type: "array"
    minItems: 1
    items:
      oneOf:
        - $ref: "#/definitions/replace_section"
        - $ref: "#/definitions/insert_section"
        - $ref: "#/definitions/delete_section"
        - $ref: "#/definitions/replace_text"

definitions:
  replace_section:
    description: >-
      章の置き換え。対象見出し行は `content` に含めず、見出し変更には
      `title` を使う。既定では既存サブセクションを保持し、
      サブセクション込み置換には `include_subsection: true` を指定する。
    type: "object"
    required:
      - "type"
      - "section"
      - "content"
    properties:
      type:
        description: >-
          操作種別を表す文字列を格納する
        type: "string"
        const: "replace_section"

      section:
        description: >-
          置き換え対象セクションの識別子。文字列指定または selector オブジェクト
          指定を受け付け、文字列指定は見出し文字列指定として扱う。
        $ref : "#/definitions/section_selector"

      title:
        description: >-
          置き換え後の見出し文字列。未指定時は既存見出しを維持する。
          指定時は対象セクションの見出しレベルを維持し、見出し文字列だけを
          置換する。空文字、改行、制御文字、Markdown ATX 見出し行は
          受け付けない。
        type: "string"

      content:
        description: >-
          置き換え後のセクション本文。対象セクション自身の見出し行は含めない。
          `include_subsection` が未指定または false の場合、Markdown 見出し行は
          受け付けない。`include_subsection` が true の場合、対象セクションより
          深いレベルの見出し行だけを受け付け、同階層以上の見出し行は
          受け付けない。
        type: "string"

      include_subsection:
        description: >-
          対象セクション配下のサブセクションを置換範囲に含めるかを指定する。
          未指定時は false とし、既存サブセクションを保持する。true の場合は
          対象セクション配下のサブセクションを含めて置換する。
        type: "boolean"

  insert_section:
    description: >-
      章の挿入
    type: "object"
    required:
      - "type"
      - "anchor"
      - "placement"
      - "content"
    properties:
      type:
        description: >-
          操作種別を表す文字列を格納する
        type: "string"
        const: "insert_section"

      anchor:
        description: >-
          挿入位置の基準となるセクションの識別子。文字列指定または selector
          オブジェクト指定を受け付け、文字列指定は見出し文字列指定として扱う。
        $ref : "#/definitions/section_selector"

      placement:
        description: >-
          anchor に対する挿入位置。before / after を指定する。
        type: "string"
        enum:
          - "before"
          - "after"
      content:
        description: >-
          挿入するセクション本文。見出し行を含む完全なセクションを指定する。
        type: "string"

  delete_section:
    description: >-
      章の削除
    type: "object"
    required:
      - "type"
      - "section"
    properties:
      type:
        description: >-
          操作種別を表す文字列を格納する
        type: "string"
        const: "delete_section"

      section:
        description: >-
          削除対象セクションの識別子。文字列指定または selector オブジェクト指定
          を受け付け、文字列指定は見出し文字列指定として扱う。
        $ref : "#/definitions/section_selector"

  # replace_lines:
  #   description: >-
  #     行の置き換え
  #   type: "object"
  #
  # insert_lines:
  #   description: >-
  #     行の挿入
  #   type: "object"
  #
  # delete_lines:
  #   description: >-
  #     行の削除
  #   type: "object"
  #
  # unified_diff:
  #   description: >-
  #     unified diff形式での編集指示
  #   type: "string"

  replace_text:
    description: >-
      テキストの置き換え
    type: "object"
    required:
      - "type"
      - "old_text"
      - "new_text"
    properties:
      type:
        description: >-
          操作種別を表す文字列を格納する
        type: "string"
        const: "replace_text"

      old_text:
        description: >-
          置き換え対象の文字列。本文中に一意に存在する必要がある。
        type: "string"

      new_text:
        description: >-
          置き換え後の文字列。
        type: "string"

      occurrence:
        description: >-
          複数一致時の対象指定。"first" / "all"。
          未指定時は "first" と同じ。
        type: "string"
        enum:
          - "first"
          - "all"

  section_selector:
    oneOf:
      - type: "string"
        description: >-
          見出し文字列そのものを指定する。
      - type: "object"
        required:
          - "by"
          - "value"
        properties:
          by:
            description: >-
              セクション識別方式。
            type: "string"
            enum:
              - "title"
              - "id"
          value:
            description: >-
              `by` で指定した方式に対応する値。
            type: "string"
```

#### 出力
```yaml
type: object
required:
  - path
  - revision
  - instance_id
  - summary
properties:
  path:
    description: >-
      更新後ページの current path 。
    type: string
  revision:
    description: >-
      編集後の revision 。
    type: integer
  instance_id:
    description: >-
      編集後のインスタンスID。
    type: "string"
  summary:
    description: >-
      実行結果の要約。
    type: string
```

#### エラー
主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が通常ページとして解決できない
- `conflict`
  - 対象ページがロック中
- `forbidden`
  - `update` スコープ不足
  - path prefix 制約違反
- `invalid_input`
  - `path` が不正
  - `operation` の指定形式が不正
  - `replace_section.title` が不正
  - `replace_section.content` に許可されない Markdown 見出し行が含まれる
  - `replace_section.include_subsection` の型が不正
- `not_latest_revision`
  - `revision`が最新リビジョンを指していない
- `instance_id_not_match`
  - `instance_id`がサーバが指しているものと異なる
- `internal_error`
  - 更新処理や内部処理で想定外の失敗が発生した

#### 注記

`operation` は非空配列とし、単一object・空配列・null・入れ子配列を拒否する。
path・revision・instance_idは要求全体で一組とする。

各操作は配列順に適用する。section／anchorのID・titleは初期本文に結び付け、
前方への挿入・削除やreplace_section.titleでも元の対象を保持する。
本文範囲と子節は各操作時点のMarkdownから求める。
同じ元節へ同レベル節X、Yを連続挿入すると、afterはA・Y・X、beforeはX・Y・Aとなる。

元対象の削除後の参照はnot_found、任意replace_textによる見出し行の変更や
構文文脈の変化で追跡不能になった後の参照はinvalid_inputとする。
置換前後が同一文字列なら追跡を維持する。見出し名変更にはreplace_section.titleを使う。
新設節や変更後titleは初期selectorでは指定できないため、保存後の新しい要求で参照する。
replace_textは中間本文を検索し、allはその操作開始時の一致範囲だけへ一度適用する。

全件成功した最終本文を一度保存する。途中失敗は部分保存せず、
最終本文が元と同じ場合も既存どおり新revision／instance_idを保存する。
成功summaryは `page edited (N operations)` とする。
操作適用エラーは既存codeを維持し、messageへ1始まりの
`operation N failed: ...` を付ける。全体の認可・競合・保存エラーには操作番号を付けない。
正本commit後の同期失敗はinternal_errorでも保存済みの場合があるため、
クライアントはget_pageで状態を再取得してから再試行を判断する。

配列入力の例（selectorは取得済みの初期TOCを参照する）。

```json
{
  "path": "/例",
  "revision": 7,
  "instance_id": "取得した値",
  "operation": [
    {"type": "insert_section", "anchor": {"by": "id", "value": "s-001"}, "placement": "before", "content": "# 注意\n注意事項\n"},
    {"type": "replace_section", "section": {"by": "id", "value": "s-002"}, "content": "使い方の更新本文"}
  ]
}
```

<a id="tool-append-page"></a>
### 2.8 `append_page`

#### 概要

指定した current path のページ本文末尾へ、
`content` をそのまま追記する。

本ツールは全文置換ではなく末尾追記専用であり、
内部では要求仕様に従って amend 相当保存の可否を判定する。

#### 認可

- Bearer 認証が必要
- 必要スコープは `append`
- 認可判定対象 path は対象ページの current path とする

#### 入力

```yaml
type: object
required:
  - path
  - content
properties:
  path:
    description: >-
      追記対象ページの絶対 path 。
    type: string
  content:
    description: >-
      末尾へ追記する文字列。全文置換ではなく差分文字列を表す。
      先頭 front matter を含む既存 raw source に対する本文末尾追記として扱う。
    type: string
```

#### 出力

```yaml
type: object
required:
  - path
  - revision
  - instance_id
  - summary
properties:
  path:
    description: >-
      更新後ページの current path 。
    type: string
  revision:
    description: >-
      保存後の revision 。amend 相当で処理された場合は既存 revision を返す。
    type: integer

  instance_id:
    description: >-
      ページ内容の一意性を表すインスタンスID。
    type: "string"

  summary:
    description: >-
      実行結果の要約。必要に応じて amend 相当で処理されたかを含む。
    type: string
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が通常ページとして解決できない
- `conflict`
  - 対象ページがロック中
  - 保存直前に最新 revision 競合が解消しない
  - amend 相当保存が許可されない内部条件に達した
- `forbidden`
  - `append` スコープ不足
  - path prefix 制約違反
- `invalid_input`
  - `path` が不正
  - `content` が空
  - `content` の指定形式が不正
  - 追記後の raw source に含まれる front matter の構文またはスキーマが不正
- `internal_error`
  - 追記処理や内部処理で想定外の失敗が発生した

#### 注記

- 追記位置は常に最新本文の末尾とする
- サーバは `content` の内部改変を原則行わず、改行自動補完も行わない
- 行を分けて追記したい場合の改行は、クライアントが `content` に含める
- front matter は raw source 先頭の一部として保持され、`append_page` は既存 front matter を直接編集する用途には用いない
- amend 相当保存の可否は内部判定で決め、公開入力として amend 指定は受け付けない

<a id="tool-rename-page"></a>
### 2.9 `rename_page`

#### 概要

指定した current path のページを、
`rename_to` で指定した新しい path へリネームする。

初期版では rename のみを扱い、restore は別操作として公開しない。

#### 認可

- Bearer 認証が必要
- 必要スコープは `update`
- 認可判定対象 path は current path と `rename_to` path の双方とする

#### 入力

```yaml
type: object
required:
  - path
  - rename_to
properties:
  path:
    description: >-
      リネーム対象ページの current path 。
    type: string
  rename_to:
    description: >-
      変更後の最終到達 path 。
    type: string
```

#### 出力

```yaml
type: object
required:
  - path
  - revision
  - instance_id
  - summary
properties:
  path:
    description: >-
      リネーム後ページの current path 。
    type: string
  revision:
    description: >-
      実行結果に対応する revision 。
    type: integer
  instance_id:
    description: >-
      ページ内容の一意性を表すインスタンスID。
    type: "string"
  summary:
    description: >-
      実行結果の要約。必要に応じて変更前 path を含む。
    type: string
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が通常ページとして解決できない
- `conflict`
  - `rename_to` に同一 current path のページが既に存在する
  - 対象ページがロック中
- `forbidden`
  - `update` スコープ不足
  - current path または `rename_to` に対する path prefix 制約違反
  - root page を移動元とするリネーム
- `invalid_input`
  - `path` が不正
  - `rename_to` が不正
  - `rename_to` が current path 配下になる
- `internal_error`
  - リネーム処理や内部処理で想定外の失敗が発生した

#### 注記

- `rename_to` は最終到達 path を明示する引数であり、親ディレクトリ指定としては扱わない
- `path` と `rename_to` が正規化後に完全一致する場合は no-op として成功扱いしてよい
- restore を意図した要求は初期版では `rename_page` に持ち込まない

<a id="tool-get-page-section"></a>
### 2.10 `get_page_section`

#### 概要

指定した current path の Markdown 本文から、
特定セクションに対応する部分本文を取得する。

このツールはページ全体取得とは別に、
ページ内の構造化された一部分だけを取得するために用いる。

#### 認可

- Bearer 認証が必要
- 必要スコープは `read`
- 認可判定対象 path は対象ページの current path とする

#### 入力

`section` は文字列指定または selector オブジェクト指定を受け付ける。
文字列指定は見出し文字列指定として扱う。

```yaml
type: object
required:
  - path
  - section
properties:
  path:
    description: >-
      対象ページの絶対 path 。
    type: string
  section:
    description: >-
      ページ内セクションを指す識別子。文字列指定は見出し文字列指定として扱う。
    oneOf:
      - type: string
        description: >-
          見出し文字列そのものを指定する。
      - type: object
        required:
          - by
          - value
        properties:
          by:
            description: >-
              セクション識別方式。
            type: string
            enum:
              - title
              - id
          value:
            description: >-
              `by` で指定した方式に対応する値。
            type: string
  revision:
    description: >-
      対象 revision 。未指定時は最新 revision を参照する。
    type: integer
```

selector の初期案は以下とする。

```json
{ "by": "title", "value": "インストール手順" }
{ "by": "id", "value": "s-003" }
```

#### 出力

```yaml
type: object
required:
  - path
  - revision
  - section
  - content
properties:
  path:
    description: >-
      解決後の current path 。
    type: string
  revision:
    description: >-
      実際に参照した revision 。
    type: integer
  section:
    description: >-
      解決後のセクション識別情報。
    type: object
    required:
      - id
      - title
      - level
      - ordinal
    properties:
      id:
        description: >-
          解決後の `section_id` 。
        type: string
      title:
        description: >-
          見出し文字列。
        type: string
      level:
        description: >-
          見出しレベル。
        type: integer
      ordinal:
        description: >-
          文書順の出現番号。
        type: integer
      parent_id:
        description: >-
          親見出しがある場合の `section_id` 。
        type: string
  content:
    description: >-
      セクション本文。
    type: string
```

#### 返却範囲

`content` の返却範囲は以下を基本とする。

- 対象見出し自身の行は `content` に含めない
- 対象見出しの直後から、
  次に現れる「同レベル以上の見出し」の直前までを返す
- 対象見出し配下の子見出しおよびその本文は含める
- ページ末尾まで次の同レベル以上見出しが現れない場合は、末尾まで返す

この方針により、
「ある節の本文全体」を自然に切り出せる形を優先する。

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 指定 revision が存在しない
  - `by=id` 指定時に該当 `section_id` が存在しない
- `forbidden`
  - `read` スコープ不足
  - path prefix 制約違反
- `invalid_input`
  - `path` が不正
  - `section` 指定形式が不正
  - `by` に未定義の selector が指定された
  - `by=title` 指定時に同名見出しが複数存在し一意に解決できない
  - `by=title` 指定時に空文字または正規化後空文字となる
  - `revision` が不正
- `internal_error`
  - Markdown解析や内部処理で想定外の失敗が発生した

#### 注記

- `get_page_toc` で取得した `section_id` は `get_page_section` で利用できる

---

### アセット tool 共通仕様

アセット tool のメタ情報 object は以下を基本とする。

```yaml
type: object
required:
  - asset_id
  - path
  - file_name
  - mime_type
  - size
  - timestamp
  - username
properties:
  asset_id:
    description: >-
      アセットID。REST API の id 相当を MCP 外部契約上明確化した名称。
    type: string
  path:
    description: >-
      アセットが紐付くページの現在有効な絶対 path 。
    type: string
  file_name:
    description: >-
      ページ内のアセットファイル名。
    type: string
  mime_type:
    description: >-
      アセット本体の MIME 種別。
    type: string
  size:
    description: >-
      raw binary size。byte 単位。
    type: number
  timestamp:
    description: >-
      アセットがアップロードされたローカル日時。
      timezone offsetを含まないYYYY-MM-DDTHH:MM:SS形式の
      秒精度文字列とする。timezone offsetまたはZを含む
      REST APIのRFC3339形式とは異なる。
    type: string
  username:
    description: >-
      アセットをアップロードしたユーザ名。
    type: string
  instance_id:
    description: >-
      アセット本体の内容識別子。存在する場合のみ返す任意項目。
    type: string
```

- `path` は全アセット tool で必須とし、Wiki ページの絶対 path を表す
- `file_name` は単一アセットを指定する tool で必須とし、空文字、`/`、バックスラッシュを許可しない
- `mime_type` は `upload_page_asset` で必須とし、空文字を許可しない
- `data_base64` は `upload_page_asset` の入力と `get_page_asset_data` の出力でのみ扱う
- `asset_id` は初期版では入力項目として採用せず、レスポンス上の識別情報としてのみ扱う
- `instance_id` は存在する場合のみ返し、欠損時は JSON 項目を省略する

---

<a id="tool-list-page-assets"></a>
### 2.11 `list_page_assets`

#### 概要

入力 `path` で指定されたページに紐付く通常アセットのメタ情報一覧を返す。
一覧にはアセット本体を含めない。

#### 認可

- Bearer 認証が必要
- 必要スコープは `read`
- 認可判定対象 path は正規化後の入力 `path` とする

#### 入力

```yaml
type: object
required:
  - path
properties:
  path:
    description: >-
      アセット一覧を取得するページの絶対 path 。
    type: string
```

#### 出力

```yaml
type: object
required:
  - path
  - items
properties:
  path:
    description: >-
      入力 path を正規化して解決された対象ページの絶対 path 。
    type: string
  items:
    description: >-
      対象ページに紐付く通常アセットのメタ情報一覧。
    type: array
    items:
      description: >-
        アセットメタ情報 object。
      type: object
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が削除済みページとして解決された
- `forbidden`
  - `read` スコープ不足
  - path prefix 制約違反
- `invalid_input`
  - `path` が不正
- `internal_error`
  - 一覧取得処理や内部処理で想定外の失敗が発生した

#### 注記

- 削除済みアセットは返さない
- `data_base64` は返さない

---

<a id="tool-resolve-page-asset"></a>
### 2.12 `resolve_page_asset`

#### 概要

入力 `path` で指定されたページ内で、`file_name` に一致する通常アセットを解決し、最小識別情報を返す。

#### 認可

- Bearer 認証が必要
- 必要スコープは `read`
- 認可判定対象 path は正規化後の入力 `path` とする

#### 入力

```yaml
type: object
required:
  - path
  - file_name
properties:
  path:
    description: >-
      アセットが紐付くページの絶対 path 。
    type: string
  file_name:
    description: >-
      解決対象のアセットファイル名。
    type: string
```

#### 出力

```yaml
type: object
required:
  - asset_id
  - path
  - file_name
properties:
  asset_id:
    description: >-
      解決されたアセットID。
    type: string
  path:
    description: >-
      入力 path を正規化して解決された対象ページの絶対 path 。
    type: string
  file_name:
    description: >-
      解決されたアセットファイル名。
    type: string
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が削除済みページとして解決された
  - 対象アセットが存在しない
  - 対象アセットが削除済みである
- `forbidden`
  - `read` スコープ不足
  - path prefix 制約違反
- `invalid_input`
  - `path` が不正
  - `file_name` が不正
- `internal_error`
  - 解決処理や内部処理で想定外の失敗が発生した

#### 注記

- 詳細メタ情報は `get_page_asset_metadata` で取得する
- `data_base64` は返さない

---

<a id="tool-get-page-asset-metadata"></a>
### 2.13 `get_page_asset_metadata`

#### 概要

入力 `path` で指定されたページ内で、`file_name` に一致する通常アセットの詳細メタ情報を返す。
アセット本体は返さない。

#### 認可

- Bearer 認証が必要
- 必要スコープは `read`
- 認可判定対象 path は正規化後の入力 `path` とする

#### 入力

```yaml
type: object
required:
  - path
  - file_name
properties:
  path:
    description: >-
      アセットが紐付くページの絶対 path 。
    type: string
  file_name:
    description: >-
      メタ情報を取得するアセットファイル名。
    type: string
```

#### 出力

```yaml
type: object
required:
  - asset
properties:
  asset:
    description: >-
      アセットメタ情報 object。
    type: object
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が削除済みページとして解決された
  - 対象アセットが存在しない
  - 対象アセットが削除済みである
- `forbidden`
  - `read` スコープ不足
  - path prefix 制約違反
- `invalid_input`
  - `path` が不正
  - `file_name` が不正
- `unsupported`
  - `asset_id` 単独入力によるメタ情報取得要求
- `internal_error`
  - メタ情報取得処理や内部処理で想定外の失敗が発生した

#### 注記

- `instance_id` は存在する場合のみ返す任意項目とし、欠損時は JSON 項目を省略する
- `data_base64` は返さない

---

<a id="tool-get-page-asset-data"></a>
### 2.14 `get_page_asset_data`

#### 概要

入力 `path` で指定されたページ内で、`file_name` に一致する通常アセットのメタ情報と本体を返す。
本体は `data_base64` に base64 文字列として格納する。

#### 認可

- Bearer 認証が必要
- 必要スコープは `read`
- 認可判定対象 path は正規化後の入力 `path` とする

#### 入力

```yaml
type: object
required:
  - path
  - file_name
properties:
  path:
    description: >-
      アセットが紐付くページの絶対 path 。
    type: string
  file_name:
    description: >-
      本体を取得するアセットファイル名。
    type: string
```

#### 出力

```yaml
type: object
required:
  - asset
  - data_base64
properties:
  asset:
    description: >-
      アセットメタ情報 object。
    type: object
  data_base64:
    description: >-
      アセット本体 bytes を base64 エンコードした文字列。
    type: string
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が削除済みページとして解決された
  - 対象アセットが存在しない
  - 対象アセットが削除済みである
- `forbidden`
  - `read` スコープ不足
  - path prefix 制約違反
- `conflict`
  - 保存済みアセットの raw binary size が 10MiB を超過している
  - MCP result size または transport payload の制約超過を返却前に検出した
- `invalid_input`
  - `path` が不正
  - `file_name` が不正
- `unsupported`
  - `asset_id` 単独入力による本体取得要求
  - 条件付き取得入力の要求
- `internal_error`
  - 本体取得、base64 encode、内部 I/O などで想定外の失敗が発生した
  - transport 送信時または serialize 後に初めて payload 制約超過が判明した

#### 注記

- `data_base64` とアセット本体 bytes は監査ログ、tracing、エラー summary に含めない
- REST API の redirect、ETag、条件付き GET は MCP tool の外部契約に持ち込まない

---

<a id="tool-upload-page-asset"></a>
### 2.15 `upload_page_asset`

#### 概要

入力 `path` で指定されたページへ、新規アセットをアップロードする。
アップロード対象の本体は `text` または `data_base64` をちょうど一方指定して受け取り、ローカルファイルパス入力は受け付けない。

#### 認可

- Bearer 認証が必要
- 必要スコープは `update`
- 認可判定対象 path は正規化後の入力 `path` とする
- `ReadOnly` ユーザでは拒否する

#### 入力

```yaml
type: object
required:
  - path
  - file_name
  - mime_type
oneOf:
  - required: [text]
    not: {required: [data_base64]}
  - required: [data_base64]
    not: {required: [text]}
properties:
  path:
    description: >-
      アセットをアップロードするページの絶対 path 。
    type: string
  file_name:
    description: >-
      アップロードするアセットファイル名。
    type: string
  mime_type:
    description: >-
      アップロードするアセットの MIME 種別。
    type: string
  text:
    description: >-
      UTF-8で保存するtext/*の直接入力。空文字を許可する。
      charsetは未指定またはUTF-8。data_base64と排他。
    type: string
  data_base64:
    description: >-
      アセット本体 bytes を base64 エンコードした文字列。
    type: string
```

#### 出力

```yaml
type: object
required:
  - asset
  - summary
properties:
  asset:
    description: >-
      アセットメタ情報 object。
    type: object
  summary:
    description: >-
      実行結果の要約。
    type: string
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が削除済みページとして解決された
- `conflict`
  - 同名アセットが既にページ内に存在する
  - 対象ページがロック中
- `forbidden`
  - `update` スコープ不足
  - path prefix 制約違反
  - `ReadOnly` ユーザによる upload
- `invalid_input`
  - text/data_base64の両方指定または両方欠落
  - textのMIME/charset不正、UTF-8 byte長10MiB超過
  - `path` が不正
  - `file_name` が不正
  - `mime_type` が不正
  - `data_base64` が base64 として不正
  - decode 後の raw binary size が 10MiB を超過した
  - decode 前の `data_base64` byte 長が 13,981,016 bytes を超過した
- `unsupported`
  - ローカルファイルパス入力の要求
  - 既存アセットの上書き upload 要求
  - lock token 入力の要求
- `internal_error`
  - upload 処理、base64 decode、内部 I/O などで想定外の失敗が発生した

#### 注記

- null・数値・配列等の本文型不正はhandler前のJSON-RPC invalid params（-32602）。欠落/両方指定はtool invalid_input
- textは空文字を許可し、BOM・Unicode・改行・前後空白をそのままUTF-8 bytesとして保存する。Base64空は拒否する
- text側はMIME構文、前後空白・ASCII制御文字の拒否、text型とwildcardなしのsubtypeを検証する。charsetは重複不可・未指定またはUTF-8（大小文字不問、quoted可）。他の有効parametersと入力MIME表記は保持する
- Base64側のMIME検証は従来どおり空拒否のみ。sniffing・拡張子照合・文字コード変換は行わない
- 認可・ページ解決後、file_name→MIME空→本文排他→text MIME/容量またはBase64検証→txn内の最新認可・状態・lock・同名の順とする
- UTF-8 bytesの上限10MiBとHTTP body全体の上限64MiBを区別する。HTTP上限超過は413でtoolへ到達しない
- 新規アセット upload のみ扱い、上書きは扱わない
- ロック中ページへの upload では lock token 入力を受け付けず `conflict` とする
- `text`、`data_base64` とアセット本体 bytes は監査ログ、tracing、エラー summary に含めない
- 成功時 `summary` はサーバ生成の短い固定的な文言とし、入力本体や base64 を含めない

---

<a id="tool-delete-page-asset"></a>
### 2.16 `delete_page_asset`

#### 概要

入力 `path` で指定されたページ内で、`file_name` に一致する通常アセットを削除する。

#### 認可

- Bearer 認証が必要
- 必要スコープは `delete`
- 認可判定対象 path は正規化後の入力 `path` とする
- `ReadOnly` ユーザでは拒否する

#### 入力

```yaml
type: object
required:
  - path
  - file_name
properties:
  path:
    description: >-
      アセットが紐付くページの絶対 path 。
    type: string
  file_name:
    description: >-
      削除するアセットファイル名。
    type: string
```

#### 出力

```yaml
type: object
required:
  - asset_id
  - path
  - file_name
  - summary
properties:
  asset_id:
    description: >-
      削除したアセットID。
    type: string
  path:
    description: >-
      入力 path を正規化して解決された対象ページの絶対 path 。
    type: string
  file_name:
    description: >-
      削除したアセットファイル名。
    type: string
  summary:
    description: >-
      実行結果の要約。
    type: string
```

#### エラー

主な失敗区分は以下とする。

- `not_found`
  - 対象ページが存在しない
  - 対象 path が削除済みページとして解決された
  - 対象アセットが存在しない
  - 対象アセットが削除済みである
- `conflict`
  - 対象ページがロック中
- `forbidden`
  - `delete` スコープ不足
  - path prefix 制約違反
  - `ReadOnly` ユーザによる delete
- `invalid_input`
  - `path` が不正
  - `file_name` が不正
- `unsupported`
  - `asset_id` 単独入力による削除要求
  - lock token 入力の要求
  - 物理削除、復元、再削除の要求
- `internal_error`
  - 削除処理や内部処理で想定外の失敗が発生した

#### 注記

- `path + file_name` で指定された通常アセットのみ削除する
- ロック中ページへの delete では lock token 入力を受け付けず `conflict` とする
- 成功時 `summary` はサーバ生成の短い固定的な文言とし、内部保存パスや削除済みアセットの内部状態を含めない

---

<a id="tool-get-page-state"></a>
### 2.17 `get_page_state`

現在の通常ページの軽量状態を取得する。Bearerの`read`（又は包含する`write`）と、
正規化した入力pathへのpath prefix認可を要求する。ReadOnlyユーザも利用可能。

#### 入力

```yaml
type: object
required: [path]
properties:
  path:
    type: string
    description: 共通Wikiパス規則に従う絶対path
```

#### 出力

```json
{"path":"/example","revision":7,"instance_id":"source-instance","state_token":"v1:<identity-digest>:<state-digest>"}
```

- `instance_id`は旧sourceで欠落する場合だけnull。`state_token`は常にstring。
- tokenは`v1:`と小文字64桁hexの識別部、`:`、小文字64桁hexの状態部（計132 bytes）。
  上記例のdigest表記は説明用であり、実際にはサーバの返却値をそのまま使う。
- path別の不透明な比較値であり、認証・権限証明ではない。本文・page_id・移動先は返さない。
- `invalid_input`はpath不正、`forbidden`は現在のscope・prefix・認証主体の利用不能、
  `not_found`は通常ページとして取得不能、`internal_error`は読取失敗・保存値不整合。
  不存在・削除・draftの詳細を区別しない。

<a id="tool-wait-page-change"></a>
### 2.18 `wait_page_change`

同じpathについて、取得済み状態からの差を期限付きで待つ。認可は`get_page_state`と同じ。
toolの待機中も、現在のtoken・不変主体・scope・prefixを各snapshotで再確認する。

#### 入力

```yaml
type: object
required: [path, expected_state]
properties:
  path:
    type: string
  expected_state:
    type: string
    description: get_page_state又は直前timeoutのstate_token
  timeout_seconds:
    type: integer
    minimum: 1
    maximum: 30
    default: 30
```

timeout_secondsのnull・小数・範囲外、tokenの長さ・版・hex不正は`invalid_input`。
必須field欠落・path等の型不正のようにtool引数のdeserialize自体が失敗する場合は、
既存tool同様にJSON-RPCのInvalidParamsとして拒否される。

#### 正常出力

```json
{"path":"/example","status":"timeout","change":null,"state":{"revision":7,"instance_id":"source-instance","state_token":"v1:<identity-digest>:<state-digest>"}}
```

- `status`は`changed`又は`timeout`。正常timeoutは`isError: false`。
- `changed`の`change`は、同一ページの更新・amendなら`updated`、同じpathの別ページなら
  `replaced`、通常ページを取得不能なら`unavailable`。
- `state`は`revision`・nullable `instance_id`・`state_token`。`unavailable`だけnull。
- `timeout`では基準と同じ状態を返す。再待機ではそのtokenを維持し、間に状態を取得し直して
  基準を更新しない。これにより取得直後・再呼出し前に入った持続する差を検出できる。
- 初期`get_page_state`がnot_foundのpathには基準を発行せず、未存在pathの作成待ちは扱わない。
  取得後からwait初回比較までの消失は正常な`changed/unavailable`。
- unavailableは削除・rename・draft等を区別せず、元pathの状態だけを返す。移動先を追跡しない。
- 入力・認可・内部エラーは状態取得と同じ分類。待機中の認証利用不能はtoolの固定forbidden。
  HTTP受付時の認証失敗401とは区別する。認可拒否をchangedと解釈しない。

#### 観測保証・終了

- 短い同一read snapshotの照合と最大500msの非同期sleepを繰り返す。sleep中に共有lock・txnを
  保持しない。期限はmonotonic clockで要求処理開始から計測する。
- 最後の認可付きsnapshotで観測した差を優先し、差がなければ期限到達でtimeoutを返す。
  OS・DB処理の遅延を含む厳密な壁時計の返信期限は保証しない。
- 全中間イベント・順序・件数を配送しない。観測間に同じ状態へ戻る変化は見えない場合がある。
- 正本commitを参照し、派生候補・FTS同期の成功は待たない。changed後の本文再取得では、
  さらに新しい版や別のエラーになる可能性がある。
- request cancel又はfuture破棄で待機を終了する。切断がcancelへ伝播しない接続では、
  期限又はsession終了によるfuture破棄まで処理が残ることがある。
- 両toolはreadOnly・non-destructive・idempotent・closed-worldのannotationsを持つ。
  token再確認でスライディングTTLを延長しない。基準値・本文・資格情報を監査へ記録しない。
- 分単位の人間の編集待ちは、利用者の待機指示が続く間の有限な再呼出しで行う。
  利用者が停止を求めたら終了し、停止中のエージェントを起動する通知とは区別する。

<a id="tool-get-agent-instructions"></a>
### 2.19 `get_agent_instructions`

固定組み込みリソース `luwiki://<authority>/builtin/agent-skill` の代替取得ツール。
運用指示本文は標準リソースと共通で、コンパイル時に内包する。

- 引数は省略または空オブジェクト `{}`。未定義の引数は JSON-RPC `-32602`。
- Bearer の `read`、またはそれを包含する `write` を要求する。read を持つ ReadOnly ユーザーは利用できる。ページ用 path prefix や resource ACL は適用しない。
- 成功時は `isError: false`。標準取得の contents と同じ `uri`、`mime_type`、`content` を JSON object として `result.content[0].text` に格納する。
- scope 不足は `isError: true` の `forbidden`。Bearer 不在・無効・失効は transport の HTTP 401。
- annotations は readOnly、non-destructive、idempotent、closed-world。

## 3. 改訂方針

- 7.1 `get_page_section` の仕様確定は、本書 2.2 および 2.10 を正式化する形で反映する
- 7.2 `list_pages` / `search_pages` のページング仕様確定は、本書 2.3 と 2.4 を正式化する形で反映する
- アセット 6 tool の追加は、MS1.2 の外部契約確定結果および MS1.3 の内部設計方針を本書へ反映する形で扱う
- アセット tool の内部責務分割、serde での未知フィールド扱い、`instance_id` 欠損時の JSON 表現、decode 前 base64 文字列長の防御的上限、MCP result size 超過時の検出箇所、監査ログ内部分類は MS1.3 の方針に従う
- 参照系・更新系の各ツール節は本書へ集約済みであり、今後の改訂は設計差し戻しまたは互換性を崩さない補足に限定する
