# RESTパス認可・監査の検証計画

> 2026-10-11: 質問書revision31、グループ1/3/4各revision1、グループ2 revision2を基準とした検証計画。
> 2026-10-13: MS3/4で認可・監査を実装済み。本書は検証観点であり、
> 個々の成功証拠は各作業記録を参照する。
> 2026-10-15: MS5グループ2〜4で統合・関係回帰・実UIを確認済み。
> 本書の観点と実行結果を区別し、最終SC評価・CP5は別途行う。

要求はREQUIREMENTS.md、公開仕様はREST_API_SPECS.md/openapi.yaml、
内部認可はREST_API_AUTHORIZATION_DESIGN.md、監査の正本はMCP_AUDIT_LOG_DESIGN.md第11節。
既存MCP認可/監査/Bearerテスト観点と併用する。

## 1. 共通ケース・実行段階

| ID | ケースと判定 | 段階 |
|---|---|---|
| P1 | root、末尾/拒否、連続/拒否、空/相対/バックスラッシュ、半角/全角空白/control、前後trimなし | MS2単体 |
| P2 | dot解決、root越え拒否、REST一回decodeとJSON%保持、最終再検証・認可/DB同値 | MS2単体＋MS3経路 |
| P3 | /docs完全一致/配下と/docs2拒否、複数prefix、未設定、/、保存prefixを自動縮約しない | MS2単体＋MCP回帰 |
| C1 | read snapshotでpath/本文/metadata/ETag一致、MCP writerとの並行renameでも別対象へ置換しない | MS3競合 |
| C2 | write txn内最新ID/path再認可、再帰境界/相対配置、元/先拒否時本文/lock/asset/パスの当該変更なし | MS3競合 |
| C3 | lock lookupの期限切れcleanup・draft/asset削除前認可、独立保守の変更と当該要求の変更を区別 | MS3副作用 |
| C4 | upload file保存前認可、DB登録/commit失敗の新規file回収、FS失敗と監査best-effortを区別 | MS3/4障害 |
| A1 | Basic不変user_id/token null、Bearer actor/token、30固定operation/32action、raw URL/資格情報非記録 | MS4監査 |
| A2 | 各公開操作の成功/入力不正/deny/不在/業務失敗・payload413・next Err・302/304を一回投入。親HTTP404内部PathPrefixDenied | MS4監査 |
| A3 | commit後FTS等500でもphase/committed/確定path/rev、metadataなしでcommit状態を推測しない | MS4障害 |
| A4 | MCP無効init・定期retention・rotation・正常停止flush・flush失敗伝播・保留append保全 | MS4統合 |
| S1 | 公開仕様の細分scope単独成功、別scope拒否、lock create OR update、restore create、compaction delete、共有POSTの誤ったOR拒否、write上位互換、read非包含 | MS2/3＋MS5 |
| S2 | Basic、ReadOnly優先、NoBasicAuth、Bearer TTL/X-Bearer-Expire、MCP既存細分scope・append集約互換 | MS5回帰 |
| U1 | 作成/移動/復元/一覧/assetの通常UI、移動末尾/維持、直接URL/trim/dotの影響と既存値保全 | MS5 UI確認 |

拒否/成功だけを同じ入力比較でなぞるテストではなく、範囲外本文/ID/snippet/Location/304の
非流出と拒否前後DB/FS差分を確認する。競合はsleepの偶然に依存せず、txn開始/認可前後に
barrier等を用いるfixtureで再現する。FTSは許可結果が全体上位100件外となるデータを生成する。

## 2. 全30操作/32アクション対応

各R行にP1〜P3、適用するCケース、A1/A2、S1/S2を組み合わせる。
read-only/無制約Bearer/ルートprefix/Basicを比較し、必要scopeのみのtokenを使用する。
監査は成功と代表早期失敗を別要求で確認し、実装のresponse body再解析を前提にしない。

R表の既存テスト候補・不足の記載はMS1設計時点の調査であり、現在の未実施を
示すものではない。現在の全R対応と実行結果は末尾の証拠参照を用いる。

