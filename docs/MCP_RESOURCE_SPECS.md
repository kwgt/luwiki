# MCP resources仕様

本書は、LuWikiがMCPクライアントへ公開するresourcesの外部契約を定義する。

front matterの構造および保存時制約は
`FRONT_MATTER_SPECS.md`を参照すること。
MCP内部の責務分割、永続化、transportは各MCP設計文書を参照すること。

---

## 1. 対象範囲

本書は以下を対象とする。

- 固定組み込みresourceの公開条件
- ページ由来resourceの公開条件
- MCP標準`resources/list`
- MCP標準`resources/read`
- front matterからMCP型へのfield mapping
- resource URI
- cursorページング
- 認証・認可
- protocol error
- capabilityと通知
- 監査および機密情報の取り扱い

本書は以下を対象としない。

- redbテーブルの物理構造
- database transactionおよび内部helperの詳細
- Actix middlewareの内部構成
- CLI再構成処理の内部実装
- Wiki tagsの将来仕様

---

## 2. resource種別

### 2.1 固定組み込みresource

- 固定組み込みresourceはLuWiki実装が提供する読み取り専用resourceとする
- 初期版では以下を公開する
  - `luwiki://local.luwiki/builtin/front-matter-spec`
  - `luwiki://local.luwiki/builtin/mcp-prompt-spec`
- 固定組み込みresourceはページpath、page ID、path prefix制約に依存しない
- 固定組み込みresourceのMIME typeは`text/markdown`とする

### 2.2 ページ由来resource

- ページのfront matterに`mcp.primitive = resource`がある場合、
  そのページをMCP resourceとして扱う
- ページ由来resourceは`mcp.resource_id`またはcurrent path由来の
  resource IDで識別する
- `mcp.resource_id`未指定時は、current pathから先頭の`/`を除いた文字列を
  resource IDとして使用する
- 初期版のページ由来resourceはfront matterとページ本文だけで定義が完結する
- ページpathおよびpage IDはMCP公開結果へ含めない

### 2.3 正本

- ページ由来resourceの正本は最新ページソース内のfront matterと本文とする
- resource候補派生データは一覧取得のための再構成可能な情報とする
- resource URI逆引き索引はURI解決のための再構成可能な情報とする
- `resources/read`のURI解決と本文取得は最新ページソースを基準とする
- 過去revisionをresource公開対象として指定する機能は提供しない

---

## 3. resource URI

### 3.1 URI形式

LuWikiのresource URIは以下の形式とする。

```text
luwiki://<authority>/builtin/<builtin_id>
luwiki://<authority>/page/<resource_id>
```

初期版のauthorityは`local.luwiki`とする。

### 3.2 固定組み込みresource URI

- 固定組み込みresourceは`/builtin/`配下のURIで公開する
- `builtin_id`は空であってはならない
- `builtin_id`に`/`、境界空白、制御文字を含めない
- 未知の`builtin_id`はnot foundとして扱う

### 3.3 ページ由来resource URI

- ページ由来resourceは`/page/`配下のURIで公開する
- `resource_id`はfront matter仕様の`mcp.resource_id`制約を満たす
- `builtin/`で始まるresource IDは固定組み込みresource用に予約する
- authorityが一致しないURIはnot foundとして扱う

---

## 4. front matterとMCP fieldの対応

### 4.1 `resources/list`

| 入力元 | MCP field | 取り扱い |
|:--|:--|:--|
| resource URI | `Resource.uri` | LuWiki resource URIを使用 |
| `mcp.name` | `Resource.name` | 入力値を変換せず使用 |
| `mcp.description` | `Resource.description` | 入力値を使用 |
| `mcp.mime_type` | `Resource.mimeType` | 未指定時は`text/markdown` |

追加の規則は以下とする。

- 固定組み込みresourceとページ由来resourceを同じ一覧結果へ合流する
- `Resource.meta`およびannotation相当の追加情報は設定しない
- ページpath、page ID、path prefix判定情報を公開しない

### 4.2 `resources/read`

