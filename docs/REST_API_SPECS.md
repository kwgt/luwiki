# REST API仕様

本書ではアプリケーションで実装するREST APIの仕様について定義する。  

--- --- --- --- --- --- --- --- --- --- --- --- --- --- ---
## 共通事項
### 認証

`/api`配下のAPIはすべてBasic認証が必須である。

### エラー時のレスポンス
リクエストに失敗した場合のレスポンスはJSONで要因を示す情報を返す。この情報は、メッセージ表示に用いることを前提とし、人間による可読性を優先したものとする。
このため、リクエスト失敗時のレスポンスヘッダの `Content-Type`は "application/json"固定となり、ボディには以下のスキーマに則ったJSONが返される。

```yaml
type: "object"
required:
  - reason
properties:
  reason:
    description: >-
      失敗の要因を表す文が格納される。
    type: "string"
```

### ページパスの指定
クエリーパラメータでページパスを渡す場合は絶対パスで渡すことを前提としている(それ以外はエラー)。

--- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

## エンドポイント一覧

  | メソッド | エンドポイント | 用途
  |:--|:--|:--
  |POST   | `/api/pages?path={page_path}`                     | [ドラフトページの作成](#create-page)
  |GET    | `/api/pages?prefix={page_path}[&forward={page_path}][&rewind={page_path}][&limit={number}][&with_deleted={boolean}]` | [ページの一覧取得](#get-pages)
  |GET    | `/api/pages/deleted?path={page_path}`             | [削除済みページの一覧取得](#get-deleted-pages)
  |GET    | `/api/pages/template` | [テンプレートの一覧取得](#get-template-pages)
  |GET    | `/api/pages/search?expr={expression}[&target={targets}][&with_deleted={boolean}][&all_revision={boolean}]` | [ページの検索](#search-pages)
  |GET    | `/api/pages/{page_id}/source[?rev={revision}]`    | [ページソースの取得](#get-page-source)
  |PUT    | `/api/pages/{page_id}/source[?amend={boolean}]`   | [ページソースの更新](#update-page-source)
  |GET    | `/api/pages/{page_id}/meta[?rev={revision}]`      | [ページのメタ情報の取得](#get-page-metadata)
  |GET    | `/api/pages/{page_id}/parent[?recursive={boolean}]` | [親ページの取得](#get-page-parent)
  |GET    | `/api/pages/{page_id}/path`                       | [ページパスの取得](#get-page-path)
  |POST   | `/api/pages/{page_id}/path?rename_to={page_path}[&recursive={boolean}]` | [ページパスの変更(リネーム)](#rename-page-path)
  |POST   | `/api/pages/{page_id}/path?restore_to={page_path}[&recursive={boolean}]` | [ページの復帰](#restore-page-path)
  |GET    | `/api/pages/{page_id}/assets`                     | [ページに付随するアセットのメタ情報一覧取得](#get-page-assets)
  |POST   | `/api/pages/{page_id}/assets/{file_name}`         | [アセットのアップロード](#upload-page-asset)
  |GET    | `/api/pages/{page_id}/assets/{file_name}`         | [アセットIDによるアセット取得へのリダイレクト](#get-page-asset)
  |POST   | `/api/pages/{page_id}/lock`                       | [ページのロック](#lock-page)
  |PUT    | `/api/pages/{page_id}/lock`                       | [ページのロック延長](#update-page-lock)
  |GET    | `/api/pages/{page_id}/lock`                       | [ページのロック状態の取得](#get-page-lock-info)
  |DELETE | `/api/pages/{page_id}/lock`                       | [ページのロック解除](#unlock-page)
  |POST   | `/api/pages/{page_id}/revision?rollback_to={rev}` | [ページソースのロールバック](#rollback-page)
  |POST   | `/api/pages/{page_id}/revision?keep_from={rev}`   | [ページソースのコンパクション](#compaction-page)
  |DELETE | `/api/pages/{page_id}`                            | [ページの削除](#delete-page)
  |POST   | `/api/assets?path={page_path}&file={file_name}`   | [アセットのアップロード](#upload-asset)
  |GET    | `/api/assets?path={page_path}&file={file_name}`   | [アセットIDによるアセット取得へのリダイレクト](#redirect-to-get-asset)
  |GET    | `/api/assets/{asset_id}/data`                     | [アセットの本体データの取得](#get-asset)
  |GET    | `/api/assets/{asset_id}/meta`                     | [アセットのメタ情報の取得](#get-asset-metadata)
  |DELETE | `/api/assets/{asset_id}`                          | [アセットの削除](#delete-asset)

--- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

## `/api/pages`

<a id="create-page"></a>
### `POST /api/pages?path={page_path}`
#### 概要
ドラフトページの作成

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `path` | string | 初期ページパス | 必須 |

#### リクエスト
リクエストボディは受け付けない。


#### レスポンス
リクエストに成功した場合、ステータスは201を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json
  | `Location` | /api/pages/{page_id}/meta
  | `ETag` | {page_id}
  | `X-Page-Lock` | "expire={expire_datetime} token={lock_token}"

また、ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
required:
  - id
properties:
  id:
    description: >-
      割り当てられたページIDが格納される
    type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `path`で指定されたパスのフォーマットが不正<br>リクエストボディが空ではない
  | 409 Conflict | `path`で指定されたページがすでに存在する

#### 注記
  - 本APIはドラフトページの作成のみを行う
  - レスポンスの`X-Page-Lock`でロック情報が返される
  - ドラフト作成とロック取得は同一トランザクションで行う
  - ドラフトページにはソースが存在しないため、`GET /api/pages/{page_id}/source`は404となる

<a id="get-pages"></a>
### `GET /api/pages?prefix={page_path}[&forward={page_path}][&rewind={page_path}][&limit={number}][&with_deleted={boolean}]`
#### 概要
ページ一覧の取得

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `prefix` | string | 対象ページパス | 必須 |
  | `forward` | string | カーソルの指定(昇順指定) | 任意 |
  | `rewind` | string | カーソルの指定(降順指定) | 任意 |
  | `limit` | number | 取得する最大件数 | 任意 |
  | `with_deleted` | boolean | 削除済みページを含めるか否か | 任意 |

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json

ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
required:
    - "items"
    - "has_more"
properties:
  items:
    description: >-
      ページ情報のリストが格納される。
    type: "array"
    items:
      description: >-
        一件分のページ情報が格納される
      type: "object"
      required:
        - "page_id"
        - "path"
        - "deleted"
        - "last_update"
      properties:
        page_id:
          description: >-
            ページIDが格納される
          type: "string"

        path:
          description: >-
            ページの絶対パスが格納される。
          type: "string"

        deleted:
          description: >-
            ページが削除されているか否かを表すフラグ値が格納される (削除されてい
            る場合true)。
          type: "boolean" 

        last_update:
          description: >-
            最終更新情報をパックしたオブジェクトが格納される
          type: "object"
          required:
            - "revision"
            - "timestamp"
            - "username"
          properties:
            revision:
              description: >-
                リビジョン番号が格納される
              type: "integer"

            timestamp:
              description: >-
                更新日時が格納される(ISO8601,タイムゾーン無し)
              type: "string"

            username:
              description: >-
                更新を行ったユーザの名前が格納される
              type: "string"
 
  has_more:
    description: >-
      同じ方向で継続して一覧取得可能か否かを表すフラグ値が格納される (trueの場合、
      継続して一覧取得が可能)。
    type: "boolean"

  anchor:
    description: >-
      次回の forward / rewind に指定すべきカーソル値 (has_moreがtrueの場合のみ格
      納される)。
    type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `prefix`,`forward`,`rewind`で指定されたパスのフォーマットが不正<br>`forward`と`rewind`が同時に指定された

#### 注記
  - `prefix`で指定されたパス配下のページ一覧を取得する
  - `forward`もしくは`rewind`でカーソルを指定し、ページパス順に`limit`で指定した件数を上限としてエントリを取得する。
      - カーソルを`forward`で指定した場合は、起点から昇順に`prefix`の配下にあるパスエントリを取得する
      - カーソルを`rewind`で指定した場合は、起点から降順に`prefix`の配下にあるパスエントリを取得する
      - なお、昇順・降順にかかわらずカーソルとして指定されたパスは取得範囲に含まれない
      - `forward`と`rewind`の両方共が指定されない場合は、`forward`に`prefix`と同じパスを指定した物として動作する
      - `forward`と`rewind`の両方共が指定された場合はエラーとする
  - `limit`が指定されなかった場合は、`limit`に50を指定した物として動作する
  - `with_deleted`が指定されなかった場合は、`with_deleted`に`false`を指定した物として動作する
  - 対象が存在しない場合は空配列を返す

<a id="get-deleted-pages"></a>
### `GET /api/pages/deleted?path={page_path}`
#### 概要
削除済みページ候補の取得

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `path` | string | 対象ページパス | 必須 |

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json

ボディには以下の内容のJSONデータが返される。

```yaml
type: "array"
items:
  type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `path`で指定されたパスのフォーマットが不正

#### 注記
  - 対象が存在しない場合は空配列を返す
  - 削除済みページおよびドラフトページは返却対象外とする
  - レスポンスの配列は`name`の昇順で返す

<a id="get-template-pages"></a>
### `GET /api/pages/template`

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json

ボディには以下の内容のJSONデータが返される。

```yaml
type: "array"
items:
  type: "object"
  required:
    - "page_id"
    - "name"
  properties:
    page_id:
      description: >-
        テンプレートページのページIDが格納される
      type: "string"

    name:
      description: >-
        テンプレート名(パスの最終エレメント)が格納される
      type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | テンプレート機能が無効化さている場合に返される

#### 注記
  - 対象が存在しない場合は空配列を返す

<a id="search-pages"></a>
### `GET /api/pages/search?expr={expression}[&target={targets}][&with_deleted={boolean}][&all_revision={boolean}]`
#### 概要
ページの検索

#### クエリーパラメータ
  |名称|型|説明|必須
  |:--|:--|:--|:--
  | `expr` | string | 検索式 | 必須
  | `target` | string | 検索対象の指定 | 任意
  | `with_deleted` | boolean | 削除済みページを対象に含めるか否かを指定するフラグ | 任意
  | `all_revision` | boolean | 全てのリビジョンを対象とするか否かを指定するフラグ | 任意

##### targetへの検索対象の指定
クエリーパラメータ`target`を指定する場合は、以下の文字列を","で連結したリストを指定する。また、最低限でも一つを指定する必要がある。

例: `target=headings`, `target=body,code`

| 値 | 検索対象
|:---|:---
|`headings` | 見出し
|`body` | 本文
|`code` | コードブロック

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json

ボディには以下の内容のJSONデータが返される。

```yaml
type: "array"
items:
  description: >-
    検索にマッチしたページの情報が格納される。
  type: "object"
  required:
    - "page_id"
    - "revision"
    - "score"
    - "path"
    - "deleted"
    - "text"
  properties:
    page_id:
      description: >-
        ページIDが格納される
      type: "string"

    revision:
      description: >-
        リビジョン番号が格納される。
      type: "integer"

    score:
      description: >-
        検索スコアが格納される。
      type: "float"

    path:
      description: >-
        ページのパスが格納される。
      type: "string"

    deleted:
      description: >-
        ページが削除されているか否かを表すフラグ値が格納される(削除されている場合true)。
      type: "boolean"
    text:
      description: >-
        検索式にマッチした周辺を含んだスニペットテキストが格納される。
      type: "string"
maxItems: 100
```
リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `target`で指定された対象リストが不正<br>exprで指定した検索式が不正

#### 注記
  - 検索式にマッチするページが存在しない場合は空配列を返す
  - クエリーパラメータ`target`が省略された場合は本文のみを検索対象とする(`target=body`を指定したのと同等)
  - クエリーパラメータ`with_deleted`が省略された場合は`with_deleted=false`を指定した物として扱う
  - クエリーパラメータ`all_revision`が省略された場合は`all_revision=false`を指定した物として扱う

<a id="get-page-source"></a>
### `GET /api/pages/{page_id}/source[?rev={revision}]`
#### 概要
ページソースの取得

#### パスエレメント
  - page_id : 操作対象のページID

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `rev` | number | 取得対象のリビジョン番号 | 任意 |

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | text/markdown
  | `Cache-Control` | "public, max-age=31536000, immutable" (固定)
  | `ETag` | "{page_id}:{revision}"

ボディには対象ページのMarkdownソースが返される。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `rev`で指定されたリビジョン番号のフォーマットが不正
  | 404 Not Found | 指定されたページIDに対応するページが存在しない<br>`rev`で指定されたリビジョンのソースが存在しない<br>ドラフトページに対するリクエスト

#### 注記
  - クエリーパラメータ`rev`を省略した場合は最新リビジョンのソースを返す。
  - ドラフトページに対するリクエストの場合、`reason`にドラフトであることを示す文言を設定する
  - 削除済みページに対するリクエストでもソースを返す

<a id="update-page-source"></a>
### `PUT /api/pages/{page_id}/source[?amend={boolean}]`
#### 概要
ページソースの更新

#### パスエレメント
  - page_id : 操作対象のページID

#### リクエストヘッダ
ロックされているページの更新を行う場合は以下のヘッダを設定する必要がある。

  | ヘッダ名 | 内容
  |:--|:--
  | `X-Lock-Authentication` | "token={lock_token}"

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `amend` | boolean | 修正か更新かを指定するフラグ | 任意 |

#### レスポンス
リクエストに成功した場合、ステータスは204を返す(HTTPヘッダに特別に設定するものはない)。
また、ボディにも何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `amend`に`true`または`false`以外が指定された
  | 403 Forbidden | 記述者以外が`amend=true`を指定した<br>ロックしたユーザと異なるユーザが更新しようとした<br>リクエストヘッダの`X-Lock-Authentication`による認証に失敗した
  | 404 Not Found | 指定されたページIDに対応するページが存在しない
  | 410 Gone | 削除済みのページを指定した
  | 423 Locked | ロックされているページにリクエストヘッダ`X-Lock-Authentication`なしでリクエストした

#### 注記
  - ロックされているページへの更新を行う場合、リクエストヘッダ`X-Lock-Authentication`を設定する必要がある
  - リクエストヘッダの`X-Lock-Authentication`の`token`には、`POST /api/pages/{page_id}/lock`及び`PUT /api/pages/{page_id}/lock`で受信した解除用のトークンを渡す必要がある
  - ロックされているページの更新に成功した場合ページにかかっていたロックは解除される(失敗した場合(ステータスが204以外の場合)は解除されない)
  - ドラフトページの更新に成功した場合は通常ページとして登録され、リビジョン番号は1となる
  - クエリーパラメータ`amend`を省略した場合は`false`が指定されたものとして処理を行う
  - `amend=true`を指定した場合はリビジョンを更新せず、最新リビジョンのソースを上書きする（誤字程度の修正用）
  - `amend=true`は最新リビジョンを記述したユーザのみが指定可能

<a id="get-page-metadata"></a>
### `GET /api/pages/{page_id}/meta[?rev={revision}]`
#### 概要
ページのメタ情報の取得

#### パスエレメント
  - page_id : 操作対象のページID

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `rev` | number | 取得対象のリビジョン番号 | 任意 |

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json
  | `Cache-Control` | "public, max-age=31536000, immutable" (固定)
  | `ETag` | "{page_id}:{revision}"

ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
properties:
  page_info:
    description: >-
      ページ全体の情報が格納される
    properties:
      path:
        description: >-
          このページのパス状態が格納される
        type: "object"
        required:
          - kind
          - value
        properties:
          kind:
            description: >-
              パスの種別("current"または"last_deleted")
            type: "string"
          value:
            description: >-
              ページパスが格納される
            type: "string"

      revision_scope:
        description: >-
          このページのリビジョン範囲が格納される
        type: "object"
        required:
          - "latest"
          - "oldest"
        properties:
          latest:
            description: >-
              このページの最新のリビジョン番号が格納される
            type: "integer"

          oldest:
            description: >-
              このページの最古のリビジョン番号が格納される
            type: "integer"

      rename_revisions:
        description: >-
          ページのリネームが行われたリビジョン番号のリストが格納される。
        type: "array"
        items:
          type: "number"

      deleted:
        description: >-
          ページが削除されているか否かを表すフラグ
        type: "boolean"

      locked:
        description: >-
          ページがロックされているか否かを表すフラグ
        type: "boolean"

  revision_info:
    description: >-
      リビジョン固有の情報が格納される
    properties:
      revision:
        description: >-
          リビジョン番号が格納される
        type: "number"

      timestamp:
        description: >-
          ソースが記録された日時
        type: "string"

      username:
        description: >-
          ソースを記録したユーザ
        type: "string"

      rename_info:
        description: >-
          リネーム情報が格納される(リネームが行われたリビジョンにのみ格納される)
        type: "object"
        properties:
          from:
            description: >-
              変更前のページパス
            type: "string"

          to:
            description: >-
              変更後のページパス
            type: "string"

          link_refs:
            description: >-
              リネーム直前のリンク解決状態(ページ中に現れる正規化済みリンク先パ
              スをキーとしたページIDへのインデックスを検索するためのマップ)。
            type: "object"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `rev`で指定されたリビジョン番号のフォーマットが不正
  | 404 Not Found | 指定されたページIDに対応するページが存在しない<br>`rev`で指定されたリビジョンのソースが存在しない


<a id="get-page-path"></a>
### `GET /api/pages/{page_id}/path`
#### 概要
ページパスの取得

#### パスエレメント
  - `page_id` : 操作対象のページID

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json
  | `Cache-Control` | "public, max-age=31536000, immutable" (固定)
  | `ETag` | "{page_id}:{revision}"

また、ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
required:
  - path
properties:
  path:
    description: >-
      指定されたページIDに対応するページのパスが格納される
    type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `page_id`で指定されたページが存在しない
  | 410 Gone | 削除済みのページが指定された

<a id="get-page-parent"></a>
### `GET /api/pages/{page_id}/parent[?recursive={boolean}]`
#### 概要
親ページの取得

#### パスエレメント
  - `page_id` : 操作対象のページID

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `recursive` | boolean | 親を辿って最初に存在するページを返す | 任意 |

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json

また、ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
required:
  - id
  - path
properties:
  id:
    description: >-
      親ページのページIDが格納される
    type: "string"
  path:
    description: >-
      親ページのパスが格納される
    type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `page_id`で指定されたページが存在しない<br>親ページが存在しない
  | 410 Gone | 削除済みのページが指定された

<a id="rename-page-path"></a>
### `POST /api/pages/{page_id}/path?rename_to={page_path}[&recursive={boolean}]`
#### 概要
ページパスの変更(リネーム)

#### パスエレメント
  - page_id : 操作対象のページID

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `rename_to` | string | リネーム先のパス | 必須 |
  | `recursive` | boolean | 配下ページを含めて移動する | 任意 |

#### レスポンス
リクエストに成功した場合、ステータスは204を返す(HTTPヘッダに特別に設定するものはない)。
また、ボディにも何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `rename_to`で指定されたパス文字列のフォーマットが不正<br>`rename_to`と`restore_to`が同時に指定された
  | 404 Not Found | 指定されたページIDに対応するページが存在しない
  | 409 Conflict | `rename_to`で指定されたパスにすでにページが存在する
  | 410 Gone | 削除済みのページが指定された
  | 423 Locked | ロックされているページをリネームしようとした

#### 注記
  - `recursive=true`の場合、配下にロック中のページが存在すると失敗する

<a id="restore-page-path"></a>
### `POST /api/pages/{page_id}/path?restore_to={page_path}[&recursive={boolean}]`
#### 概要
ページの復帰

#### パスエレメント
  - page_id : 操作対象のページID

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `restore_to` | string | 復帰先のパス | 必須 |
  | `recursive` | boolean | 配下ページを含めて復帰する | 任意 |

#### レスポンス
リクエストに成功した場合、ステータスは204を返す(HTTPヘッダに特別に設定するものはない)。
また、ボディにも何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `restore_to`で指定されたパス文字列のフォーマットが不正<br>`rename_to`と`restore_to`が同時に指定された
  | 404 Not Found | 指定されたページIDに対応するページが存在しない
  | 409 Conflict | `restore_to`で指定されたパスにすでにページが存在する<br>削除済みページではない
  | 423 Locked | ロックされているページを復帰しようとした

#### 注記
  - 復帰操作ではリビジョン番号は増加しない
  - `recursive=true`の場合、配下にロック中のページが存在すると失敗する

<a id="get-page-assets"></a>
### `GET /api/pages/{page_id}/assets`
#### 概要
ページに付随するアセットのメタ情報一覧を返す

#### パスエレメント
  - `page_id` : 操作対象のページID

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json
  | `Cache-Control` | "public, max-age=31536000, immutable" (固定)
  | `ETag` | "{page_id}"

また、ボディには以下の内容のJSONデータが返される。

```yaml
type: "array"
items:
  type: "object"
  properties:
    id:
      description: >-
        アセットIDが格納される。
      type: "string"

    file_name:
      description: >-
        ファイル名が格納される。
      type: "string"

    mime_type:
      description: >-
        アセットデータのMIME種別が格納される
      type: "string"

    size:
      description: >-
        アセットデータのバイナリサイズが格納される
      type: "number"

    timestamp:
      description: >-
        アセットがアップロードされた日時
      type: "string"

    username:
      description: >-
        アセットをアップロードしたユーザの名前が格納される。
      type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `page_id`で指定されたページが存在しない
  | 410 Gone | `page_id`で削除済みのページを指定した<br>`file_name`で指定される削除済みのアセットを指定した


<a id="upload-page-asset"></a>
### `POST /api/pages/{page_id}/assets/{file_name}`
#### 概要
アセットのアップロード

#### パスエレメント
  - `page_id` : 操作対象のページID
  - `file_name` : アップロードするアセットのファイル名

#### リクエストヘッダ
以下のヘッダを設定する必要がある。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Length` | {asset_data_size}
  | `X-Lock-Authentication` | "token={lock_token}" (ページがロックされている場合)

#### レスポンス
リクエストに成功した場合、ステータスは201を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json
  | `Location` | /api/assets/{asset_id}/data
  | `ETag` | 割り当てられたページID

また、ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
required:
  - id
properties:
  id:
    description: >-
      割り当てられたアセットIDが格納される
    type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `page_id`で指定されたページIDのフォーマットが不正<br>`file_name`で指定されたファイル名のフォーマットが不正
  | 404 Not Found | `page_id`で指定されたページが存在しない
  | 409 Conflict | `file_name`で指定されたアセットがすでにページ内に存在する
  | 410 Gone | `page_id`で削除済みのページを指定した
  | 411 Length Required | リクエストヘッダに`Content-Length`が含まれていない
  | 413 Content Too Large | アッセとデータのサイズが大きすぎる
  | 423 Locked | ロックされているページにアップロードしようとした
  | 403 Forbidden | ロック認証に失敗した<br>ロック取得者と異なるユーザがアップロードしようとした

#### 注記
  - アセットデータのサイズ制限は10MiBまでとする。10MiBを超えるアセットデータを送信された場合は413を返す
  - ロックされているページへアップロードする場合、`X-Lock-Authentication`が必須となる
  - `X-Lock-Authentication`に指定するトークンは、`POST /api/pages/{page_id}/lock`および`PUT /api/pages/{page_id}/lock`で取得した解除用トークンを使用する

<a id="get-page-asset"></a>
### `GET /api/pages/{page_id}/assets/{file_name}`
#### 概要
アセットIDによるアセット取得へのリダイレクト

#### パスエレメント
  - `page_id` : 操作対象のページID
  - `file_name` : ダウンロードするアセットのファイル名

#### レスポンス
リクエストに成功した場合、ステータスは302を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Location` | /api/assets/{asset_id}/data

また、ボディにも何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `page_id`で指定されたページIDのフォーマットが不正<br>`file_name`で指定されたファイル名のフォーマットが不正
  | 404 Not Found | `page_id`で指定されたページが存在しない<br>`file_name`で指定されるアセットがページに存在しない
  | 410 Gone | `page_id`で削除済みのページを指定した<br>`file_name`で指定される削除済みのアセットを指定した

<a id="lock-page"></a>
### `POST /api/pages/{page_id}/lock`
#### 概要
ページのロック

#### パスエレメント
  - `page_id` : 操作対象のページID

#### レスポンス
リクエストに成功した場合、ステータスは204を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `X-Page-Lock` | "expire={expire_datetime} token={lock_token}"

ボディには何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `page_id`で指定されたページが存在しない
  | 409 Conflict | `page_id`で指定されたページはすでにロックされている
  | 410 Gone | `page_id`で削除済みのページを指定した

#### 注記
  - レスポンスヘッダの`X-Page-Lock`でロックに関する情報が返される。
  - `expire`はロックの有効期限を返す(通常5分後)。
  - `token`でロック解除用のトークンを返す。
  - ロックを延長する場合は`PUT /api/pages/{page_id}/lock`によって延長を行う
  - 同一ユーザ・同一セッションであっても、同一ページに複数のロックを保持することはできない。既にロックが存在する場合、再度のロック要求は 409 Conflict を返す。

<a id="update-page-lock"></a>
### `PUT /api/pages/{page_id}/lock`
#### 概要
ページのロック延長

#### パスエレメント
  - `page_id` : 操作対象のページID

#### リクエストヘッダ
以下のヘッダを設定する必要がある。

  | ヘッダ名 | 内容
  |:--|:--
  | `X-Lock-Authentication` | "token={lock_token}"

#### レスポンス
リクエストに成功した場合、ステータスは204を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `X-Page-Lock` | "expire={expire_datetime} token={lock_token}"

ボディには何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `page_id`で指定されたページが存在しない<br>`page_id`で指定されたページはロックされていない(ロックの期限切れを含む)
  | 403 Forbidden | ロックしたユーザと異なるユーザが延長しようとした<br>リクエストヘッダの`X-Lock-Authentication`による認証に失敗した
  | 410 Gone | `page_id`で削除済みのページを指定した

#### 注記
  - リクエストヘッダの`X-Lock-Authentication`の`token`には、`POST /api/pages/{page_id}/lock`及び`PUT /api/pages/{page_id}/lock`で受信した解除用のトークンを渡す必要がある。
  - ドラフトページに対してロック解除を行う場合は、ドラフトページを削除する
  - ドラフト削除時は付随アセットも同時に削除する
  - レスポンスヘッダの`X-Page-Lock`で更新されたロック情報が返される。
  - `expire`はロックの有効期限を返す(通常5分後)。
  - `token`でロック解除用のトークンを返す。
  - ロックの再延長は`PUT /api/pages/{page_id}/lock`によって延長を行う
  - ロック延長 API を呼び出すと、有効期限の延長に加えて 新しい解除トークンが発行され解除トークンが切り替わる。これ以降のロック解除には新しく発行された解除トークンのみを受け付ける。

<a id="get-page-lock-info"></a>
### `GET /api/pages/{page_id}/lock`
#### 概要
ロック情報の取得

#### パスエレメント
  - `page_id` : 操作対象のページID

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json

また、ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
required:
  - expire
  - username
properties:
  expire:
    description: >-
      ロックの有効期限が格納される
    type: "string"

  username:
    description: >-
      ロックを行ったユーザの名前
    type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `page_id`で指定されたページが存在しない<br>`page_id`で指定されたページはロックされていない(ロックの期限切れを含む)
  | 410 Gone | `page_id`で削除済みのページを指定した

<a id="unlock-page"></a>
### `DELETE /api/pages/{page_id}/lock`
#### 概要
ページのロック解除

#### パスエレメント
  - `page_id` : 操作対象のページID

#### リクエストヘッダ
以下のヘッダを設定する必要がある。

  | ヘッダ名 | 内容
  |:--|:--
  | `X-Lock-Authentication` | "token={lock_token}"

#### レスポンス
リクエストに成功した場合、ステータスは204を返す(HTTPヘッダに特別に設定するものはない)。
ボディには何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `page_id`で指定されたページが存在しない<br>`page_id`で指定されたページはロックされていない(ロックの期限切れを含む)
  | 403 Forbidden | ロックしたユーザと異なるユーザが解除しようとした<br>リクエストヘッダの`X-Lock-Authentication`による認証に失敗した
  | 410 Gone | `page_id`で削除済みのページを指定した

#### 注記
  - リクエストヘッダの`X-Lock-Authentication`の`token`には、`POST /api/pages/{page_id}/lock`及び`PUT /api/pages/{page_id}/lock`で受信した解除用のトークンを渡す必要がある。

<a id="rollback-page"></a>
### `POST /api/pages/{page_id}/revision?rollback_to={rev}`
#### 概要
ページソースのロールバック

#### パスエレメント
  - page_id : 操作対象のページID

#### クエリーパラメータ
  |名称|型|説明|必須
  |:--|:--|:--|:--
  | `rollback_to` | number | ロールバック先のリビジョン番号 | 必須 

#### レスポンス
リクエストに成功した場合、ステータスは204を返す(HTTPヘッダに特別に設定するものはない)。
また、ボディにも何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `rollback_to`で指定されたリビジョン番号のフォーマットが不正<br>`rollback_to`で指定されたリビジョンが存在しない<br>`rollback_to`と`keep_from`が同時に指定された
  | 404 Not Found | 指定されたページIDに対応するページが存在しない
  | 410 Gone | 削除済みのページが指定された
  | 423 Locked | ロックされているページを操作しようとした

#### 注記
  - ロック中のページに対するリクエストは禁止されている
  - 本リクエストによる操作は取り消すことはできない

<a id="compaction-page"></a>
### `POST /api/pages/{page_id}/revision?keep_from={rev}`
#### 概要
ページソースのコンパクション(指定リビジョンより過去のリビジョンソースの破棄)

#### パスエレメント
  - page_id : 操作対象のページID

#### クエリーパラメータ
  |名称|型|説明|必須
  |:--|:--|:--|:--
  | `keep_from` | number | 保持する下限リビジョン番号 | 必須 

#### レスポンス
リクエストに成功した場合、ステータスは204を返す(HTTPヘッダに特別に設定するものはない)。
また、ボディにも何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | `keep_from`で指定されたリビジョン番号のフォーマットが不正<br>`keep_from`で指定されたリビジョンが存在しない<br>`rollback_to`と`keep_from`が同時に指定された
  | 404 Not Found | 指定されたページIDに対応するページが存在しない
  | 410 Gone | 削除済みのページが指定された
  | 423 Locked | ロックされているページを操作しようとした

#### 注記
  - ロック中のページに対するリクエストは禁止されている
  - 本リクエストによる操作は取り消すことはできない

<a id="delete-page"></a>
### `DELETE /api/pages/{page_id}`
#### 概要
ページの削除

#### パスエレメント
  - `page_id` : 操作対象のページID

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `recursive` | boolean | 配下ページを含めて削除する | 任意 |

#### リクエストヘッダ
ロックされているページの削除を行う場合は以下のヘッダを設定する必要がある。

  | ヘッダ名 | 内容
  |:--|:--
  | `X-Lock-Authentication` | "token={lock_token}"

#### レスポンス
リクエストに成功した場合、ステータスは204を返す(HTTPヘッダに特別に設定するものはない)。
また、ボディにも何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `page_id`で指定されたページが存在しない
  | 403 Forbidden | ロックしたユーザと異なるユーザが削除しようとした<br>リクエストヘッダの`X-Lock-Authentication`による認証に失敗した
  | 410 Gone | `page_id`で削除済みのページを指定した
  | 423 Locked | ロックされているページにリクエストヘッダ`X-Lock-Authentication`なしでリクエストした<br>配下ページにロック中のページが存在する

#### 注記
  - ロックされているページへの削除を行う場合、リクエストヘッダ`X-Lock-Authentication`を設定する必要がある。
  - リクエストヘッダの`X-Lock-Authentication`の`token`には、`POST /api/pages/{page_id}/lock`及び`PUT /api/pages/{page_id}/lock`で受信した解除用のトークンを渡す必要がある。
  - 削除したページに紐付けられているアセットも同時削除される
  - ドラフトページに対する削除はハードデリートとし、付随アセットもハードデリートする
  - `recursive=true`が指定された場合は配下ページもまとめて削除する
  - `recursive=true`の場合、配下にロック中のページが存在すると、ロック解除トークンを保持していても削除しない

--- --- --- --- --- --- --- --- --- --- --- --- --- --- ---

## `/api/assets`

<a id="upload-asset"></a>
### `POST /api/assets?path={page_path}&file={file_name}`
#### 概要
アセットのアップロード

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `path` | string | アセットを付随させるページのパス | 必須 |
  | `file` | string | アップロードするアセットのファイル名 | 必須 |

#### リクエストヘッダ
以下のヘッダを設定する必要がある。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Length` | {asset_data_size}
  | `X-Lock-Authentication` | "token={lock_token}" (ページがロックされている場合)

#### レスポンス
リクエストに成功した場合、ステータスは201を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json
  | `Location` | /api/assets/{asset_id}/data
  | `ETag` | {asset_id}

また、ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
required:
  - id
properties:
  id:
    description: >-
      割り当てられたアセットIDが格納される
    type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | クエリーパラメータ`file`で指定されたファイル名のフォーマットが不正
  | 404 Not Found | クエリーパラメータ`path`で指定されたページが存在しない
  | 409 Conflict | クエリーパラメータ`file`で指定されたアセットがすでにページ内に存在する
  | 410 Gone | クエリーパラメータ`path`で削除済みのページを指定した
  | 411 Length Required | リクエストヘッダに`Content-Length`が含まれていない
  | 413 Content Too Large | アッセとデータのサイズが大きすぎる
  | 423 Locked | ロックされているページにアップロードしようとした
  | 403 Forbidden | ロック認証に失敗した<br>ロック取得者と異なるユーザがアップロードしようとした

#### 注記
  - アセットデータのサイズ制限は10MiBまでとする。10MiBを超えるアセットデータを送信された場合は413を返す
  - ロックされているページへアップロードする場合、`X-Lock-Authentication`が必須となる
  - `X-Lock-Authentication`に指定するトークンは、`POST /api/pages/{page_id}/lock`および`PUT /api/pages/{page_id}/lock`で取得した解除用トークンを使用する

<a id="redirect-to-get-asset"></a>
### `GET /api/assets?path={page_path}&file={file_name}`
#### 概要
アセットIDによるアセット取得へのリダイレクト

#### クエリーパラメータ
  |名称|型|説明|必須|
  |:--|:--|:--|:--|
  | `path` | string | アセットが付随しているページのパス | 必須 |
  | `file` | string | ダウンロードするアセットのファイル名 | 必須 |

#### レスポンス
リクエストに成功した場合、ステータスは302を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json
  | `Location` | /api/assets/{asset_id}/data
  | `ETag` | {asset_id}

また、ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
required:
  - id
properties:
  id:
    description: >-
      割り当てられたアセットIDが格納される
    type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 400 Bad Request | クエリーパラメータ`path`で指定されたページIDのフォーマットが不正<br>クエリーパラメータ`file`で指定されたファイル名のフォーマットが不正
  | 404 Not Found | クエリーパラメータ`path`で指定されたページが存在しない<br>クエリーパラメータ`file`で指定されたファイル名のアセットが存在しない
  | 410 Gone | クエリーパラメータ`path`で削除済みのページを指定した<br>クエリーパラメータ`file`で削除済のアセットを指定した

<a id="get-asset"></a>
### `GET /api/assets/{asset_id}/data`
#### 概要
アセットの本体データの取得

#### パスエレメント
  - `asset_id` : 操作対象のアセットのID

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | (アセットのMIME種別)
  | `Cache-Control` | "public, max-age=31536000, immutable" (固定)
  | `ETag` | {asset_id}

また、ボディにはアセットのデータ(バイナリデータ)が返される。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `asset_id`で指定されたアセットが存在しない
  | 410 Gone | `asset_id`で削除済みアセットを指定した

<a id="get-asset-metadata"></a>
### `GET /api/assets/{asset_id}/meta`
#### 概要
アセットのメタ情報の取得

#### パスエレメント
  - `asset_id` : 操作対象のアセットのID

#### レスポンス
リクエストに成功した場合、ステータスは200を返しHTTPヘッダは以下の内容が設定される。

  | ヘッダ名 | 内容
  |:--|:--
  | `Content-Type` | application/json
  | `Cache-Control` | "public, max-age=31536000, immutable" (固定)
  | `ETag` | {asset_id}

また、ボディには以下の内容のJSONデータが返される。

```yaml
type: "object"
properties:
  file_name:
    description: >-
      ファイル名が格納される。
    type: "string"

  mime_type:
    description: >-
      アセットデータのMIME種別が格納される
    type: "string"

  size:
    description: >-
      アセットデータのバイナリサイズが格納される
    type: "number"

  timestamp:
    description: >-
      アセットがアップロードされた日時
    type: "string"

  username:
    description: >-
      アセットをアップロードしたユーザの名前が格納される。
    type: "string"
```

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `asset_id`で指定されたアセットが存在しない
  | 410 Gone | `asset_id`で削除済みアセットを指定した

<a id="delete-asset"></a>
### `DELETE /api/assets/{asset_id}`
#### 概要
アセットの削除

#### パスエレメント
  - `asset_id` : 操作対象のアセットのID

#### レスポンス
リクエストに成功した場合、ステータスは204を返す(HTTPヘッダに特別に設定するものはない)。
また、ボディにも何も返さない。

リクエストに失敗したときは以下のステータスが返される。

  | ステータス | 説明
  |:--|:--
  | 404 Not Found | `asset_id`で指定されたアセットが存在しない
  | 410 Gone | `asset_id`で削除済みのアセットを指定した
  | 423 Locked | ロックされているページのアセットを削除しようとした
