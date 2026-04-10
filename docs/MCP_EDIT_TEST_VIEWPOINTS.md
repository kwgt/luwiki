# MCP `edit_page` テスト観点一覧

本書は、MCPサーバ機能における `edit_page` の部分編集、内容整合性確認、
競合時の失敗分類、および操作種別拡張を見据えたテスト観点を整理した独立したテスト観点書である。

位置付けとしては、`docs/MCP_IMPLEMENTATION_DESIGN_TASKS.md` の 4.9 系に対応する成果物であり、
MCP 設計書群から参照される実装・テスト実装向け文書として扱う。

`edit_page` は単なる更新ではなく、
「単一 operation による部分編集」「`revision` と `instance_id` による内容整合性確認」
「`update_page` / `append_page` との責務分離」「将来的な operation 種別増加への追従」
を同時に満たす必要がある。
そのため本書では、入力モデル、operation 別意味論、整合性失敗、エラー写像、保存結果まで含めて観点を整理する。

## 1. 参照仕様

- `docs/MCP_TOOL_SPECS.md`
  - 2.7 `edit_page`
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 2.3.7 `edit_page`
  - 3.4.7 `edit_page`
  - 3.5.7 `edit_page`
  - 4.3 論理エラー区分への写像
  - 4.8 `not_latest_revision` と `instance_id_not_match` の位置付け
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - 2.7.4 read / update / edit / append / rename に対する適用範囲
  - 3.4.3 edit
  - 3.5 共通出力モデル
  - 3.6 サービス API の単位
- `docs/MCP_AUTHORIZATION_TEST_VIEWPOINTS.md`
  - 4.2 write系操作
  - 5.1 単一path操作

## 2. 観点の使い方

- 本書は `edit_page` の operation 解釈、整合性確認、保存結果、失敗分類を単体テスト、サービス層結合テスト、MCP 統合テストへ配分するための別紙である
- operation 種別は今後増える可能性があるため、共通観点と operation 個別観点を分けて維持する
- `update_page` や `append_page` の再テストを重複して列挙するのではなく、`edit_page` 固有の責務差分に集中する
- 監査ログ固有の詳細観点は本書の主対象にせず、必要に応じて監査ログ観点書側で扱う

## 3. 共通入力モデルの観点

### 3.1 基本入力

1. `path`、`revision`、`instance_id`、`operation` が必須であること
2. `operation` が単数受理であり、配列や複数指定を受け付けないこと
3. `path` 不正が `invalid_input` になること
4. `revision` の型不正や値不正が `invalid_input` になること
5. `instance_id` の型不正や空値扱いが `invalid_input` になること

### 3.2 operation 種別

1. 初期版で `replace_section`、`insert_section`、`delete_section`、`replace_text` を受け付けること
2. 未定義 `type` を含む operation が `invalid_input` になること
3. operation ごとの required 欠落が `invalid_input` になること
4. 将来追加予定の operation 種別を現時点で暗黙許可しないこと

## 4. セクション selector の観点

### 4.1 selector 形式

1. セクション指定が文字列指定または selector オブジェクト指定を受け付けること
2. 文字列単独指定が `by=title` の省略形として扱われること
3. selector オブジェクトで `by=id` と `by=title` を受け付けること
4. selector オブジェクトの `by` / `value` 欠落が `invalid_input` になること
5. 未定義 selector 方式が `invalid_input` になること

### 4.2 解決失敗

1. `by=id` 指定時に対象セクションが存在しない場合、失敗分類が仕様どおりになること
2. `by=title` 指定時に同名見出しが複数あり一意に解決できない場合、`invalid_input` になること
3. `by=title` 指定時に空文字または正規化後空文字が `invalid_input` になること

## 5. operation 個別観点

### 5.1 `replace_section`

1. 対象見出し行は保持し、本文部分のみを置き換えること
2. 子見出しを含むセクション範囲の扱いが仕様どおりであること
3. 存在しない対象セクションでは成功扱いにしないこと

### 5.2 `insert_section`

1. `placement=before` で anchor の直前へ挿入されること
2. `placement=after` で anchor の直後へ挿入されること
3. 挿入本文が見出し行を含む完全なセクションとして扱われること
4. anchor 解決失敗時に成功扱いにしないこと