- 返却するcontentsはtext contentsとする
- contentsの`uri`には解決済みresource URIを設定する
- contentsの`mimeType`にはresourceのMIME typeを設定する
- 固定組み込みresourceの本文は埋め込み文書本文を使用する
- ページ由来resourceの本文は、front matter除去後の最新raw Markdown本文を使用する
- ページ由来resourceのrevisionは監査ログ内部で扱い、MCP結果へは公開しない

---

## 5. 認証・認可

### 5.1 Bearer認証

- MCP transportではBearer認証を要求する
- Authorization欠落はHTTP 401、
  reason `missing bearer token for MCP`として拒否する
- 存在しないBearerおよび失効済みBearerはHTTP 401、
  reason `unauthorized`として拒否する
- transport認証失敗はresources handlerへ到達する前に処理する
- transport認証失敗時はresource一覧、resource URI、本文、
  JSON-RPC resultおよびJSON-RPC errorを返さない

### 5.2 scope

- `resources/list`と`resources/read`は`read` scopeを要求する
- `append`などのwrite系scopeは`read`を暗黙包含しない
- `ReadOnly`属性を持つユーザでも、
  Bearerに`read` scopeがあればresources操作を利用できる
- scope不足はtransport認証失敗ではなくJSON-RPC認可失敗とする

### 5.3 path prefix

- 固定組み込みresourceにはページ用path prefix制約を適用しない
- ページ由来resourceにはページ用path prefix制約を適用する
- `resources/list`では、current pathがpath prefix範囲外のページ由来resourceを除外する
- `resources/read`では、current pathがpath prefix範囲外のページ由来resourceを
  not foundとして扱う
- 同じBearerを使用するpath基準MCP toolsには、
  従来どおりpath prefix制約を適用する

---

## 6. ページ状態と公開条件

| ページ状態 | `resources/list` | `resources/read` | URI予約 |
|:--|:--|:--|:--|
| 通常ページ・latest | 公開 | 取得可能 | 維持 |
| draft | 非公開 | not found | 登録しない |
| soft delete | 非公開 | not found | 維持 |
| undelete後 | 条件を満たせば再公開 | 取得可能 | 維持 |
| hard delete後 | 非公開 | not found | 解放 |
| 過去revision | 非公開 | 直接取得不可 | latestを基準 |

追加の規則は以下とする。

- 明示`mcp.resource_id`を持つページのrenameはresource URIを変更しない
- `mcp.resource_id`未指定ページのrenameはcurrent path由来resource IDを更新する
- 一覧候補に対応するページ索引がない場合は公開しない
- URI索引の解決先ページが存在しない場合はnot foundとして扱う
- URI索引と最新ソースのprimitiveまたはresource IDが一致しない場合は
  内部不整合として扱う
- 非公開状態の違いを外部エラーから判別可能にしない

---

## 7. `resources/list`

### 7.1 基本動作

- 公開可能なresourceを`Resource`の配列として返す
- 固定組み込みresourceとページ由来resourceを合流してからソートする
- 状態フィルタをソート、cursor、件数上限より先に適用する
- 候補がない場合は正常な空一覧を返す
- URI重複など一覧合流と矛盾する状態を黙って公開しない

### 7.2 ソート

- `Resource.uri`のcase-sensitiveな文字列比較で昇順に並べる
- Rustの`str::cmp`相当の順序を使用する
- 小文字化、locale依存照合、Unicode正規化を行わない

### 7.3 ページサイズ

- 1要求で返すresourceは最大50件とする
- 実装は最大51件を取得し、続きの有無を判定できる
- 続きがある場合は返却する50件目のresource URIを`nextCursor`とする
- 続きがない場合は`nextCursor`を設定しない

---

## 8. cursor

### 8.1 境界規則

- cursor未指定時は先頭から取得する
- cursor指定時は`uri > cursor`のresourceを対象とする
- cursor自身を次ページへ含めない
- cursor URIが実在することを要求しない
- 有効なcursorより後ろに候補がない場合は正常な空一覧を返す

### 8.2 値制約

- cursorはLuWiki resource URI形式であることを要求する
- cursorは空文字および空白だけの値を許容しない
- cursorの先頭・末尾空白を許容しない
- cursorに制御文字を許容しない
- cursorのauthorityは`local.luwiki`とする
- `/builtin/<builtin_id>`または`/page/<resource_id>`だけを許容する
- `/page/<resource_id>`のresource IDには`mcp.resource_id`と同じ値制約を適用する
- cursorの最大長は`luwiki://local.luwiki/page/`と512文字のresource IDを合わせた長さとする

