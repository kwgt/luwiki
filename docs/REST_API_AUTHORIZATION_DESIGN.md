# REST API認可・共通パス設計

実装・結果の対応は[検証観点・証拠参照](REST_API_AUTHORIZATION_TEST_VIEWPOINTS.md)を参照する。

> 2026-10-11: 合意済み仕様・設計を反映。この時点では未実装・実動作未検証。
> 2026-10-15: MS2〜4で共通パス・認可・監査を実装し、MS5グループ2〜4の
> 必要検証を完了。実行条件・結果・制約は正式作業記録を参照する。
> MS5の最終SC評価・CP5は未実施。
> 要求の正本はREQUIREMENTS.md、公開応答はREST_API_SPECS.md、監査項目/summaryはMCP_AUDIT_LOG_DESIGN.md第11節。

## 適用状態とスコープ差

公開RESTの要求scopeはREST_API_SPECS.mdを正とする（read、create、update、delete、
lock取得/延長のcreateまたはupdate。writeは既存の上位互換）。MS1調査時の
一律Write指定とhelloのread欠落は、MS2/3で公開仕様の細分scopeへ修正済み。
write上位互換・ReadOnly優先・MCPの既存細分scopeを維持し、MS5の
認証・scope統合と関係回帰で確認した。
共有POSTではrename=update/restore=create、rollback=update/compaction=deleteを
正常分岐ごとに判定する。lock取得/延長のcreate OR updateと混同してhandler全体を
OR許可しない。分岐を解釈するための入力400と403の一般優先順位は新設しない。

## 設計範囲と責務

公開操作はREST_API_SPECS.md・openapi.yaml、32アクションの対応は
REST_API_AUTHORIZATION_TEST_VIEWPOINTS.mdを基準とする。本書は合意した
認可設計を記述する。実装・検証済みの範囲と制約はMS2〜5の正式記録を参照し、
本書の設計記述だけを実動作成功の証拠としない。汎用ACL基盤や新しいCLI管理機能を
増やさず、共通パス処理、対象DB操作の認可付き入口、REST認可応答に責務を分ける。

- crate内共通モジュール`src/wiki_path.rs`に、通常入力の検証・dot解決、
  保存値の正規形検証、パス境界付きprefix判定を置く。
  `NormalizedWikiPath`をcrate内の文字列newtypeとして扱い、入力解決済み値と
  rawを混同しない。モジュールはHTTP/MCP/DBのエラー型へ依存しない。
- `src/auth.rs`の認証文脈のscope意味・ReadOnly優先を維持し、prefixから操作対象pathを判定する最小の共有関数を使う。RESTのHTTP応答・MCPのprotocol error変換は各公開境界の責務。
- DB層へHTTP応答やリクエストを渡さない。認可付きDB入口にはprefix制約を保持する`PageAccessPolicy`を渡す。Basicおよび制約なし/ルートprefixのBearerはUnrestricted、その他は許可prefix集合。deny理由と実際の対象pathを型付きで返し、REST/MCPで既存応答へ変換する。
- 認可付き対象解決は`src/database/manager/authorized_pages.rs`・
  `authorized_writes.rs`・`authorized_assets.rs`・`locks.rs`に接続済み。
  `pages_write.rs`・`assets.rs`・`txn_helpers.rs`等の既存txn内処理を共有する。
  既存CLI等の非HTTP入口はUnrestricted wrapperを維持し、保存形式・token管理形式を
  変更しない。

## 3.1 入力形式とWikiパス処理

| 段階 | 確定する処理 |
|---|---|
| 入力形式の解釈 | REST queryは既存web::Query、route parameterは既存Actix extractor、MCP toolはJSON文字列を一度だけ解釈。共通Wiki関数内でpercent decodeしない。JSONの%2Fは文字列として保持 |
| raw検証 | 空/相対/バックスラッシュを拒否。連続//、Unicode空白（全角を含む）・制御文字を拒否。ルート/以外の末尾/を拒否。前後trimしない |
| dot解決 | /区切りstack、.は取り除き、..は一要素pop。空stackからのpopはルート越えで拒否。解決後stack空なら/。一般文字列の点は保持 |
| 最終検証 | 正規化済み絶対パス、root以外末尾/なし、連続/なし、dot segmentなし、禁止文字なしを確認しnewtypeを生成。認可とDBにはこの値を渡す |
| エラー変換 | REST400/MCP InvalidInput。scope/path認可失敗と区別。認証→公開仕様のscope/ReadOnly確認は公開層で維持し、400と403の新しい一般優先順位は導入しない |

