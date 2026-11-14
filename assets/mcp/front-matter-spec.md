# LuWiki の front matter

ページ先頭の YAML front matter で、Wiki のテンプレート・タグ、MCP prompts・resources、ユーザー定義メタデータを設定できます。以下に保存時の制約と設定例を示します。

MCP prompts の呼出しと placeholder の詳細は、同じ接続先の組み込みリソース `luwiki://<authority>/builtin/mcp-prompt-spec` から取得できます。 `<authority>` には接続先の設定値を使用します。

## 2. 基本方針

### 2.1 正本の所在

- front matter の正本はページソース内に存在する
- front matter 自体を本文と分離した別正本としては保持しない
- バックエンドは front matter を解析し、必要な派生データを構築する
- 派生データが失われても、最新ページソースから再構成可能であることを前提とする

### 2.2 フロントエンドでの扱い

- 編集時は raw source をそのまま編集対象とする
- 閲覧時は front matter を本文として表示しない
- 編集画面プレビューでも front matter を本文として表示しない
- 通常のfront matter除去では、区切り行後の本文の空白を保持する
- テンプレート適用時のみ、front matter除去・既存マクロ処理後の本文へ先頭空行除去を行う
  - 半角スペース・タブだけの先頭行も全て除去し、空白だけ本文は空文字とする
  - 最初の非空行の字下げ、本文内部・末尾・コード内の空白と改行は保持する
- 将来的にフロントエンドが front matter の内容自体を参照する可能性は残す

### 2.3 保存時の扱い

- front matter の構文チェックは保存時に行う
- 構文不正な front matter を含むページソースは保存失敗とする
- 構文・スキーマと、同じ Wiki における MCP の名前・URI の一意性を保存時に検証する。
- 対象操作は create、put、amend、append、rollback、import とする。
- エラーに保存済みと示されている場合は、最新本文を取得してから操作を再試行する。

## 3. front matter の認識

### 3.1 認識位置

- front matter はページソースの先頭にのみ記述できる
- 先頭以外に記述された `---` 区切りブロックは front matter としては扱わない

### 3.2 区切り記法

- front matter は YAML front matter 互換の `---` 区切りで記述する

例:

```markdown
---
wiki:
  template:
    name: "議事録"
    macro_expand: true
  tags:
    - rust
    - wiki
mcp:
  primitive: prompt
  name: "ページ要約"
  description: "ページ内容を要約するための prompt"
---

# 見出し

本文
```

### 3.3 構文形式

- front matter の内部表現は YAML とする
- YAML として構文解析可能でなければ保存時エラーとする
- M1 の実装では YAML パーサとして `serde_yaml_ng` を採用する
- `serde_yaml_ng` は front matter の YAML 構文解析および
  `serde` による内部構造体へのデシリアライズに用いる
- YAML の意味検証はパーサ側へ寄せず、
  デシリアライズ後の `validate()` 相当処理で行う

---

## 4. トップレベル構造

### 4.1 基本構造

front matter のトップレベルは object とする。

- `wiki`
  - LuWiki 固有のメタ情報
- `mcp`
  - MCP 連携用のメタ情報
- `custom_meta`
  - ユーザ定義メタ情報

M1.1 時点では、トップレベルの予約名前空間は
`wiki` 、 `mcp` 、 `custom_meta` の3つとする。

- `wiki` および `mcp` は LuWiki が意味解釈する名前空間とする
- `custom_meta` は LuWiki が構造だけを検証し、
  配下の個別意味は解釈しない名前空間とする
- 上記以外の未知 top-level 項目は許容しない

### 4.2 名前空間分離

`wiki` と `mcp` と `custom_meta` を分離する理由は以下の通り。

- Wiki 固有機能と MCP 連携機能の責務を分離するため
- 組み込み機能とユーザ自由領域の責務を分離するため
- 汎用的なキー名の衝突を避けるため
- 保存時バリデーションを機能単位で整理しやすくするため
- 将来の MCP 拡張を既存 Wiki 機能と独立に進めやすくするため

---

## 5. `wiki` 名前空間

`wiki` は LuWiki 固有メタ情報の名前空間とする。

初期段階で想定する主な項目は以下。

- `template`
  - テンプレートページのマーキング
- `tags`
  - タグ情報

個別仕様は別途定義するが、少なくとも以下を前提とする。