### 8.3 snapshot

- cursorはDB snapshotまたは一覧世代を表さない
- 各要求時点の最新公開候補集合に対するURI境界として扱う
- 継続取得中の追加、削除、rename、resource ID変更により、
  結果の重複または欠落が生じないことは保証しない

---

## 9. `resources/read`

### 9.1 URI解決

- URIから固定組み込みresourceまたはページ由来resourceを判定する
- 固定組み込みresourceは実装内の固定IDから本文を解決する
- ページ由来resourceはresource URI逆引き索引からpage IDを解決する
- ページ由来resourceは解決後に最新ページ状態、latest revision、latest sourceを取得する
- 最新ソースのfront matterを共通parserで再検証する
- 最新ソースがresourceでない場合またはresource IDが一致しない場合は、
  別候補を探索せず内部不整合とする

### 9.2 本文取得

- 固定組み込みresourceは対応する埋め込み文書本文を返す
- ページ由来resourceはfront matter終端直後からページソース末尾までの
  raw Markdownを返す
- front matterを本文へ含めない
- 本文が空または空白だけでも、resourceとして取得できる

---

## 10. エラー

### 10.1 論理区分

resources操作では以下の論理区分を用いる。

- `forbidden`
- `invalid_input`
- `not_found`
- `internal_error`

### 10.2 `resources/list`

- read scope不足は`forbidden`とする
- cursor形式不正は`invalid_input`とする
- DB失敗、URI重複、派生データ不整合は`internal_error`とする
- path prefix範囲外のページ由来resourceはエラーではなく一覧から除外する

### 10.3 `resources/read`

- read scope不足は`forbidden`とする
- URI形式不正は`invalid_input`とする
- authority不一致、未知の固定組み込みresource、存在しないページ由来resource、
  draft、soft delete、hard delete、path prefix範囲外は`not_found`とする
- URI索引と最新ソースの不整合、latest source欠落、front matter再検証失敗、
  DB失敗は`internal_error`とする
- `not_found`では非公開理由を区別できる詳細を返さない
- `internal_error`ではDB内部エラー、front matter本文、Bearer tokenを返さない

---

## 11. capabilityと通知

- resource URI逆引き索引が対応済みの構築状態である場合だけ、
  MCP server capabilitiesにresourcesを含める
- resources capabilityはtools capabilityおよびprompts capabilityと共存する
- `resources.listChanged`は宣言しない
- 保存、削除、rename、import、rollback、amend、再構成から
  resources/listChanged通知を送信しない
- クライアントはresource集合の最新状態が必要な場合、`resources/list`を再取得する

---

## 12. 監査と機密情報

### 12.1 記録対象

- `resources/list`は`list_resources` operationとして監査ログへ記録する
- `resources/read`は`read_resource` operationとして監査ログへ記録する
- 成功と失敗の両方を監査対象とする
- 監査ログ書き込みに失敗してもresources操作結果を壊さない

### 12.2 記録禁止情報

以下を監査ログ、通常ログ、protocol error detailへ記録しない。

- Bearer token
- Authorization header
- resource本文
- ページfront matter本文
- DB内部エラー詳細
- path prefix範囲外であることを示す詳細

### 12.3 summary

- `list_resources`のsummaryには件数やcursor有無など、本文を含まない概要だけを記録する
- `read_resource`のsummaryには成功・失敗、取得種別、revisionなど、
  本文を含まない概要だけを記録する
- resource URIは要求識別子として扱い、本文やfront matterとは別に記録可能とする

---

## 13. 派生データと再構成

- resource候補とresource URI逆引き索引は再構成可能な派生データとする
- `derived rebuild --target resources`はresources関連派生データを再構成する
- `derived rebuild --target all`はresources関連派生データを再構成対象に含める
- 再構成のtransaction境界、失敗時の既存データ維持、CLI引数は
  `MCP_SERVICE_AND_STORAGE_DESIGN.md`および`CLI_SPECS.md`を参照する