Unicode空白・制御文字はRustの`char::is_whitespace`・`is_control`を基準とし、byte値だけのASCII検証にしない。可視UTF-8の判定を独自に拡張したり不可視format文字まで新たに禁止したりしない。解釈不能な既存エンコードの仕様は入力形式側の既存挙動を維持し、本文/ファイル名の仕様へWikiパス禁止文字を転用しない。

REST `rename_to`/`restore_to`には専用の入力解決入口を設ける。raw段階では単一末尾/だけ例外、その他の禁止は通常どおり。末尾/ありを別flagで保持し、配置先baseのdotを解決してから、同じwrite txnで取得したcurrent/last_deleted pathの最後の要素を付加し、最終検証する。raw /も末尾指定flagを保持し、配置先ルートに名前を付加して/basenameとする。結合時はroot区切りを一つにし、配置先解決でrootになった場合にも//basenameを生成しない。末尾/なしはそのまま最終配置先。MCP renameや他REST入力へこの例外を一般化しない。

MS2でMCPの従来の末尾除去/dot拒否を共通処理へ接続し、prefix/cursor/page/
asset owner pageも同じ入力規則を使用する。token prefixは正規化済み保存値として
比較し、requestごとにdotを解決して許可範囲を変えない。CLIの既存validatorと
ブラウザ表示の入口にも、各入力形式に対応する解釈を接続した。ブラウザUI内の
相対入力・preview補正は、REST/MCP受信rawの規則とは区別する。
新規則から外れる既存保存値を自動変換しない。具体的な既存不適合値が見つかった
場合は保全し、互換性の判断へ戻す。

## 3.2 対象状態・ID解決とsnapshot

| 対象 | 同一DB snapshot/txnでの解決と認可 |
|---|---|
| 作成先path | raw解決済み作成先を認可し、同じwrite txnで存在/衝突確認とdraft/lock作成 |
| path指定参照 | 解決済み要求pathをまず認可。index/deleted lookupも同じread txnで解決し、実対象pathと一致することを確認。prefix境界判定後の別表記追跡は行わない |
| ページID | indexを取得し、draftは保持path、通常はcurrent、deletedはlast_deleted pathを対象とする。ID不在ならscope等以外のpath403を作らず404。必要path欠損はDB内部不整合500 |
| 履歴rev | 現在/削除時pathで認可して同一snapshotのsource/revision/rename_infoを返す。履歴当時pathへの追加認可・link_refs加工なし |
| asset ID | asset info→page_id→page indexを同じtxnで解決。page_id=Noneはzombie410。page_idありでindexなし/必要path欠損は内部不整合500。deleted flagだけを先に返して所属page認可を省略しない |
| lock情報を伴う取得 | 対象index/pathを認可後、同じwrite txnで必要な期限切れcleanupとlock/meta返却値を生成。get_page_lock_infoの副作用を認可前に実行しない |

純粋参照用の認可付きDB入口はread txnを開き、そのsnapshotでindex、path、source、asset metadata等を取得して返却DTOを生成する。handlerがindexを読んだ後、別DBメソッドで最新source/pathを読み直す構成を残さない。asset bodyは認可済みasset IDに対応するファイルをread txn保持中に読み、metadata/ETag/返却bytesをまとめる。asset bytesのID単位不変という既存運用に従い、並行hard deleteによるファイル消失は既存FS失敗として扱う。snapshot後の別要求は再認可される。

DB txnとAppState/sinkの同期lockはasync awaitを跨いで保持しない。payloadは先に読み終え、DB入口の同期区間でtxnを閉じてからHTTP返却/監査を行う。対象状態が削除後の本文参照を許すか等は既存アクション仕様に従い、認可だけで新しい状態操作を許可しない。

## 3.3 一覧・検索・テンプレート・親・履歴