- `wiki.template` は object とする
- `wiki.tags` はタグ一覧を表す配列を想定する

### 5.1 `wiki.template`

`wiki.template` は、テンプレートページであることを示すとともに、
テンプレート一覧表示およびテンプレート適用時に必要となる
メタ情報を保持する object とする。

初期段階では以下の項目を定義する。

- `name`
  - テンプレート名
  - ページ名とは独立した表示名
- `description`
  - テンプレート用途の説明文
  - 任意
- `macro_expand`
  - テンプレート適用時に、プレースホルダのうち
    即時展開マクロと同名のものを展開するか否か
  - 任意

例:

```yaml
wiki:
  template:
    name: "議事録"
    description: "定例会議の議事録テンプレート"
    macro_expand: true
```

注記:

- `name` は必須とする
- `macro_expand` は初期段階では boolean とする
- 将来的に展開対象マクロの絞り込みなどが必要になった場合は
  別途拡張を検討する
- M2 以降では、`wiki.template` の内容を
  テンプレート候補派生データへ射影する正本入力として利用する

### 5.2 `wiki.tags`

`wiki.tags` はページに紐付くタグ一覧を表す。
初期段階では文字列タグの列挙とし、string の配列で表現する。

例:

```yaml
wiki:
  tags:
    - rust
    - mcp
    - template
```

注記:

- 初期段階ではタグ自体に追加メタ情報は持たせない
- タグごとの説明、表示名、別名、色などは将来拡張項目とする
- タグ文字列は空白文字および制御文字を含めない
- 同一ページ内で同一タグが複数指定された場合は、
  保存時に重複除去して扱う

---

## 6. `mcp` 名前空間

### 6.1 基本方針

`mcp` は MCP 連携用メタ情報の名前空間とする。

1つのページは、同時に複数の MCP サービス種別へ属する前提を採らない。
つまり、1ページにつき `mcp` が表す MCP primitive は 1種類とする。

この方針により以下の利点を得る。

- ページの責務が曖昧になりにくい
- 保存時バリデーションを単純化できる
- 派生データ更新処理を単純化できる
- 将来の運用上の混乱を減らせる

### 6.2 `mcp.primitive`

MCP 側での primitive の考え方に合わせ、
`mcp` の種別指定には `type` ではなく `primitive` を採用する。

以下の方針とする。

- `mcp.primitive` は必須
- `mcp.primitive` は文字列
- primitive ごとに許可される追加プロパティを切り替える
- `prompt` および `resource` を基本スキーマ検証の受理対象とする

受理する primitive:

- `prompt`
- `resource`

注記:

- primitive 指定には `mcp.primitive` を使用する
- `resource` の機能固有仕様は、M4で定義したMCP resources連携仕様に従う

### 6.3 `mcp` の構造例

`prompt` の例:

```yaml
mcp:
  primitive: prompt
  name: "ページ要約"
  description: "ページ内容を要約するための prompt"
  system: "必要に応じて system prompt を記述する"
  arguments:
    - name: target
      description: "対象ページ"
      required: true
```

注記:

- 初期版の `prompt` はページ内で完結する定義を前提とする
- したがって、カーソル継続取得やページング継続位置に関する
  メタ情報は初期段階では持たない
- `arguments` は `prompts/list` で配信する引数定義へ対応付ける
- prompt 本文自体はページ本文側を利用する前提とし、
  front matter には一覧化・公開判定・引数定義に必要な情報を置く

`resource` の例:

```yaml
mcp:
  primitive: resource
  resource_path: /docs/front-matter
  name: "Front Matter Specification"
  description: "LuWiki front matter specification"
  mime_type: text/markdown
  resource_acl:
    default:
      list: true
      read: true
    list:
      allow:
        - docs-reader
    read:
      deny:
        - suspended-token
```

注記:

- `resource_path` は省略可能とする
- `resource_path: null` は省略と同義とする
- `resource_path` 省略時は、対象ページの current path から `/pages/<page-path-without-leading-slash>` を resource path として使用する
- `mime_type` は省略可能とし、省略時は `text/markdown` を既定値とする
- `resource_acl` は省略可能とし、省略時は Bearer の `read` scope とページ状態を満たす全tokenへ公開する
- resource 本文自体はページ本文側を利用する前提とし、front matter には一覧化、URI解決、公開判定に必要な情報を置く

### 6.4 prompt の値制約

