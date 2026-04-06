# エクスポートデータ仕様

## 概要
- エクスポートデータはZIPファイルとして出力を行う
- エクスポートデータは暗号化を施すことも可能
- 暗号化時はAES-256を優先し、利用できない場合はStandard ZIP 2.0へフォールバックしてよい
- Standard ZIP 2.0へフォールバックした場合は警告を表示する
- パスワード仕様は採用したZIP暗号方式に従う
- 復号失敗、不正パスワード、未対応形式の受信はすべてエラーとして扱う
- 暗号方式情報はZIPレイヤーの責務とし、エクスポートデータ本体には保持しない
- `migrate` 用エクスポートはサーバ停止状態で実行し、エクスポート処理と移送元ページツリー削除を単一トランザクションとして扱う
- `migrate` 用エクスポートが成功した場合は、移送元ページツリーを無条件に削除し、対象ページに紐付くロック情報も同時に削除する
- `migrate` 用エクスポートまたはインポートで失敗が発生した場合はロールバックする
- ドラフトページはエクスポートデータに含めないが、`migrate` 成功時の移送元ページツリー削除対象には含める
- ロック中のページはエクスポート対象に含めるが、ロック情報自体はエクスポートデータに含めない
- 削除済みアセットはエクスポートデータに含めない
- アセットは編集不能オブジェクトとして扱う
- 孤立アセットは、ページパスを軸とした対象選定の結果、エクスポート対象に含めない
- エクスポートデータに含まれるデータは以下の通り
    - マニフェスト(`manifest.json`)
    - ユーザ情報リスト(`users.jsonl`)
    - ページ情報リスト(`pages.jsonl`)
    - リビジョン情報リスト(`revisions.jsonl`)
    - アセット情報リスト(`assets.jsonl`)
    - アセットファイル(`assets/...`)
- `backup` では rename リビジョンと rename 情報を完全に保持する
- `migrate` では rename リビジョン自体は保持するが、有効な rename 情報は保持しない
- `migrate` の `revisions.jsonl.rename` は、必要に応じて `"removed_by_migrate"` を格納する
- `migrate` の `pages.jsonl.rename_revisions` は出力しない

## 検証ルール

- `backup` の `dry-run` では追加の検証は行わない
- `migrate` の `dry-run` では、少なくともツリー外へのページリンクおよびアセットリンクの有無、絶対パスによるページリンクの有無、インポート完了後の `username` 重複の有無、無効リンクへの置換対象の有無を検証する
- `migrate` インポート時は、移送先に子ページを持つ既存パスが存在する場合はエラーとする
- インポート時は `manifest` の件数と各JSONLの実件数の一致を検証する
- アセットについては、`assets.jsonl` の件数と実体ファイル数の一致も検証する
- インポート時は、エクスポートデータ中のIDおよびインポート先の同種IDとの重複を検出した場合はエラーとする
- インポート時はエクスポートデータ内の参照整合性を検証し、少なくともJSONL間の参照切れ、アセット実体ファイルの欠落、アセットサイズ不一致を検出対象に含める
- `migrate` の `strict-mode` では、少なくともツリー外のページへのリンクおよび絶対パスによるページリンクをエラー対象に含める
- 非`strict-mode`では、上記リンクは未解決リンクとして扱う
- `migrate` インポート時に有効な rename 情報が含まれていた場合、通常モードでは警告を出し、rename 情報を `"removed_by_migrate"` 相当へ正規化して継続する
- `migrate` インポート時に `pages.jsonl.rename_revisions` が含まれていた場合、通常モードでは警告を出し空配列として扱う
- `migrate` の `strict-mode` でも、有効な rename 情報および `pages.jsonl.rename_revisions` の混入はwarningを出した上で正規化して処理を継続する
- `--fix-broken-link` が指定されている場合、未解決リンクは `about:invalid` へ置換する
- `--fix-broken-link` が指定されていない場合、未解決リンクはそのまま保持する
- 削除済みページへのリンクは通常の未存在リンクと同様に扱う
- `page_id` 直接指定リンクはマイグレート仕様上の考慮対象外とする
- `migrate` ではrename情報を用いた再解決は行わない

## データ構造

