# WebSocket通信仕様

## 共通接続と使用方針

- 同一originの`GET /ws`をWebSocketへUpgradeする。MCP無効時も提供する。
- 接続先を用途別に切り替えず、機能と操作は`type`で識別する。
- Basic認証またはBearerのreadを要求する。ReadOnlyも利用できる。
- ブラウザは既存の`/api/hello`で認証を確立してから接続する。
- Origin欠落・null・異なるoriginは拒否する。資格情報をURLに含めない。
- handshake時の認証に加え、状態送信前に不変主体と現在の権限を再確認する。
- 接続上限64、標準Ping周期15秒・Pong待ち10秒、送信待ち5秒とする。
- 切断中の全イベント再送・独自ACK・連番・配送順序制御は設けない。

## 共通メッセージ

UTF-8のJSON objectを一つのアプリケーションメッセージとする。
トップレベルは次の2項目のみとし、追加項目・型不正は拒否する。

| 項目 | 型 | 意味 |
|:--|:--|:--|
| type | 非空string | `page.subscribe`等の機能・操作の識別 |
| payload | object | typeごとのデータ。データなしは`{}` |

channel・必須version・汎用相関IDは設けない。将来の機能は同じ/wsへ
新しいtype・payload定義・handler・認可を追加する。本体更新後は画面を
再読み込みし、新旧通信形式の継続的な共存は保証しない。

## ページ変更通知

通常の`/wiki`閲覧が対象。一接続一閲覧対象、明示確認は同時一要求。
編集・履歴管理・一覧・検索画面は購読しない。

| 方向 | type | payloadの必須項目 |
|:--|:--|:--|
| client→server | page.subscribe | path:string、page_id:string、expected_state:string |
| client→server | page.check | なし。空object |
| server→client | page.state | kind、page_state、navigation、viewer_key |
| server→client | page.changed | kind、page_state、navigation、viewer_key |
| server→client | page.checked | kind、page_state、navigation、viewer_key |
| server→client | page.error | code:string、message:string |
| server→client | error | code:string、message:string |

pathは既存の絶対Wikiパス検証を適用する。page_idは表示中のページID、
expected_stateは実際に取得したsourceの`X-LuWiki-Page-State`を渡す。
そのヘッダは200・304で返し、本文・ETag・既存キャッシュ契約を維持する。

kindはunchanged・updated・replaced・renamed・unavailable。
page_stateは通常ページの場合だけid・revision・nullable instance_id・
state_tokenを持ち、それ以外はnull。navigationは認可した移動先または
最も近い通常先祖のkind/pathを持ち、なければnull。viewer_keyは認証した
不変主体の比較値であり、権限を与えない。本文は送信しない。

表示中IDが別pathへ移動していればrenamedを優先する。元pathの別IDは
replaced、draft・削除・未存在はunavailable。内部不整合や認可失敗を
unavailableへ変換しない。先祖の最初の通常候補が非認可なら案内を返さず
探索を終える。ハード削除後も入力pathから探索する。

初期照合はpage.state、変更はpage.changed、明示確認はpage.checked。
古い接続の結果を適用せず、切断中の確認は失敗にする。初期・再接続で現在
状態を照合し、短時間の中間イベント・変更回数・順序は保証しない。

```json
{"type":"page.check","payload":{}}
```

共通形式・未知typeはerror、機能の入力・認可・内部失敗はpage.errorで
返す。codeはinvalid_input・unsupported_type・forbidden・internal_error等
の固定値とし、資格情報や生の要求をmessageへ複写しない。

### payloadの型と拒否条件

- page.subscribeは表の3項目だけを持つobjectとする。pathは既存の絶対Wiki
  パス、page_idは既存ページID、expected_stateはv1形式の比較値として検証する。
  同一接続での二度目のsubscribeと、subscribe前のcheckはinvalid_inputとする。
- page.checkは空objectだけを受理し、余分な項目はinvalid_inputとする。
- kindがunchanged／updated／replacedならpage_stateはobject、navigationは
  nullとする。idはstring、revisionは1以上の整数、instance_idはstring又は
  null、state_tokenは`v1:`に64桁の小文字16進数、`:`、64桁の小文字16進数を
  続けた132文字のstringとする。
- kindがrenamed／unavailableならpage_stateはnullとする。navigationは
  null、又はkind（renamed／ancestor）とpath（絶対Wikiパス）のobjectとする。
  renamedの移動先はrenamed、unavailableの通常先祖はancestorを返す。
- viewer_keyは認証済み不変主体から生成するstring。同じ表示退避の復元先が
  同一主体かを比較するために使い、認証や認可の代わりには使用しない。
- 誤方向のpage.state／page.changed等もunsupported_typeとする。共通形式・
  payloadの入力不正と未対応typeの応答後は接続を継続できる。現在認可の拒否や
  snapshot内部失敗ではpage.errorを返して接続を終了する。
- Binary入力、frame不正、送信失敗、heartbeat期限、サーバ停止では接続を終了する。
  アプリケーションmessageの上限は既存のasset payload上限を共用する。
  独自ACK・再送は行わず、クライアントは再接続後の初期状態を照合する。

以下は正常な確認応答と固定エラーの例である。

```json
{"type":"page.checked","payload":{"kind":"unavailable","page_state":null,"navigation":{"kind":"ancestor","path":"/manual"},"viewer_key":"example-viewer"}}
{"type":"page.error","payload":{"code":"invalid_input","message":"invalid_input"}}
{"type":"error","payload":{"code":"unsupported_type","message":"unsupported_type"}}
```

### 通知時間の受け入れ条件

接続済み・可視・安定LANで、DB commitからpage.changed受信まで1秒以内、
受信から通常reload開始まで250ms以内を測定する。再接続、認証、本文取得、
描画の時間は分けて扱う。HTTP要求開始からの計測はcommitより前を含む上限
であり、上限の超過だけではcommit後の通知時間の超過と断定しない。

## 文書の責務

内部の接続・認可・commit通知・終了処理は[内部設計](WEBSOCKET_DESIGN.md)、
閲覧モード・位置維持はFRONTEND_UI_SPECS.md、本文取得はREST_API_SPECS.mdを参照する。