#### 6.4.1 `mcp.name`

- 必須の文字列とする
- 空文字および空白文字だけの値を許容しない
- 先頭および末尾の空白文字を許容しない
- 制御文字を許容しない
- 最大128文字とし、Unicode scalar value 単位で数える
- 内部空白および Unicode 文字を許容する
- trim、小文字化、Unicode 正規化を行わず、入力値を保持する

#### 6.4.2 `mcp.description`

- 必須の文字列とする
- 空文字および空白文字だけの値を許容しない
- 最大1024文字とし、Unicode scalar value 単位で数える
- 通常の空白、複数行、末尾改行を許容する
- trim、改行正規化、Unicode 正規化を行わず、入力値を保持する

#### 6.4.3 `mcp.system`

- 任意項目とし、未指定および `null` は未指定として扱う
- 文字列として指定した場合、空文字および空白文字だけの値を許容しない
- 最大8192文字とし、Unicode scalar value 単位で数える
- 通常の空白、タブ、LF、CR、Unicode 文字を許容する
- タブ、LF、CR以外の制御文字を許容しない
- 先頭・末尾空白、複数行、末尾改行を保持する
- trim、改行正規化、Unicode 正規化を行わない

#### 6.4.4 `mcp.arguments`

- 任意項目とし、省略時は引数定義なしとして扱う
- 明示的な空配列を許容しない
- 配列の記載順を保持する
- 各要素は `name` と `description` を必須とする
- 各要素の `required` は任意の boolean とする
- `required` は未指定、`false`、`true` の三状態を保持する
- 引数名は最大64文字とし、Unicode scalar value 単位で数える
- 引数名は `^[A-Za-z_][A-Za-z0-9_-]*$` に一致すること
- 引数名は大文字・小文字を区別する完全一致で重複を拒否する
- 引数説明は空文字および空白文字だけの値を許容しない
- 引数説明は最大1024文字とし、Unicode scalar value 単位で数える
- 引数説明は通常の空白、タブ、LF、CR、Unicode 文字を許容する
- 引数説明ではタブ、LF、CR以外の制御文字を許容しない
- prompt および各引数の未知プロパティを許容しない

### 6.5 prompt 本文と placeholder の保存時境界

- prompt 本文は front matter 終端直後からページソース末尾までの raw Markdown とする
- front matter の保存時検証では、
  prompt 本文が空または空白だけであることを理由に保存を拒否しない
- front matter の保存時検証では、
  `mcp.system` および本文に含まれる placeholder と
  `mcp.arguments` の参照整合性を検証しない
- `prompts/get` は最新ページソースの front matter を共通 parser で再検証する
- `prompts/get` で有効な未宣言 placeholder を検出した場合は、
  保存時入力不正ではなく最新正本の内部不整合として扱う
- placeholder の公開記法、要求引数検証、展開規則は
  組み込み MCP リソース `mcp-prompt-spec` で確認できる

### 6.7 prompt 名の一意性

- prompt 名は MCP primitive ごとの名前空間で一意とする
- 一致判定は大文字・小文字を区別する完全一致とする
- trim、小文字化、Unicode 正規化を行わない
- `(primitive, name) => page_id` の共通名前索引で一意性を管理する
- 名前索引はページ正本と同じ redb write transaction で更新する
- 同一ページが現在所有する名前の再利用は許容する
- 別ページが同一 prompt 名を所有する場合は正本の commit 前に拒否する
- prompt 指定解除、名前変更、別 primitive への変更時は旧キーを解放する
- soft delete では名前予約を維持し、hard delete で解放する
- import および既存ページからの初期構築でも同じ重複規則を適用する

### 6.9 resource の値制約

#### 6.9.1 `mcp.resource_path`

- optional な文字列とする
- `null` は未指定と同義として扱う
- 未指定時はページの current path から `/pages/<page-path-without-leading-slash>` を導出する
- 文字列として指定する場合は `/` で始まる絶対 URI path とする
- 空文字および空白文字だけの値を許容しない
- 先頭および末尾の空白文字を許容しない
- 制御文字を許容しない
- 最大512文字とし、Unicode scalar value 単位で数える
- `/builtin` および `/builtin/` 配下を許容しない
- `/pages` および `/pages/` 配下を明示値として許容しない
- `/` そのものおよび `/` で終わる値を許容しない
- `//` を含む値を許容しない
- `.` または `..` の path segment を含む値を許容しない
- trim、小文字化、Unicode 正規化を行わず、入力値を保持する