- 一覧は要求prefixを正規化・認可した後、同じread txnからprefix完全一致/配下の候補を取得する。cursorは形式検証・dot解決と前後同時指定拒否だけを行い、存在確認・独立認可・範囲内prefix制約を追加しない。既存cursor比較→sort→limit→has_more/anchorをprefix候補にだけ適用し、範囲外cursorでも範囲を拡張しない。MCP固有のcursor制約をRESTへ転用しない。
- 検索はDB read snapshotで通常/削除済みindexを走査し、with_deleted・draft除外とcurrent/last_deleted pathのpolicyを適用した許可page ID集合を得る。FTSは既存expression Queryと、indexed page_idフィールドのTermSetQueryをBooleanQueryのMustでANDする専用の制約付き検索入口を追加する。許可ID条件は検索expressionのscoreを変えないフィルタとしてANDする。TermSetQueryを0スコアにするBoostQuery等を用い、許可ID数やID条件の一致が公開scoreへ寄与しないようにする。具体APIの組合せが成立することと既存scoreの維持は実装時に検証する。ユーザquery文字列へIDやpathを連結しない。空集合は200[]でFTS検索を省略できる。
- 各targetのTopDocs100はこの型付き許可条件の後に実行する。既存の(page_id,revision)でdedup・複数target最大score採用を維持し、merge後もscore順100件へ制限する。同scoreの順序はpage ID/revisionで決定できるよう内部で安定化する。各targetの許可上位100をmergeすれば、最大scoreによる全体の上位100を選べる。許可前上位100からのfilterは行わない。
- 検索返却path/deleted/履歴可否は同じDB snapshotから解決する。FTSの古いpathやdeleted表示を認可の根拠にしない。FTSとredbを一つのtransactionにできるとは扱わず、FTSは候補・score・snippet、DB snapshotは対象認可・返却状態の正本とする。候補不整合は既存内部失敗分類に従い、非認可候補のID/name/snippetを公開しない。
- テンプレートは候補ページIDを同じread snapshotのcurrent pathへ解決してpolicy filter後にレスポンス情報を生成。候補なしは200[]。candidate indexだけを認可根拠にしない。
- 親は指定index/pathを認可し、同じread snapshotで通常のdirect/recursive探索を行う。得た親候補のpathを判定し、非認可なら親なしと同じ404/reason=parent not found。非認可親を飛ばして別の祖先を探す変更は行わない。監査内部resultはPathPrefixDenied、HTTPは例外404として区別する。
- source/meta履歴の過去path・変更日時・rename_info.link_refsはそのまま。そこに含むID/pathの別要求は対象ページを再認可する。本文文字列の秘匿やlink装飾は追加しない。

## 3.4 更新・再帰・同一対象保証

RESTだけのhandler lockは採らない。redb write txn内で最新対象を再解決し、認可した対象と更新対象を同じ値として扱う。MCP/CLI/lock cleanupの並行writerはredbのwrite serializationでこの区間へ割り込めない。既存AppState read lockをTOCTOU保証として扱わない。

認可付きDB入口では、txn開始→最新ID/index/asset owner解決→scope済みpolicyでpath認可（rename/restoreは同じindexから最終配置先を解決して両側認可）→既存状態/lock/衝突の検証→変更→commitの順にする。read時点の期待IDを携える操作はID固定とし、旧pathが別ページへ付け替わった場合に別IDを更新しない。任意の古い期待pathを使って最新indexを更新することも禁止。並行変更があった場合、最新txn対象の再解決・再認可により許可なら実行、非許可なら変更前に拒否する。認可失敗を理由に成功済み部分を残さない。

表中の「現行handler」はMS1設計時点の接続差分を指す。source PUTは現在、
認可付き同一txn入口へ接続済みであり、旧分離処理が残るという意味ではない。

