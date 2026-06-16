# MCP prompts仕様

本書は、LuWikiがMCPクライアントへ公開するpromptsの外部契約を定義する。

front matterの構造および保存時制約は
`FRONT_MATTER_SPECS.md`を参照すること。
MCP内部の責務分割、永続化、transportは各MCP設計文書を参照すること。
prompt専用placeholderと既存マクロの境界は
`MACRO_SPECS.md`を参照すること。

---

## 1. 対象範囲

本書は以下を対象とする。

- promptページの公開条件
- MCP標準`prompts/list`
- MCP標準`prompts/get`
- front matterからMCP型へのfield mapping
- cursorページング
- 要求引数の検証
- prompt専用placeholderの展開
- message生成
- 認証・認可
- protocol error
- capabilityと通知
- 監査および機密情報の取り扱い

本書は以下を対象としない。

- redbテーブルの物理構造
- database transactionおよび内部helperの詳細
- Actix middlewareの内部構成
- CLI再構成処理の内部実装
- MCP resourcesおよびWiki tagsの将来仕様

---

## 2. promptページ

### 2.1 公開指定

- ページのfront matterに`mcp.primitive = prompt`がある場合、
  そのページをMCP promptとして扱う
- promptはページpathやpage IDではなく`mcp.name`で識別する
- `mcp.name`はprompt primitive内で一意とする
- 異なるprimitive間の同名可否は各primitiveの仕様で定義する
- 初期版のpromptはfront matterとページ本文だけで定義が完結する

### 2.2 正本

- promptの正本は最新ページソース内のfront matterと本文とする
- prompt候補派生データは一覧取得のための再構成可能な情報とする
- `prompts/get`の名前解決と本文取得は最新ページソースを基準とする
- prompt候補の欠損だけを理由として`prompts/get`を失敗させない
- 過去revisionをprompt公開対象として指定する機能は提供しない

### 2.3 保存時検証との境界

- front matter保存時にはprompt属性と引数定義を検証する
- front matter保存時には本文が空または空白だけであることを理由に
  保存を拒否しない
- front matter保存時にはsystemおよび本文内のplaceholderと
  引数定義の参照整合性を検証しない
- `prompts/get`で有効な未宣言placeholderを検出した場合は、
  最新正本の内部不整合として扱う

---

## 3. front matterとMCP fieldの対応

### 3.1 `prompts/list`

| front matter | MCP field | 取り扱い |
|:--|:--|:--|
| `mcp.name` | `Prompt.name` | 入力値を変換せず使用 |
| `mcp.description` | `Prompt.description` | `Some(...)`として使用 |
| `mcp.arguments` | `Prompt.arguments` | 定義順を維持 |
| `arguments[].name` | `PromptArgument.name` | 入力値を使用 |
| `arguments[].description` | `PromptArgument.description` | `Some(...)`として使用 |
| `arguments[].required` | `PromptArgument.required` | 三状態を維持 |

追加の規則は以下とする。

- `mcp.arguments`未指定時は`Prompt.arguments = None`とする
- `required`は未指定、`false`、`true`を正規化せず保持する
- `Prompt.title`、`Prompt.icons`、`Prompt.meta`は`None`とする
- `PromptArgument.title`は`None`とする
- `ListPromptsResult.meta`は`None`とする
- `mcp.system`は一覧へ独自fieldとして公開しない
- ページpath、page ID、path prefix判定情報を公開しない

### 3.2 `prompts/get`

- `GetPromptResult.description`には`mcp.description`だけを設定する
- `mcp.system`をdescriptionへ連結しない
- messageは9章の規則で生成する
- ページpath、page ID、revisionはMCP結果へ公開しない

---

## 4. 認証・認可

### 4.1 Bearer認証

- MCP transportではBearer認証を要求する
- Authorization欠落はHTTP 401、
  reason `missing bearer token for MCP`として拒否する
- 存在しないBearerおよび失効済みBearerはHTTP 401、
  reason `unauthorized`として拒否する
- transport認証失敗はprompts handlerへ到達する前に処理する
- transport認証失敗時はprompt一覧、prompt名、本文、
  JSON-RPC resultおよびJSON-RPC errorを返さない

### 4.2 scope

- `prompts/list`と`prompts/get`は`read` scopeを要求する
- `append`などのwrite系scopeは`read`を暗黙包含しない
- `ReadOnly`属性を持つユーザでも、
  Bearerに`read` scopeがあればprompts操作を利用できる
- scope不足はtransport認証失敗ではなくJSON-RPC認可失敗とする

### 4.3 path prefix

- prompts操作にはページ用path prefix制約を適用しない
- path prefix範囲外のページ由来promptも、
  `read` scopeを満たす場合は名前で一覧・取得できる
- 同じBearerを使用するpath基準MCP toolsには、
  従来どおりpath prefix制約を適用する

---

## 5. ページ状態と公開条件