#### 6.9.2 `mcp.name`

- 必須の文字列とする
- 空文字および空白文字だけの値を許容しない
- 先頭および末尾の空白文字を許容しない
- 制御文字を許容しない
- 最大128文字とし、Unicode scalar value 単位で数える
- 内部空白および Unicode 文字を許容する
- trim、小文字化、Unicode 正規化を行わず、入力値を保持する

#### 6.9.3 `mcp.description`

- 必須の文字列とする
- 空文字および空白文字だけの値を許容しない
- 改行およびタブ以外の制御文字を許容しない
- 最大1024文字とし、Unicode scalar value 単位で数える
- trim、小文字化、Unicode 正規化を行わず、入力値を保持する

#### 6.9.4 `mcp.mime_type`

- optional な文字列とする
- 未指定時は MCP resources 公開時に `text/markdown` を既定値として扱う
- 空文字を許容しない
- 最大128文字とし、Unicode scalar value 単位で数える
- ASCII 文字だけを許容する
- 空白文字および制御文字を許容しない
- `type/subtype` の essence 形式とする
- type と subtype は空であってはならない
- subtype 内に `/` を含めない
- type と subtype は MIME type token 文字だけで構成する

#### 6.9.5 resource 固有の禁止プロパティ

- `mcp.primitive = resource` では `mcp.system` を許容しない
- `mcp.primitive = resource` では `mcp.arguments` を許容しない
- `mcp.resource_id` は廃止済み項目として扱い、指定された場合は `mcp.resource_path` の使用を求める validation error とする
- resource の未知プロパティを許容しない

#### 6.9.6 `mcp.resource_acl`

- 任意項目とし、未指定および `null` は ACL 未指定として扱う
- `default.list` と `default.read` は任意の boolean とする
- `list.allow`、`list.deny`、`read.allow`、`read.deny` は任意の文字列配列とする
- `allow` および `deny` は、指定する場合は空配列を許容しない
- ACL principal は Bearer token ID または Bearer token name と照合する
- ACL principal が ULID 形式の場合は token ID として扱い、それ以外は token name として扱う
- ACL principal は空文字、前後空白、制御文字を許容しない
- 同一操作内では deny を allow より優先する
- allow に一致する場合は許可する
- deny と allow のどちらにも一致しない場合は `default.<operation>` を使用する
- `default.<operation>` も未指定の場合は許可する
- `resources/list` 用 ACL は resource の発見可能性だけを制御する
- `resources/read` 用 ACL は URI 指定時の取得可能性だけを制御し、list ACL を継承しない

### 6.10 resource 本文と保存時境界

- resource 本文は front matter 終端直後からページソース末尾までの raw Markdown とする
- front matter の保存時検証では、resource 本文が空または空白だけであることを理由に保存を拒否しない
- `resources/read` は最新ページソースの front matter を共通 parser で再検証する
- `resources/read` で front matter、resource path、最新ソース、URI索引の不整合を検出した場合は、最新正本または派生データの内部不整合として扱う

### 6.12 resource URI の一意性

- ページ由来 resource は resource path から生成する URI で一意に識別する
- resource path の一致判定は大文字・小文字を区別する完全一致とする
- trim、小文字化、Unicode 正規化を行わない
- `resource_path => page_id` の resource URI 逆引き索引で一意性を管理する
- URI逆引き索引はページ正本と同じ redb write transaction で更新する
- 同一ページが現在所有する resource path の再利用は許容する
- 別ページが同一 resource path を所有する場合は正本の commit 前に拒否する
- resource 指定解除、明示 resource path 変更、別 primitive への変更時は旧 resource path を解放する
- soft delete では resource path 予約を維持し、hard delete で解放する
- import、rollback、amend、および既存ページからの初期構築でも同じ重複規則を適用する
- `/builtin` および `/pages` 配下は LuWiki 予約 path として扱い、明示 `mcp.resource_path` では使用しない

## 7. `custom_meta` 名前空間

### 7.1 基本方針

`custom_meta` は、ユーザが自由に定義できる補助メタ情報の
予約名前空間とする。

