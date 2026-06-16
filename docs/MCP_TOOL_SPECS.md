# MCPツール仕様

本書では、MCPクライアントへ公開するツール仕様を定義する。

`docs/MCP_INTERFACE_AND_ERROR_DESIGN.md` が内部設計として
ツール責務、入出力モデル、エラー分類の設計意図を整理するのに対し、
本書は外部契約としてのツール名、入力、出力、エラー、および注記を定義する。

MCP標準`prompts/list`と`prompts/get`はtoolではなく標準prompts操作である。
promptsの外部契約は`docs/MCP_PROMPT_SPECS.md`を参照する。
MCP標準`resources/list`と`resources/read`はtoolではなく標準resources操作である。
resourcesの外部契約は`docs/MCP_RESOURCE_SPECS.md`を参照する。

初期版MCPの外部仕様は、`docs/REQUIREMENTS.md`、`docs/MCP_SPEC_DECISION_TASKS.md`、
および各設計分冊で確定した内容を本書へ集約する。

---

## ツール一覧

  | ツール | 用途
  |:--|:--
  | `get_page` | [ページ全体の取得](#tool-get-page)
  | `get_page_toc` | [ページ見出し構造の取得](#tool-get-page-toc)
  | `list_pages` | [ページ一覧の取得](#tool-list-pages)
  | `search_pages` | [ページ検索の実行](#tool-search-pages)
  | `create_page` | [ページ作成](#tool-create-page)
  | `update_page` | [ページ更新](#tool-update-page)
  | `edit_page` | [ページ編集](#tool-edit-page)
  | `append_page` | [ページ追記](#tool-append-page)
  | `rename_page` | [ページリネーム](#tool-rename-page)
  | `get_page_section` | [特定セクション本文の取得](#tool-get-page-section)

---

## 1. 共通事項

### 1.1 基本方針

- MCP は `run` コマンドで明示的に有効化された場合のみ公開する
- MCP は Bearer 認証を前提とし、Basic 認証は受け付けない
- 本書で定義するページtoolはpathベースで公開し、外部へ`page_id`を露出しない
- pathベースtoolの認可はBearerスコープとpath prefix制約の両方で判定する
- MCP promptsはread scopeを要求するが、ページ用path prefix制約を適用しない
- 監査ログは MCP 操作および関連する認可失敗を対象として記録する

### 1.2 公開するツール

初期版では、少なくとも以下のツールを公開する。

- `get_page`
- `get_page_toc`
- `list_pages`
- `search_pages`
- `create_page`
- `update_page`
- `edit_page`
- `append_page`
- `rename_page`
- `get_page_section`

### 1.3 初期版から除外する機能

以下は初期版ではツールとして公開しない。

- 削除済みページ参照
- restore
- アセット操作
- ロック操作
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
      編集操作を単一指定する。初期版では `replace_section`、`insert_section`、
      `delete_section`、`replace_text` を公開対象とする。
    oneOf:
      - $ref: "#/definitions/replace_section"
      - $ref: "#/definitions/insert_section"
      - $ref: "#/definitions/delete_section"
      # - $ref: "#/definitions/replace_lines"
      # - $ref: "#/definitions/insert_lines"
      # - $ref: "#/definitions/delete_lines"
      # - $ref: "#/definitions/unified_diff"
      - $ref: "#/definitions/replace_text"

definitions:
  replace_section:
    description: >-
      章の置き換え
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

      content:
        description: >-
          置き換え後のセクション本文。
          対象見出し行は保持される。本文部分のみ置き換える。
        type: "string"

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
- `not_latest_revision`
  - `revision`が最新リビジョンを指していない
- `instance_id_not_match`
  - `instance_id`がサーバが指しているものと異なる
- `internal_error`
  - 更新処理や内部処理で想定外の失敗が発生した

#### 注記

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

## 3. 改訂方針

- 7.1 `get_page_section` の仕様確定は、本書 2.2 および 2.10 を正式化する形で反映する
- 7.2 `list_pages` / `search_pages` のページング仕様確定は、本書 2.3 と 2.4 を正式化する形で反映する
- 参照系・更新系の各ツール節は本書へ集約済みであり、今後の改訂は設計差し戻しまたは互換性を崩さない補足に限定する
