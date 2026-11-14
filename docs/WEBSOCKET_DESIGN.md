# WebSocket内部設計

## 責務

共通/wsは認証・Origin・type/payloadの検証・type振分け・標準Ping/Pong・
資源管理・監査・停止を担当する。page.* handlerは購読対象の検証・認可・
状態取得・導線を担当する。動的pluginや汎用多重化基盤は追加しない。
wire契約は[WebSocket仕様](WEBSOCKET_SPECS.md)を正本とする。

## 保存から通知まで

DatabaseManagerの関連write txnがcommit成功した直後に、プロセス内共有の
tokio watchを更新する。印に本文やpathを入れず、対象ごとの短いread
snapshotを起こすために使う。保存失敗では起床せず、commit後の派生同期
失敗は保存を巻き戻さないため起床させる。無receiver時もDB成功を維持する。

通常保存・amend・append・作成・削除・rename・復元・再帰・rollback・
compaction・import・migrate削除・page状態を変えるcleanupと認証情報変更を
接続対象とする。状態不変なら照合後の送信を省略する。CLIの同時DB openは
redbの既存排他に従い、サーバ停止中の変更は再起動後の初期照合で検出する。

## 初期照合・再接続

receiver登録→既読印更新→認可と状態snapshot→初期応答の順とする。
取得・送信中のcommitは次のchangedで再照合する。同一接続の取得と送信を
直列化し、遅い別taskから古い状態を返さない。再接続は表示済みsourceの
比較値を基準にする。旧sourceのinstance_id欠落は本文fallbackで比較し、
旧データを書き換えない。OP1のtoken入力・domainと公開契約を維持する。

## 現在認可

Basicは資格情報検証と同じsnapshotから不変actorとcredential識別値を
取得し、passwordを保持しない。Bearerは不変actor/tokenの現在有効性・
scope・prefixを確認する。各状態送信前のsnapshotで入力path、表示中IDの
current/last_deleted path、返す移動先・先祖を認可する。待機中の再確認で
Bearer TTLを延長しない。接続時の通常認証と混同しない。

## 終了・監査

待機中はDB txn・AppState guardを解放する。停止signalを待機のselectへ
含め、gate→close/取消→監査確定→lease解放→要求数0→最終flushを守る。
接続ライフサイクルはGET /ws、明示確認はws.page.checkで公開要求単位に
記録し、自発通知・Pingごとに要求監査を増やさない。認証失敗はHTTPログ。
payload・token・credentialは監査へ記録しない。

## フロントエンド

共通接続処理とページ機能を分離する。自動/半自動/無視、非表示・離脱で
必要な購読を終了し、可視復帰・再接続時は現在状態を照合する。通常更新は
位置保存→window.location.reload、削除/renameは表示を保持して案内する。
通知reloadには一回分の表示退避とURL一時印を使い、直前削除で編集へ自動
転送されることを防ぐ。復元は再認証・同viewer・同tab/pathでだけ行う。
退避不能時はreloadを中止する。通常の未存在閲覧URLの挙動は維持する。

位置は内容→見出し→保存量。document/articleの両scrollを扱い、同一IDへ
だけ適用する。DOM・Mermaid・画像の描画後に補正し、500ms安定または5秒で
終える。利用者操作で補正を止め、observer/listener/timerを解放する。