- LuWiki 本体は `custom_meta` 配下の個別キー意味を解釈しない
- `custom_meta` は全文検索時の補助情報保持を主要用途とする
- `custom_meta` の内容はページソースへそのまま保持し、
  保存・再読込・全文検索対象化できることを前提とする

### 7.2 構造制約

- `custom_meta` 自体の値は object 限定とする
- `custom_meta` 配下のキーは文字列キーとする
- `custom_meta` 配下の値は文字列、数値、真偽値、null、配列、object を許容する
- `custom_meta` 配下で object を使う場合も、再帰的に文字列キー map として扱う

例:

```yaml
custom_meta:
  project: "alpha"
  priority: 3
  flags:
    reviewed: true
  tags:
    - release
```

不許可例:

```yaml
custom_meta: "alpha"
```

---

## 8. スキーマ定義

本章のスキーマは JSON Schema 風の表記である。
厳密な JSON Schema としての妥当性よりも、構造の共有を目的とする。

```yaml
type: "object"
properties:
  wiki:
    type: "object"
    properties:
      template:
        $ref: "#/definitions/wiki/template"
      tags:
        $ref: "#/definitions/wiki/tags"
    additionalProperties: true

  mcp:
    oneOf:
      - $ref: "#/definitions/mcp/prompt"
      - $ref: "#/definitions/mcp/resource"
    discriminator:
      propertyName: "primitive"
      mapping:
        prompt: "#/definitions/mcp/prompt"
        resource: "#/definitions/mcp/resource"

  custom_meta:
    $ref: "#/definitions/custom_meta"

additionalProperties: false

definitions:
  wiki:
    template:
      description: >-
        テンプレートページであることを示し、
        テンプレート一覧表示およびテンプレート適用時に
        必要なメタ情報を保持する。
      type: "object"
      required:
        - "name"
      properties:
        name:
          description: >-
            テンプレート名。
            ページ名とは独立した表示名。
          type: "string"

        description:
          description: >-
            テンプレート用途の説明文。
          type: "string"

        macro_expand:
          description: >-
            テンプレート適用時に、プレースホルダのうち
            即時展開マクロと同名のものを展開するか否か。
          type: "boolean"
      additionalProperties: false

    tags:
      description: >-
        ページに紐付くタグ一覧。
        初期段階では文字列タグの列挙として扱う。
      type: "array"
      items:
        type: "string"
        minLength: 1
        pattern: "^[^[:space:][:cntrl:]]+$"
      minItems: 1

  mcp:
    prompt:
      description: >-
        MCP の prompt primitive に対するメタ情報。
        原則として prompts/list で配信する情報を
        ページ front matter へマッピングする。
        初期版では prompt はページ内で完結するものとし、
        カーソル継続取得やページング継続位置に関する
        項目は持たない。
      type: "object"
      required:
        - "primitive"
        - "name"
        - "description"
      properties:
        primitive:
          description: >-
            MCP primitive 種別。
            prompt 固定。
          type: "string"
          enum:
            - "prompt"

        name:
          description: >-
            prompt の表示名。
          type: "string"
          minLength: 1
          maxLength: 128

        description:
          description: >-
            prompt の説明文。
          type: "string"
          minLength: 1
          maxLength: 1024

        system:
          description: >-
            prompt 実行時に補助的に利用する system 情報。
            prompt 本文本体とは別に、公開メタ情報として
            付与したい補助情報がある場合に用いる。
          type: "string"
          minLength: 1
          maxLength: 8192

        arguments:
          description: >-
            prompts/list で配信する arguments 相当の
            prompt 引数定義一覧。
          type: "array"
          items:
            $ref: "#/definitions/mcp/prompt_argument"
          minItems: 1
      additionalProperties: false

    prompt_argument:
      description: >-
        prompt 引数定義。
        prompts/list の引数情報へ対応付けることを想定する。
      type: "object"
      required:
        - "name"
        - "description"
      properties:
        name:
          type: "string"
          minLength: 1
          maxLength: 64
          pattern: "^[A-Za-z_][A-Za-z0-9_-]*$"

        description:
          type: "string"
          minLength: 1
          maxLength: 1024

        required:
          type: "boolean"
      additionalProperties: false

    resource:
      description: >-
        MCP の resource primitive に対するメタ情報。
        MCP標準resources/listおよびresources/readで公開する情報を
        ページfront matterへマッピングする。
      type: "object"
      required:
        - "primitive"
        - "name"
        - "description"
      properties:
        primitive:
          description: >-
            MCP primitive 種別。
            resource 固定。
          type: "string"
          enum:
            - "resource"

        resource_path:
          description: >-
            ページ由来resource URIの絶対path。
            未指定またはnull時は対象ページのcurrent pathから/pages配下へ導出する。
          type: "string"
          minLength: 1
          maxLength: 512

        name:
          description: >-
            resource の表示名。
          type: "string"
          minLength: 1
          maxLength: 128

        description:
          description: >-
            resource の説明文。
          type: "string"
          minLength: 1
          maxLength: 1024

        mime_type:
          description: >-
            resource 本文の MIME type。
            未指定時は text/markdown として公開する。
          type: "string"
          minLength: 1
          maxLength: 128

        resource_acl:
          description: >-
            resources/list および resources/read の token 単位 ACL。
          type: "object"
      additionalProperties: false

  custom_meta:
    description: >-
      ユーザ定義メタ情報領域。
      LuWiki 本体は配下の個別キー意味を解釈しないが、
      保存・再読込・全文検索対象化の対象とする。
    type: "object"
    propertyNames:
      type: "string"
    additionalProperties:
      $ref: "#/definitions/custom_meta/value"

    value:
      description: >-
        `custom_meta` 配下で許容する値。
      oneOf:
        - type: "string"
        - type: "number"
        - type: "boolean"
        - type: "null"
        - type: "array"
          items:
            $ref: "#/definitions/custom_meta/value"
        - type: "object"
          propertyNames:
            type: "string"
          additionalProperties:
            $ref: "#/definitions/custom_meta/value"
```