### マニフェスト
マニフェストはJSON形式で以下のスキーマのデータが格納される(OpenAPI SchemaをYAML形式で記述)。
ここでのパス情報は、エクスポート対象ツリーの基準位置を示すために絶対パスで扱う。
`export_root`は常に絶対パスであり、`pages.jsonl`に含まれる各ページの`path`はこの`export_root`からの相対パスとして扱う。
パス型フィールドには、DBに保持された正規化済みパスを用いる。
ルートページは特例として `"/"` で表す。

```yaml
type: "object"
required:
  - "version"
  - "export_type"
  - "export_root"
  - "timestamp"
  - "page_count"
  - "revision_count"
  - "asset_count"

properties:
  version:
    description: >-
      エクスポートデータのフォーマットバージョン番号が格納される
    type: "integer"
    const: 1

  export_type:
    description: >-
      エクスポートデータの種別が格納される。"backup"の場合はバックアップ用デー
      タ、"migrate"の場合はマイグレート用データであることを表す。
    type: "string"
    enum:
      - "backup"
      - "migrate"

  export_root:
    description: >-
      エクスポート対象ツリーの先頭パス(絶対パス)が格納される。
      backupデータでは常に"/"が格納される。
      migrateデータではエクスポート時に指定されたサブツリー先頭パスが格納される。
    type: "string"

  timestamp:
    description: >-
      エクスポートが実行された日時がISO 8601形式で格納される。
    type: "string"
    format: "date-time"

  page_count:
    description: >-
      エクスポートデータに含まれるページ数が格納される。
    type: "integer"
    minimum: 1

  revision_count:
    description: >-
      エクスポートデータに含まれるリビジョン情報の数が格納される。
    type: "integer"
    minimum: 0

  asset_count:
    description: >-
      エクスポートデータに含まれるアセットの数が格納される。
    type: "integer"
    minimum: 0
```

### ユーザ情報リスト
ユーザ情報リストにはJSONL形式のデータで格納される。ユーザ情報は構造体`UserInfo`に展開される。
`users.jsonl` に含まれるユーザ情報は、`backup` の復元時だけでなく `migrate` のインポート時にも追加対象として扱う。
認証情報は復元対象に含める。
`--user-map` 未指定ユーザも追加対象に含める。
インポート完了後に `username` が重複する状態になる場合は、`strict-mode` の有無に関わらずエラーとして中断する。
個々のユーザ情報のスキーマ定義は以下の様に定義される(OpenAPI SchemaをYAML形式で記述)。

```yaml
type: "object"
required:
  - "id"
  - "username"
  - "password"
  - "salt"
  - "display_name"
properties:
  id:
    description: >-
      ユーザのID(ULID)が格納される
    type: "string"

  username:
    description: >-
      ユーザ名が格納される
    type: "string"

  password:
    description: >-
      ハッシュ化済みパスワードが格納される
    type: "string"

  salt:
    description: >-
      ハッシュ時に与えるソルトデータが格納される
    type: "array"
    items:
      type: "integer"
      minimum: 0
      maximum: 255
    minItems: 16
    maxItems: 16

  display_name:
    description: >-
      ユーザの表示名が格納される
    type: "string"

  attributes:
    description: >-
      ユーザ属性の配列が格納される。属性が無い場合は空配列とする。
      後方互換のため省略を許容し、省略時も空配列として扱う。
      初期実装では "NoBasicAuth" および "ReadOnly" を保持対象に含める。
    type: "array"
    items:
      type: "string"
```

### ページ情報リスト
ページ情報リストにはJSONL形式のデータで格納される。ページ情報は構造体`PageInfo`に展開される。
`path`は`manifest.json`の`export_root`を基準とした相対パスで格納される。
すなわち、絶対パスは`manifest.json`側（`export_root`）で管理し、ページごとのパスは相対表現のみを用いる。
`path` には `..` を含まない最短の正規化済み表現を用いる。
`export_type`ごとの出力ルールは以下の通り。

- `backup` : `rename_revisions`を含め、ページ情報を完全に保持する。
- `migrate` : `rename_revisions`は出力しない。renameリビジョンの存在は `revisions.jsonl.rename` 側で表現する。

個々のページ情報のスキーマ定義は以下の様に定義される(OpenAPI SchemaをYAML形式で記述)。