### 5.3 `delete_section`

1. 対象セクションが削除されること
2. 対象範囲外の本文が不必要に削除されないこと
3. 存在しない対象セクションでは成功扱いにしないこと

### 5.4 `replace_text`

1. 一意一致時に対象文字列だけが置き換わること
2. `occurrence` 未指定時に `first` と同じ扱いになること
3. `occurrence=first` で先頭一致だけを置き換えること
4. `occurrence=all` で全一致箇所を置き換えること
5. 一致箇所が存在しない場合に成功扱いにしないこと
6. 複数一致時の扱いが仕様どおりであること

## 6. 内容整合性確認の観点

### 6.1 `revision` 一致確認

1. 入力 `revision` が最新 revision と一致する場合のみ編集が実行されること
2. 入力 `revision` が最新でない場合に `not_latest_revision` になること
3. `not_latest_revision` が `conflict` や `invalid_input` と混同されないこと

### 6.2 `instance_id` 一致確認

1. 入力 `instance_id` が最新内容の `instance_id` と一致する場合のみ編集が実行されること
2. 入力 `instance_id` が一致しない場合に `instance_id_not_match` になること
3. `instance_id_not_match` が `conflict` や `invalid_input` と混同されないこと

### 6.3 判定順序

1. current path 解決後に最新 revision と最新本文が読み取られること
2. `revision` / `instance_id` 整合確認失敗時に保存処理へ進まないこと
3. 整合確認失敗時に部分保存や中途半端な書き換えが発生しないこと

## 7. 競合・失敗分類の観点

### 7.1 競合系

1. 対象ページがロック中の場合に `conflict` になること
2. ロック競合が `not_latest_revision` や `instance_id_not_match` と混同されないこと

### 7.2 競合以外の失敗との分離

1. `invalid_input` が `not_latest_revision` と混同されないこと
2. `invalid_input` が `instance_id_not_match` と混同されないこと
3. `not_found` が内容整合性エラーと混同されないこと
4. DB 異常が `internal_error` として区別されること

## 8. 保存結果の観点

### 8.1 成功時応答

1. 成功時に `path`、`revision`、`instance_id`、`summary` を返すこと
2. 編集後の `instance_id` が返ること
3. 結果 `path` が current path 基準で返ること

### 8.2 保存経路

1. operation 適用後の全文が update 系保存経路へ橋渡しされること
2. 保存後に本文、latest revision、`instance_id` の整合が保たれること
3. `edit_page` が append 専用の amend 挙動を暗黙に持ち込まないこと

## 9. 公開面との接続観点

### 9.1 ツール境界

1. 全文置換を必要とするケースでは `edit_page` ではなく `update_page` を使う設計が維持されること
2. 末尾単純追記を必要とするケースでは `edit_page` ではなく `append_page` を使う設計が維持されること
3. `edit_page` が `update` スコープで扱われ、`append` スコープでは許可されないこと

### 9.2 将来拡張

1. operation 種別が増えても共通入力モデル観点と operation 個別観点を分離して追加できること
2. 新 operation の追加時に既存 operation の意味論が崩れていないことを回帰確認できる構成になっていること

## 10. 実施レイヤ分割の目安

- 単体テスト向き
  - operation 入力バリデーション
  - selector 解決失敗の分類
  - `revision` / `instance_id` 整合確認
  - operation ごとの本文変換
- サービス層結合テスト向き
  - current path 解決から整合確認、保存までの一連フロー
  - ロック競合時の `conflict` 写像
  - 保存結果 `revision` / `instance_id` の返却
- MCP 統合テスト向き
  - `edit_page` の応答形
  - `not_latest_revision` / `instance_id_not_match` / `invalid_input` / `conflict` / `not_found` の写像
  - `update_page` / `append_page` との責務境界

## 11. 整理結果

- `edit_page` 固有の責務である operation 解釈、部分編集、内容整合性確認を独立観点として整理した
- `not_latest_revision` と `instance_id_not_match` を公開エラーとして確認する観点を整理した
- operation 増加を見据え、共通観点と operation 個別観点を分離した構成にした