| ページ状態 | `prompts/list` | `prompts/get` | 名前予約 |
|:--|:--|:--|:--|
| 通常ページ・latest | 公開 | 取得可能 | 維持 |
| draft | 非公開 | not found | 登録しない |
| soft delete | 非公開 | not found | 維持 |
| undelete後 | 条件を満たせば再公開 | 取得可能 | 維持 |
| hard delete後 | 非公開 | not found | 解放 |
| 過去revision | 非公開 | 直接取得不可 | latestを基準 |

追加の規則は以下とする。

- renameはprompt名と公開指定を変更しない
- 一覧候補に対応するページ索引がない場合は公開しない
- 名前索引の解決先ページが存在しない場合はnot foundとして扱う
- 名前索引と最新ソースのprimitiveまたはnameが一致しない場合は
  内部不整合として扱う
- 非公開状態の違いを外部エラーから判別可能にしない

---

## 6. `prompts/list`

### 6.1 基本動作

- 公開可能なpromptを`Prompt`の配列として返す
- 状態フィルタをソート、cursor、件数上限より先に適用する
- 候補がない場合は正常な空一覧を返す
- 同名候補など名前索引と矛盾する状態を黙って公開しない

### 6.2 ソート

- `Prompt.name`のcase-sensitiveな文字列比較で昇順に並べる
- Rustの`str::cmp`相当の順序を使用する
- 小文字化、locale依存照合、Unicode正規化を行わない

### 6.3 ページサイズ

- 1要求で返すpromptは最大50件とする
- 実装は最大51件を取得し、続きの有無を判定できる
- 続きがある場合は返却する50件目のprompt名を
  `nextCursor`とする
- 続きがない場合は`nextCursor`を設定しない

---

## 7. cursor

### 7.1 境界規則

- cursor未指定時は先頭から取得する
- cursor指定時は`name > cursor`のpromptを対象とする
- cursor自身を次ページへ含めない
- cursor名が実在することを要求しない
- 有効なcursorより後ろに候補がない場合は正常な空一覧を返す

### 7.2 値制約

cursorには`mcp.name`と同じ値制約を適用する。

- 空文字および空白だけの値を許容しない
- 先頭・末尾空白を許容しない
- 制御文字を許容しない
- 最大128 Unicode scalar valuesとする

### 7.3 snapshot

- cursorはDB snapshotまたは一覧世代を表さない
- 各要求時点の最新公開候補集合に対する名前境界として扱う
- 継続取得中の追加、削除、rename、prompt名変更により、
  結果の重複または欠落が生じないことは保証しない

---

## 8. `prompts/get`

### 8.1 名前解決

- `(prompt, name)`のprimitive共通名前索引からpage IDを解決する
- prompt候補テーブルの全件走査で名前解決しない
- 名前索引に該当項目がない場合はnot foundとする
- 解決後、最新ページ状態、latest revision、latest sourceを取得する
- 最新ソースのfront matterを共通parserで再検証する
- 最新ソースがpromptでない場合またはnameが一致しない場合は、
  別候補を探索せず内部不整合とする

### 8.2 本文

- front matter終端直後からページソース末尾までを本文とする
- raw Markdownを保持する
- 見出し、コード、wiki link、通常マクロ、空白、改行を保持する
- trimおよび改行正規化を行わない
- 通常マクロ、wiki link、`{{!macro}}`を展開しない

---

## 9. 引数・placeholder・message

### 9.1 要求引数

- 引数はJSON objectとして受け付ける
- JSON objectの性質上、同一キーの重複指定は保持されない
- `required = true`の引数が未指定の場合は拒否する
- front matterで宣言されていない引数を拒否する
- 引数値はJSON stringだけを許容する
- `null`、boolean、number、array、objectを拒否する
- optional引数の未指定値は空文字列とする
- 宣言済みだがsystemや本文で未使用の引数は許容する

### 9.2 placeholder

- `{{@name}}`を同名引数の文字列へ展開する
- 同じplaceholderが複数ある場合はすべて展開する
- `{{@@name}}`はリテラル`{{@name}}`へ戻す
- systemと本文へ個別に一回だけ展開を適用する
- 挿入値に含まれるplaceholder、通常マクロ、wiki linkを再展開しない
- Markdownコードおよびコードフェンス内も同じ規則で展開する
- `\{{@name}}`のバックスラッシュをescapeとして扱わず、
  バックスラッシュを保持したままplaceholderを展開する
- 引数名規則に一致しない`{{@...}}`は変更しない
- 閉じ`}}`がない記法は変更しない
- 有効な未宣言placeholderは内部不整合とする

### 9.3 message生成

`mcp.system`未指定時のmessage文字列は以下とする。

```text
<展開後本文>
```

`mcp.system`指定時のmessage文字列は以下とする。

```text
<展開後system>

<展開後本文>
```

追加の規則は以下とする。