```yaml
type: "object"
required:
  - "id"
  - "path"
  - "latest"
  - "earliest"
properties:
  id:
    description: >-
      ページID(ULID)が格納される。
    type: "string"

  path:
    description: >-
      ページに割り当てられているパスが格納される。パスはいずれの場合も相対パスで格納される。
    type: "string"

  latest:
    description: >-
      ページのリビジョン番号(最新)が格納される。
    type: "integer"
    minimum: 1

  earliest:
    description: >-
      ページのリビジョン番号(最古)が格納される。
    type: "integer"
    minimum: 1

  rename_revisions:
    description: >-
      リネームが行われたリビジョン番号のリストが格納される。
    type: "array"
    items:
      type: "integer"
      minimum: 1
```

### リビジョン情報リスト
リビジョン情報リストにはJSONL形式のデータで格納される。リビジョン情報は構造体`PageSource`に展開される。
`export_type`ごとの出力ルールは以下の通り。

- `backup` : リビジョン情報を完全に保持し、有効なrename情報をobject形式で出力する。
- `migrate` : renameリビジョン自体は保持するが、有効なrename情報は保持しない。renameが失効したことを表す場合、`rename`フィールドには文字列 `"removed_by_migrate"` を格納する。
- `source` に格納するMarkdownソース中のリンク文字列は、ユーザ記述をそのまま保持し、正規化しない。

個々のリビジョン情報のスキーマ定義は以下の様に定義される(OpenAPI SchemaをYAML形式で記述)。

```yaml
type: "object"
required:
  - "page"
  - "revision"
  - "timestamp"
  - "user"
  - "source"
properties:
  page:
    description: >-
      対応するページのID(ULID)が格納される。
    type: "string"

  revision:
    description: >-
      リビジョン番号が格納される。
    type: "integer"
    minimum: 1

  timestamp:
    description: >-
      更新日時がISO 8601形式の文字列で格納される。
    type: "string"
    format: "date-time"

  user:
    description: >-
      このリビジョンの編集者のユーザID(ULID)が格納される。
    type: "string"

  rename:
    description: >-
      リネーム情報が格納される。
      null は rename 情報なしを表す。
      object は有効な rename 情報を表す。
      string の "removed_by_migrate" は、rename リビジョン自体は保持されるが、
      rename 情報がマイグレートにより失効したことを表す。
    oneOf:
      - type: "null"
      - type: "string"
        const: "removed_by_migrate"
      - type: "object"
        required:
          - "to"
          - "link_refs"
        properties:
          from:
            description: >-
              リネーム前の正規化済み絶対パスが格納される(新規作成の場合は格納されない)。
            type: "string"

          to:
            description: >-
              リネーム後の正規化済み絶対パスが格納される。
            type: "string"

          link_refs:
            description: >-
              リネーム直前時点でのページ中リンク解決状態（1段分）が格納される。 キー
              を正規化済みのパス、値が解決されたページID (ULID、未作成などで解決でき
              なかった場合はnullが格納される)
            type: "object"
            additionalProperties:
              type: "string"
              nullable: true

  source:
    description: >-
      ページのソース(Markdownソース)が格納される
    type: "string"
```

### アセット情報リスト
アセット情報リストにはJSONL形式のデータで格納される。アセット情報は構造体`AssetInfo`に展開される。
個々のリビジョン情報のスキーマ定義は以下の様に定義される(OpenAPI SchemaをYAML形式で記述)。

```yaml
type: "object"
required:
  - "id"
  - "page"
  - "file_name"
  - "mime"
  - "size"
  - "user"
  - "timestamp"

properties:
  id:
    description: >-
      アセットID(ULID)が格納される。
    type: "string"

  page:
    description: >-
      アセットが紐付けられたページのID(ULID)が格納される。
    type: "string"

  file_name:
    description: >-
      アセットのファイル名が格納される。
    type: "string"

  mime:
    description: >-
      アセットのMIME種別が格納される。
    type: "string"

  size:
    description: >-
      アセットのサイズ(バイト単位)が格納される。
    type: "integer"
    minimum: 0

  user:
    description: >-
      アセットをアップロードしたユーザのID(ULID)が格納される。
    type: "string"

  timestamp:
    description: >-
      アセットをアップロードした日時がISO 8601形式の文字列で格納される。
    type: "string"
    format: "date-time"
```

### アセットファイル
アセットファイルは`assets`ディレクトリの直下にアセットIDをファイル名としたファイルが置かれる。