| ID | 操作・分岐 | 必要scope（writeは上位互換） | 固有確認 | MS1時点の候補・不足 |
|---|---|---|---|---|
| R01 | GET /api/hello | read | readなし403・制約Bearer成功・Basic監査 | tests/api_hello_auth.rs |
| R02 | GET /api/users/me | read | read・主体情報・pathなし監査 | tests/api_users_me.rs |
| R03 | POST /api/pages | create | create単独成功・作成先拒否でdraft/lockなし | source/read_onlyの作成helper・fixture |
| R04 | GET /api/pages | read | 上位prefix403・範囲外cursor/rewind・limit/anchor | tests/api_pages_list.rs |
| R05 | GET /api/pages/deleted | read | 削除時path認可・候補なし[] | deleted復元fixture;専用横断不足 |
| R06 | GET /api/pages/id | read | dot解決・draft404/deleted410とpath403順 | unit:pages/id.rs:157,245 |
| R07 | GET /api/pages/short | read | 正規化後のshort・不在/削除状態 | unit:pages/short.rs |
| R08 | GET /api/pages/template | read | 許可候補だけ・空200・名称/説明の非流出 | unit:pages/template.rs:128,192,221 |
| R09 | GET /api/pages/search | read | 全体100件外の許可結果・multi-target dedup/上限・score不変 | tests/api_pages_search.rs |
| R10 | GET /api/pages/{page_id}/source | read | latest/履歴/削除本文・304前認可 | tests/api_pages_source.rs:19,71,113 |
| R11 | PUT /api/pages/{page_id}/source | update | update単独成功・lock副作用前拒否・commit後500監査 | tests/api_pages_source.rs:199,592,798; read_only |
| R12 | GET /api/pages/{page_id}/meta | read | draft/履歴/削除・link_refs無加工・cleanup前拒否 | tests/api_pages_meta.rs; unit:pages/short.rs |
| R13 | GET /api/pages/{page_id}/parent | read | 親direct/recursive非認可404と不存在reason同一 | 専用の親認可テスト不足 |
| R14 | GET /api/pages/{page_id}/short | read | draft/current/deleted対応とshort返却前認可 | unit:pages/short.rs |
| R15 | GET /api/pages/{page_id}/path | read | path返却前認可・ID不在でpath403を捏造しない | read_only fixtureのpath取得;専用不足 |
| R16 | POST /api/pages/{page_id}/path action=rename | update | 元/先両側・raw /特例・再帰・競合・no-op認可 | DB recursive/rename既存; read_only;専用認可不足 |
| R17 | POST /api/pages/{page_id}/path action=restore | create | 削除元/先・raw /特例・再帰・assets復元・非削除409 | tests/api_pages_delete.rs:141; DB undelete |
| R18 | POST /api/pages/{page_id}/revision action=rollback | update | update単独・状態/rev/lockと認可順・FTS失敗補足 | tests/api_pages_revision.rs:86,197,260 |
| R19 | POST /api/pages/{page_id}/revision action=compaction | delete | delete単独・履歴除去前認可・指定rev監査 | tests/api_pages_revision.rs:142,197,260 |
| R20 | GET /api/pages/{page_id}/assets | read | owner path・配列絞込条件・user fallback回帰 | tests/api_pages_assets.rs:242,310 |
| R21 | POST /api/pages/{page_id}/assets/{file_name} | update | payload413一記録・owner拒否時fileなし・重複/lock | tests/api_pages_assets.rs:40,87; read_only |
| R22 | GET /api/pages/{page_id}/assets/{file_name} | read | 302 Location返却前認可 | tests/api_pages_assets.rs:407 |
| R23 | POST /api/pages/{page_id}/lock | create/update | createのみ/updateのみ成功・ReadOnly拒否 | tests/api_pages_lock.rs:25,86; read_only |
| R24 | PUT /api/pages/{page_id}/lock | create/update | createのみ/updateのみ成功・path認可がtoken検査より先 | tests/api_pages_lock.rs:123,163,201; read_only |
| R25 | GET /api/pages/{page_id}/lock | read | 期限切れdraft cleanup前拒否・lockなし404 | tests/api_pages_lock.rs:25,86 |
| R26 | DELETE /api/pages/{page_id}/lock | update | update単独・draft/asset削除副作用前拒否 | tests/api_pages_lock.rs:123,163; read_only |
| R27 | DELETE /api/pages/{page_id} | delete | delete単独・再帰境界/lock/拒否無更新 | tests/api_pages_delete.rs:38,86; read_only |
| R28 | POST /api/assets | update | path指定upload・認可後file作成・commit失敗回収 | tests/api_assets.rs:38,82; read_only |
| R29 | GET /api/assets | read | path指定302・削除/不在の成立順 | tests/api_assets.rs:231 |
| R30 | GET /api/assets/{asset_id}/data | read | asset ID→owner認可・304・zombie410・不整合500 | tests/api_assets.rs:231 |
| R31 | GET /api/assets/{asset_id}/meta | read | asset ID→owner認可・zombie410・metadata非流出 | tests/api_assets.rs |
| R32 | DELETE /api/assets/{asset_id} | delete | delete単独・非削除zombieでも410・owner/lock前認可 | tests/api_assets.rs:164; read_only |