| 更新種別 | 具体的な接続差分 |
|---|---|
| draft作成 | create_draft_page内txnで作成先認可、衝突判定→index/path/lock登録。CLI用入口はUnrestricted |
| source PUT | put_pageのtxn内へpage認可・lock/token/owner/amend判定・必要lock解放を統合。現行handlerの保存→別txn lock releaseを認可付きAPIでは分けない。返却結果に実path/committed revisionを保持 |
| rename/restore | ID指定の認可付き入口でcurrent/deleted元pathと最終配置先を同時確定。末尾記法をhandlerとDBで二重付加しない。source/asset復元やindex更新も同じtxn |
| recursive rename/restore/delete | 既存txn_helpersの起点一致またはbase+/配下収集を再利用。元/先両起点（deleteは元のみ）認可後、配下lock・衝突を全件検証し、relative suffixを維持したmappingを同一txnでcommit。個々の配下policy再判定は不要。元起点変更による別範囲操作を防ぐ |
| rollback/compaction | DB write txnのindex取得直後にcurrent path認可、状態/lock/rev確認→更新。FTS同期はcommit後に既存方式を維持 |
| lock POST/PUT/DELETE | index/path認可後にlock token/owner/期限検証・更新を同じtxnで実施。DELETEに伴うdraft/asset削除も同じ認可対象。GET/meta等cleanup付き処理も同じ順序 |
| asset upload | 既存create_assetのfile保存-before-txn（src/database/manager/assets.rs:334–346）を認可付き入口では使わない。write txn内で最新owner page認可・lock/重複/状態確認後に新規IDファイルを作成しmetadata登録/commit。commit/登録失敗時は新規ファイルを回収。拒否前にfile/directory作成しない |
| asset DELETE | asset/owner/index認可→zombie/deleted/lock条件→soft deleteを同一txnで実施。非削除zombieでも一律410で更新しない。page_idありの解決失敗は内部不整合 |

内部関数は既存DB処理のtxn内部分を最小限切り出し、認可付き入口から同じtxnを渡して呼ぶ。txn保持中に別のbegin_writeする既存publicメソッドを呼んでnested txnにしない。MCPの認可対象操作も共有入口を利用できるよう接続し、既存成功/失敗形式を維持する。Unrestricted CLIの更新は競合writerとしてwrite txn直列化の対象であり、HTTP policyを偽装させない。

認可後のlock cleanupで期限切れdraftがなくなった場合、以後の状態応答を既存動作に合わせる。scope/pathの拒否はcleanupより先なので当該拒否によるdraft/lock/asset変更はない。バックグラウンドの独立cleanupによる変更を当該拒否要求の変更と誤認しない。

DB commit後のFTS/FS整理失敗は従来の業務後処理失敗として500になり得る。これは「認可拒否時の無更新」と別であり、全DB/FTS/FSの分散atomic化を本ミッションに追加しない。新規assetファイルの回収失敗は内部診断へ残し、完成したuploadとして返さない。保証対象は同一DBに対しredbの書込直列化に従う経路に限定する。別プロセスによる複数Database openの可否は未確認であり、保証済みとは扱わない。asset fileを外部から直接書換える運用まで保証するものではない。

## 3.5 API表への認可・応答差分

| 検証表No. | 接続先と認可対象 | 返却/更新前の判定 |
|---|---|---|
| 1–2 | hello/meにpath guardなし、helloにRを追加 | 既存認証・R、監査対象 |
| 3 | 作成txn、最終作成path | draft/lock作成前 |
| 4–9 | read snapshot、要求prefix/path、候補current/deleted path | cursorは比較のみ、検索100は認可後、template空200 |
| 10,14–15,20,22 | read snapshot、ID対象index/path/asset owner | 本文/short/path/asset一覧/302/304前 |
| 12,25 | cleanup付きwrite txn、対象index/path | 認可前にget_page_lock_info副作用なし |
| 13 | read snapshot、指定ページ＋通常探索の親候補 | 指定非認可403、候補非認可404/共通reason |
| 11,18–19,23–24,26–27 | 認可付きwrite txn、draft/current/deleted対応 | lock/token/revision/変更前、403優先、拒否無更新 |
| 16–17 | 認可付きwrite txn、元＋特例解決後の配置先 | 両側認可、同値204も認可、再帰mapping一致 |
| 21,28 | upload write txn、指定path/最新owner | lock・新規file・metadata前 |
| 29–32 | read/write snapshot、指定pathまたはasset→owner | redirect/data/304/metadata/delete前、zombie410 |

成立条件間で403 > 410 > 404。scope/ReadOnlyは入口で確認、path入力が与える対象は解決後認可、ID対象は最新indexから認可対象が得られた時だけpath判定。ID不在でpath403を作らない。deletedでも本文/meta履歴が許されるアクションではdeleted410を新たに作らない。入力400・認証401・業務409/423/500の一般的な新優先順位は定めない。lockの403は当該アクションでlock認証条件が成立する場合に扱い、存在しないlockを仮定して410/404を上書きしない。親候補非認可404は合意した明示例外、zombieはscope等403がなければ410。