- systemと本文をLF 2文字で連結する
- 1件のUser text messageとして返す
- system roleおよびAssistant roleを使用しない
- 複数messageへ分割しない
- systemと本文をtrimしない
- systemと本文の改行を正規化しない

---

## 10. protocol error

### 10.1 `prompts/list`

| 条件 | JSON-RPC code | `data.code` | message |
|:--|--:|:--|:--|
| scope不足 | `-32600` | `forbidden` | `operation is not allowed` |
| cursor不正 | `-32602` | `invalid_input` | `cursor is invalid` |
| DB失敗・候補不整合 | `-32603` | `internal_error` | `internal error` |

### 10.2 `prompts/get`

| 条件 | JSON-RPC code | `data.code` | message |
|:--|--:|:--|:--|
| scope不足 | `-32600` | `forbidden` | `operation is not allowed` |
| 不存在・非公開 | `-32602` | `not_found` | `prompt not found` |
| 要求引数不正 | `-32602` | `invalid_input` | 安全な入力エラー説明 |
| 正本・索引不整合 | `-32603` | `internal_error` | `internal error` |

要求引数不正の主なmessageは以下とする。

- 必須引数不足
  - `required prompt argument is missing: <name>`
- 未知引数
  - `unknown prompt argument: <name>`
- JSON string以外
  - `prompt argument must be a string: <name>`

要求引数不正のmessageには、問題のある引数名を含めてよい。
引数値は含めない。

### 10.3 エラー情報の秘匿

protocol errorへ以下を含めない。

- 引数値
- system
- 本文
- 展開後または部分生成済みmessage
- front matter全体
- ページpath
- page ID
- DB内部エラー
- ローカルファイルpath
- Bearer token平文
- Authorization header
- serialize済みrequest body

---

## 11. capabilityと通知

### 11.1 capability

- primitive名前索引の対応済み構築状態を確認できる場合だけ、
  prompts capabilityをinitialize応答へ含める
- 構築状態を確認できない場合は安全側へ倒し、
  prompts capabilityを公開しない
- prompts capabilityの状態にかかわらずtools capabilityを維持する
- 未実装のresources capabilityをpromptsと同時に先行公開しない

### 11.2 通知

- M3初期版では`prompts.listChanged`を宣言しない
- `notifications/prompts/list_changed`を送信しない
- prompt保存後同期、soft delete、hard delete、再構成から
  MCP sessionへ通知しない
- クライアントは最新一覧が必要な場合に`prompts/list`を再取得する

---

## 12. 監査

### 12.1 `list_prompts`

- 監査操作名は`list_prompts`とする
- 成功時は`target_path = null`、`revision = null`とする
- 成功summaryは`count=<件数> has_more=<true|false>`だけとする
- scope不足、cursor不正、内部不整合をそれぞれ
  `scope_denied`、`invalid_input`、`internal_error`として記録する
- 失敗summaryは固定公開messageだけとする
- cursor値、prompt名およびprompt一覧を記録しない

### 12.2 `get_prompt`

- 監査操作名は`get_prompt`とする
- 成功時は`target_path = null`、`revision = latest revision`とする
- 値制約を満たす要求prompt名だけを
  `name=<prompt名>`としてsummaryへ記録できる
- 不正な要求prompt名はsummaryへ記録しない
- 不存在、引数不正、scope不足、内部不整合をそれぞれ
  `not_found`、`invalid_input`、`scope_denied`、
  `internal_error`として記録する
- 失敗時は`target_path = null`、`revision = null`とする

### 12.3 共通項目と記録禁止情報

- user ID、token ID、取得可能な入力元IP address、timestampを
  既存の共通監査項目として記録する
- 監査書込みはbest-effortとし、
  書込み失敗によってprompts操作の結果を変更しない
- 以下を監査ログへ記録しない
  - prompt本文
  - system
  - 展開後message
  - 引数値
  - request body
  - front matter全体
  - ページpath
  - page ID
  - DB内部エラー
  - Bearer token平文
  - Authorization header

---

## 13. 関連文書

- `REQUIREMENTS.md`
  - MCP promptsの要求仕様
- `FRONT_MATTER_SPECS.md`
  - prompt front matterの構造、値制約、保存時処理
- `MCP_INTERNAL_DESIGN.md`
  - MCP共通内部設計
- `MCP_ARCHITECTURE_DESIGN.md`
  - promptsの層間接続
- `MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - prompt候補、名前索引、同期、再構成
- `MCP_RUNTIME_AND_TRANSPORT_DESIGN.md`
  - capability、transport、標準handler、通知
- `MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 内部モデル、MCP型変換、エラー設計
- `MCP_AUTHORIZATION_TEST_VIEWPOINTS.md`
  - promptsの認証・認可確認
- `MCP_AUDIT_LOG_DESIGN.md`
  - prompts監査設計
- `MACRO_SPECS.md`
  - prompt専用placeholderと既存マクロの境界
