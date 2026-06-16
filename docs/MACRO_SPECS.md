# マクロ仕様書
本書ではアプリケーションに組み込むマクロについて定義する。

## 基本仕様
- 以下の3つのタイプをサポートする。
  - 入力時に即座に展開される即時変換型
  - レンダリング時に展開されるレンダリング時変換型
  - 特例マクロ

## マクロ一覧

  | マクロ名 | 種別 | 機能
  |:--|:--|:--
  | `now[:utc][:iso8601]` | 即時変換型 | [現在の日時を展開](#macro-now)
  | `today[:utc][:iso8601]` | 即時変換型 | [現在の日付を展開](#macro-today)
  | `page[:id][:basename]` | 即時変換型 | [ページ情報を展開](#macro-page)
  | `user[:display]` | 即時変換型 | [ユーザ情報を展開](#macro-user)
  | `children[:depth={number}][:recursive]` | レンダリング時変換型 | [子ページのリストへ展開](#macro-children)
  | `toc[:depth={number}]` | レンダリング時変換型 | [ページ内のTOCへ展開](#macro-toc)
  | `include_code:src={asset_path}[:lang={string}]` | レンダリング時変換型 | [アセットのコードブロック展開](#macro-include_code)
  | `include_csv:src={asset_path}` | レンダリング時変換型 | [アセットのテーブル展開](#macro-include_csv)
  | `[[{page_path}]]` | 特例型 | [ページへのリンク](#macro-page_link)
  | `[[{page_path}\|{alias_name}]]` | 特例型 | [ページへのリンク](#macro-alias_link)
  | `![[asset:{asset_path}]]` | 特例型 | [アセットの埋め込み](#macro-asset_link)

## 波括弧記法の責務境界

LuWikiでは、既存マクロのほかにテンプレート適用時および
MCP `prompts/get`時に処理する専用placeholderを使用する。

| 記法 | 用途 | 処理時点 |
|:--|:--|:--|
| `{{macro}}` | 既存の即時変換・レンダリング時マクロ | 編集時または表示時 |
| `{{!macro}}` | テンプレート適用時専用の即時変換placeholder | テンプレート適用時 |
| `{{@name}}` | MCP prompt要求引数placeholder | `prompts/get`時 |
| `{{@@name}}` | `{{@name}}`のリテラル表現 | `prompts/get`時 |

各処理系は自身が担当する記法だけを展開する。

- 既存マクロ処理は`{{@name}}`および`{{@@name}}`を展開しない
- prompt引数展開は`{{macro}}`および`{{!macro}}`を展開しない
- prompt引数展開はwiki linkおよびasset linkを変更しない
- prompt引数値に含まれる各記法を再処理しない

## テンプレート適用時専用placeholder

### `{{!macro}}`

`{{!macro}}`は、テンプレート適用時に既存の即時変換型マクロを
明示的に展開するための専用placeholderとする。

例:

- `{{!today:iso}}`
- `{{!page:basename}}`
- `{{!user:display}}`

以下の規則を適用する。

- `wiki.template.macro_expand = true`の場合だけ展開する
- 展開時は`!`を除去し、残りを既存の即時変換型マクロとして処理する
- `macro_expand = false`または未指定の場合は入力をそのまま保持する
- inline codeおよびfenced code内では展開しない
- MCP prompt引数placeholderとは別の記法として扱う
- `prompts/get`では`{{!macro}}`を展開しない

## MCP prompt専用placeholder

MCP prompt専用placeholderは、`mcp.primitive = prompt`の
systemおよびページ本文に記述し、`prompts/get`時に処理する。

公開契約の詳細は`MCP_PROMPT_SPECS.md`を参照すること。

### `{{@name}}`

`{{@name}}`は、`mcp.arguments`で宣言した同名引数の文字列を
挿入するplaceholderとする。

以下の規則を適用する。

- systemと本文へ個別に一回だけ展開を適用する
- 同じplaceholderが複数ある場合はすべて展開する
- optional引数が未指定の場合は空文字列へ展開する
- 挿入値に含まれるplaceholder、既存マクロ、wiki linkを再展開しない
- inline codeおよびfenced code内でも展開する
- `\{{@name}}`のバックスラッシュをescapeとして扱わず、
  バックスラッシュを保持したままplaceholderを展開する
- 引数名規則に一致しない`{{@...}}`は変更しない
- 閉じ`}}`がない記法は変更しない
- 有効な未宣言placeholderは`prompts/get`時の内部不整合とする
- front matter保存時にはplaceholderと引数定義の参照整合性を検証しない

### `{{@@name}}`

`{{@@name}}`は、prompt本文またはsystemへ
リテラル`{{@name}}`を含めるためのprompt専用escapeとする。

以下の規則を適用する。

- `prompts/get`時に`{{@name}}`へ一回だけ戻す
- 戻した`{{@name}}`を同じ処理中に再展開しない
- 既存マクロ処理およびテンプレート適用時処理のescapeには使用しない

## 即時変換型マクロ
即時変換型は編集画面のエディタ上で入力時にリアルタイムに展開が行われる(このためソースには残らない)。

入力形式は`{{マクロ名:引数..}}`とする。

例を上げると以下のような記述となる。
- `{{now}}`
- `{{now:utc:iso8601}}`

<a id="macro-now"></a>
### now
現在の日時を展開する。

#### 引数

| 引数 | 意味 | 省略形 | 指定
|:--|:--|:--|:--
| `utc`     | UTCでの日時展開を行う   | (なし) | 任意
| `iso8601` | ISO8601形式で展開を行う | `iso`  | 任意

#### 注記
- `utc`が指定されなかった場合はローカルタイムで展開が行われる。
- `iso8601`が指定されなかった場合は年月日区切りが"/"で展開が行われる。

<a id="macro-today"></a>
### today
現在の日付を展開する。

#### 引数

| 引数 | 意味 | 省略形 | 指定
|:--|:--|:--|:--
| `utc`     | UTCでの日付展開を行う   | (なし) | 任意
| `iso8601` | ISO8601形式で展開を行う | `iso` | 任意

#### 注記
- `utc`が指定されなかった場合はローカルタイムで展開が行われる。
- `iso8601`が指定されなかった場合は年月日区切りが"/"で展開が行われる。

<a id="macro-page"></a>
### page
現在のページの情報を展開する。

#### 引数
デフォルトではページパスを展開する

| 引数 | 意味 | 省略形 | 指定
|:--|:--|:--|:--
| `id` | ページIDの展開を行う | (なし) | 任意
| `basename` | ページ名(パスの最後のエレメント)の展開を行う | b,bn | 任意

#### 注記
- `id`と`basename`の同時指定はエラーとして扱う。

<a id="macro-user"></a>
### user
ユーザ情報を展開する。

#### 引数
デフォルトではユーザIDへの展開を行う。

| 引数 | 意味 | 省略形 | 指定
|:--|:--|:--|:--
| `display` | 表示名の展開を行う | `d` | 任意

## レンダリング時変換型マクロ

入力形式は`{{マクロ名:引数..}}`とする。例を上げると以下のような記述となる。

例を上げると以下のような記述となる。

- `{{children:depth=3}}`
- `{{include_code:src=test.cpp:lang=cpp}}`

<a id="macro-children"></a>
### children
配下のページへのリンクをアイテムとしたリストに展開される。

#### 引数

| 引数 | 意味 | 省略形 | 指定
|:--|:--|:--|:--
| `depth={number}` | 表示名の展開を行う見出しの深さ | `d` | 任意
| `recursive` | 再帰的に配下のページ全てを展開 | `r` | 任意

#### 注記
- `depth`と`recursive`の両方が指定されなかった場合は`depth=1`として扱う(直下のページのみでリストを構成)。
- `depth`と`recursive`の同時指定はエラーとして扱う

<a id="macro-toc"></a>
### toc
ページ内へのTOCをリストとして展開する。

#### 引数

| 引数 | 意味 | 省略形 | 指定
|:--|:--|:--|:--
| `depth={numer}` | 表示名の展開を行う | `d` | 任意

#### 注記
- `depth`が指定されなかった場合は`depth=3`として扱う(第2〜4レベル見出しの3レベル分でリストを構成する)。

<a id="macro-include_code"></a>
### include_code
アセットを読み込みコードブロックに展開する。

#### 引数

| 引数 | 意味 | 省略形 | 指定
|:--|:--|:--|:--
| `src={string}`  | コードブロックに展開するアセットへのパス | `s` | 必須
| `lang={string}` | コードブロックに対する言語指定 | `l` | 任意

##### 注記
- `lang`を指定しなかった場合はMIME種別から推測される言語指定を行う
- MIME種別が`text/*`に一致しないアセットが指定された場合ははエラーとなる。

<a id="macro-include_csv"></a>
### include_csv
アセット(CSV)を読み込みテーブルに展開する。

| 引数 | 意味 | 省略形 | 指定
|:--|:--|:--|:--
| `src={string}`  | テーブルに展開するアセットへのパス | `s` | 必須

##### 注記
- MIME種別が`text/csv`に一致しないアセットが指定された場合ははエラーとなる。

## 特例型マクロ

特例型マクロは、他システムの入力形式のエミュレートなどを行うためのマクロとして定義される。

<a id="macro-page_link"></a>
### `[[{page_path}]]` : ページリンクマクロ
Scrapbox / WikiWiki / Obsidian 系由来

#### 引数

| 引数 | 意味 | 指定
|:--|:--|:--
| `{page_path}`  | リンク先のページパス | 必須

### 注記
- `[[/path/to/page#fragment]]`は`[page](/path/to/page#fragment)`と等価に展開される。

<a id="macro-alias_link"></a>
### `[[{page_path}|{alias_name}]]` : ページリンクマクロ
Scrapbox / WikiWiki 系由来

#### 引数

| 引数 | 意味 | 指定
|:--|:--|:--
| `{page_path}`  | リンク先のページパス | 必須
| `{alias_name}`  | 別名 | 必須

### 注記
- `[[/path/to/page#fragment|alias]]`は`[alias](/path/to/page#fragment)`と等価に展開される。

<a id="macro-asset_link"></a>
### `![[asset:{asset_path}]]` : アセットの埋め込み
Obsidian 系由来

#### 引数

| 引数 | 意味 | 指定
|:--|:--|:--
| `{asset_path}`  | アセットへのパス | 必須

### 注記
- `![[asset:/path/to/page:image.png]]`は`[image.png](!asset:/path/to/page:image.png)`と等価に展開される。