## 3. 合意・成功条件との対応

| 質問書 | ケース | 受入条件 |
|---|---|---|
| 1.1 | R04/P2/P3 | prefix403、範囲外cursorでもprefix内items/has_more/anchor |
| 1.2 | R08/R09 | 許可後100、multi-target dedup/score維持、候補なし200[] |
| 1.3 | R13/A2 | 指定403、親候補404・reason同一、direct/recursive |
| 2.1 | R05/R10/R12/R14/R15/R25/R27/C1 | current/draft/deleted/history、過去path/link_refs維持・後続再認可 |
| 2.2 | R16/R17/C2/P2 | 両起点、特例最終配置先、再帰境界・競合・拒否無更新 |
| 2.3 | R30〜R32/C4 | zombie410、page_idありindex欠損は不整合 |
| 3.1 | P1/P2/R16/R17/U1 | raw禁止→dot解決→最終検証、raw /特例とMCP非一般化 |
| 3.2 | R06/R10/R13/R17/R30/C3 | 成立403>410>404、親例外、lock副作用前拒否 |
| 3.3 | A1〜A4/R01〜R32 | Basic/Bearer全公開出口・固定operation/summary・共通運用 |
| 3.4 | R01/R02/S1 | path除外、read、監査対象 |
| 追加合意scope | S1/S2/R03/R11/R16〜R19/R21/R23/R24/R26〜R28/R32 | 公開要求どおりの細分scope、ReadOnly優先、MCP既存条件維持 |

SC1はR01〜R32と30operation登録・OpenAPIを照合した対応表、SC10設計部分は要求→公開仕様→
内部設計→監査→本検証表の参照とCP1資料で判定する。SC2〜SC8はP/C/A/Rケース、
SC9はS/Uとcargo check・必要テスト、SC10最終判定はMS5の実装/文書整合と結果報告で行う。
MS1文書作業の完了をSC2〜SC9の実動作成功とみなさない。

## 4. 実行前提と結果の扱い

既存tests/common/mod.rsのテスト専用DB/assets/FTS、XDG隔離、localhost bind、
CARGO_BIN_EXE_luwikiまたは実ビルド出力を使う。実運用DB/秘密値をfixtureに使用しない。
一部API testsの独自TLS/redirect無効clientとテスト間Mutexの条件を維持する。
通常ServerGuardのDrop/killは正常停止flushの検証にならないので、ShutdownSignalによる
正常停止のfixtureを別途使う。cargo fmt一括は禁止、Rust変更後はcargo checkと該当テスト必須。
frontend変更が必要なら既存のTypeScript/Vue検証方法を使用する。

2026-10-11のMS1文書作成時はYAML構文・localref・全operation対応と
文書差分/文字コードの確認だけを行い、実動作の成功とは扱わなかった。
その後の実装・実検証は以下の正式MS5作業記録に対応する。

## 5. 実行証拠の参照

| 観点 | 実装・証拠の参照 |
|---|---|
| 全R32/30操作・scope/入力/対象/状態/監査 | [G1現行R32][ms5-r32]・[G2両認証32行/deny][ms5-g2] |
| P/C・S1/S2 | [G2認可/属性/TTL][ms5-g2]・[G3関係回帰][ms5-g3] |
| A1〜A3 | [G2実JSONL/出口/projection][ms5-g2]。物理障害と区別 |
| A4 | [MS4 G5同実装][ms4-g5]・[MS5 G3実正常停止][ms5-g3] |
| U1 | [G4実UI16群/修正後検証][ms5-g4]（Linux Chromium） |

実行条件・個別test名・hash・再利用理由・未検証範囲は正式記録を正とする。
32アクションは30操作に対応し、共有POSTの分岐を別operationへ増やさない。
実UI群数・wrapper再試行・fixture準備を成功test数へ加算しない。
この参照整合はSC最終認定・CP5通過やWindows/他browserの全面保証を意味しない。

[ms5-r32]: </ワークエリア/luwiki/セキュリティホール対策(1)/OP1-MS5タスクリスト/グループ1作業記録/SC受入表初版・R32対応>
[ms5-g2]: </ワークエリア/luwiki/セキュリティホール対策(1)/OP1-MS5タスクリスト/グループ2作業記録>
[ms5-g3]: </ワークエリア/luwiki/セキュリティホール対策(1)/OP1-MS5タスクリスト/グループ3作業記録>
[ms5-g4]: </ワークエリア/luwiki/セキュリティホール対策(1)/OP1-MS5タスクリスト/グループ4作業記録>
[ms4-g5]: </ワークエリア/luwiki/セキュリティホール対策(1)/OP1-MS4タスクリスト/グループ5作業記録>