---

## 9. バリデーション方針

M1 の実装では、front matter の処理段階を以下の順序で扱う。

1. ページソース先頭からの front matter 抽出
2. YAML パース
3. `serde` による内部構造体へのデシリアライズ
4. `validate()` 相当処理による妥当性検証
5. 各機能へのデータ分配

ページソース先頭から front matter ブロックを切り出す処理は補助処理として扱い、
front matter の内部データ構造そのものとは分離する。

### 9.1 共通

- front matter 全体が YAML として構文正当であること
- トップレベルが object であること
- `wiki` が存在する場合は object であること
- `mcp` が存在する場合は object であること
- `custom_meta` が存在する場合は object であること
- top-level では `wiki` 、 `mcp` 、 `custom_meta` 以外を許容しない

### 9.2 `wiki`

- `wiki` 配下の個別項目制約は別途定義する
- 未定義項目の扱いは今後検討する
- `wiki` および `mcp` は optional な名前空間として扱う
- `wiki.tags` は空配列を許容しない
- `wiki.tags` の各要素は空文字を許容しない
- `wiki.tags` の各要素は空白文字および制御文字を含めない

### 9.3 `mcp`

- `mcp.primitive` は必須
- `mcp.primitive` に応じて許可プロパティを切り替える
- 未対応 primitive が指定された場合は保存時エラーとする
- `mcp.primitive = prompt` では、
  `prompts/list` へマッピング可能な情報のみを
  front matter 上へ持つ
- 初期版の `prompt` ではカーソル関連情報を定義しない
- prompt の name、description、system、arguments は
  6.4 の値制約を満たすこと
- prompt および各 argument の未知プロパティを許容しない
- 引数名の重複は大文字・小文字を区別する完全一致で拒否する
- prompt 名のページ横断一意性は、
  Wiki 全体で検証する
- `mcp.primitive = resource` では、
  `resources/list` および `resources/read` へマッピング可能な情報のみを
  front matter 上へ持つ
- resource の resource_path、name、description、mime_type、resource_acl は
  6.9 の値制約を満たすこと
- resource の未知プロパティを許容しない
- resource path のページ横断一意性は、
  Wiki 全体で検証する

### 9.4 `custom_meta`

- `custom_meta` は optional な名前空間として扱う
- `custom_meta` 自体が scalar または array の場合は保存時エラーとする
- `custom_meta` 配下の key は文字列キーであることを要求する
- `custom_meta` 配下の value は自由とするが、
  object は再帰的に文字列キー map であることを要求する
- `custom_meta` の validation error は `custom_meta` またはその配下を
  `property_path` に含めて返す
- `wiki` / `mcp` の既存 validation error 表現は `custom_meta` 追加後も維持する

---
