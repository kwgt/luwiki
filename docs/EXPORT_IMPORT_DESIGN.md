# エクスポート／インポート機能 実装設計

本書は、バックアップおよびページツリー単位のマイグレートを目的とした
エクスポート／インポート機能の実装設計を定義する。
要求仕様、CLI仕様、エクスポートデータ仕様、および現在の実装構造を踏まえ、
Rust 実装へ直接落とし込める粒度で責務分割と処理手順を整理する。

## 1. 参照資料

- `docs/REQUIREMENTS.md`
- `docs/BASE_DESIGN.md`
- `docs/CLI_SPECS.md`
- `docs/EXPORT_DATA_SPEC.md`
- `docs/EXPORT_IMPORT_DESIGN_VIEWPOINTS.md`
- `docs/PROJECT_CONSTRAINTS.md`

## 2. 現在実装から見た前提

### 2.1 既存データモデル

既存実装では、エクスポート対象となる主要データは以下の通りである。

1. ページインデックス
   - `src/database/types.rs` の `PageIndex`
   - 通常ページは `PageInfo`、ドラフトは `DraftInfo`
2. ページ履歴
   - `src/database/types.rs` の `PageSource`
   - rename 情報は `RenameInfo`
3. ユーザ
   - `src/database/types.rs` の `UserInfo`
   - ユーザ属性は `NoBasicAuth` や `ReadOnly` を含めて保持対象とする
4. アセット
   - `src/database/types.rs` の `AssetInfo`
   - 実体ファイルは `DatabaseManager::asset_file_path()` 配下
5. ロック
   - `src/database/types.rs` の `LockInfo`
   - 仕様上エクスポート対象外

### 2.2 既存 DB API の活用可能範囲

既存の `DatabaseManager` には、設計対象に対して以下の活用余地がある。

1. ページ列挙
   - `list_page_entries_by_prefix()`
   - `list_page_index_entries()`
   - `list_page_source_entries_by_id()`
   - `get_page_id_by_path()`
2. ユーザ列挙
   - `list_users()`
   - `get_user_id_by_name()`
   - `get_user_name_by_id()`
3. アセット列挙・取得
   - `list_page_assets()`
   - `get_asset_info_by_id()`
   - `read_asset_data()`
4. 再帰削除
   - `delete_pages_recursive_by_id()`
   - `delete_draft_by_id()`
5. ロック削除
   - `delete_page_lock_by_id()`

ただし、エクスポート／インポートでは以下が不足している。

1. 1トランザクション内での「列挙のみ」API
2. エクスポート済みモデルを DB へ直接投入する低レベル API
3. エクスポート ZIP 生成・読取の共通部品
4. マイグレート用リンク検証・書換え API
5. `export` / `import` の CLI 引数定義とコマンドディスパッチ

### 2.3 現在の制約

1. redb の `WriteTransaction` を使った原子的更新が基本方針である
2. 既存コードは `src/command/` と `src/cmd_args/` で CLI を構成している
3. 内部リンク解決は `src/database/link_refs.rs` の字句走査で実装されている
4. `migrate` はサーバ停止状態の管理操作として実行する前提である
5. Windows では `cargo` 検証手順に固有制約があるため、設計書では実装詳細に留める

## 3. 設計方針

### 3.1 方針

1. `backup` と `migrate` は同一の中核モデルを共有し、差分はポリシーで吸収する
2. 「列挙」「シリアライズ」「ZIP I/O」「検証」「DB反映」を分離する
3. `dry-run` は本実行と同じ検証ロジックを使い、副作用だけを抑止する
4. `migrate export` は「ZIP 生成成功」と「移送元削除完了」を同一成功条件とする
5. `import` は検証完了後にのみ DB 反映を開始する
6. 永続化済み `PageSource` との後方互換は `PageSource` 単位で吸収する
7. ユーザ属性は export / import で欠落させず、そのまま維持する

### 3.2 非方針

1. REST API での export/import 提供は本設計の対象外とする
2. rename 情報を用いたマイグレート後リンク再解決は行わない
3. `page_id` 直リンクの再配置支援は行わない
4. ロック情報の移送や再構築は行わない
5. `RenameInfo` 単体の版分岐で既存データ互換を吸収しない

## 4. モジュール設計

### 4.1 追加モジュール構成

以下の追加を想定する。

1. `src/export_import/mod.rs`
   - 公開入口
2. `src/export_import/model.rs`
   - manifest / JSONL 行モデル
3. `src/export_import/policy.rs`
   - `backup` / `migrate` の差分吸収
4. `src/export_import/export_collect.rs`
   - DB からの対象収集
5. `src/export_import/archive_write.rs`
   - ZIP 出力と暗号化
6. `src/export_import/archive_read.rs`
   - ZIP 読取と復号
7. `src/export_import/validate.rs`
   - 形式・件数・参照整合性・配置整合性の検証
8. `src/export_import/link_plan.rs`
   - マイグレート時のリンク検証・書換え計画
9. `src/export_import/import_apply.rs`
   - DB 反映
10. `src/export_import/errors.rs`
   - 利用者向けエラー種別

CLI 側には以下を追加する。

1. `src/command/export.rs`
2. `src/command/import.rs`
3. `src/cmd_args/mod.rs` への引数定義追加
4. `src/command/mod.rs` への公開追加

### 4.2 役割分担

#### export_collect

責務:

1. ページ集合の確定
2. リビジョン集合の確定
3. ユーザ集合の確定
4. アセット集合の確定
5. エクスポート用中間モデル生成

#### archive_write / archive_read

責務:

1. ZIP エントリ名の統一
2. JSON / JSONL シリアライズ
3. ストリーム入出力
4. 暗号化・復号の実装差吸収

#### validate

責務:

1. 構文検証
2. 件数検証
3. ID 重複検証
4. 参照整合性検証
5. 配置衝突検証
6. `username` 重複検証
7. マイグレート固有リンク検証

#### import_apply

責務:

1. 反映順序制御
2. `dry-run` との副作用切替
3. アセットファイル配置
4. DB トランザクションとファイル書込の調停

## 5. 内部データモデル

### 5.1 中間モデル

実装では ZIP 入出力の直前に以下の中間モデルへ束ねる。

```rust
struct ExportBundle {
    manifest: ExportManifest,
    users: Vec<ExportUser>,
    pages: Vec<ExportPage>,
    revisions: Vec<ExportRevision>,
    assets: Vec<ExportAsset>,
    asset_blobs: Vec<ExportAssetBlob>,
}
```

`ExportAssetBlob` は ZIP 書込専用であり、JSONL には含めない。

### 5.2 PageSource 互換方針

`PageSource` は DB 永続化型であり、既存データとの互換性は `RenameInfo` 単体ではなく
`PageSource` 単位で吸収する。

実装では以下の構成を想定する。

1. 現行保存済み形式を `page_source_v1::PageSourceV1` として定義する
2. 新形式の `PageSource` を別定義する
3. `Deserialize for PageSource` を手書きし、次の順で読込を行う
   - 新形式 `PageSource`
   - 旧形式 `page_source_v1::PageSourceV1`
4. 旧形式から新形式への変換時は、旧 `rename: Option<RenameInfoV1>` を
   新 `rename: RenameInfo` へ写像する

この方式により、既存 redb 内の MessagePack データを壊さずに新形式へ移行できる。

### 5.3 新 `PageSource` / `RenameInfo` 形状

新形式では `PageSource.rename` を `Option<RenameInfo>` ではなく
`RenameInfo` そのものに変更する。

```rust
struct PageSource {
    revision: u64,
    instance_id: Option<Id>,
    timestamp: DateTime<Local>,
    user: UserId,
    rename: RenameInfo,
    source: String,
}

enum RenameInfo {
    None,
    Active {
        from: Option<String>,
        to: String,
        link_refs: BTreeMap<String, Option<Id>>,
    },
    RemovedByMigrate,
}
```

意図:

1. rename 無しを `RenameInfo::None` として型で表現する
2. 通常の rename を `RenameInfo::Active` で表現する
3. マイグレートで意味を失った rename を `RenameInfo::RemovedByMigrate` で表現する

旧形式からの変換規則:

1. `rename = None` は `RenameInfo::None`
2. `rename = Some(old)` は `RenameInfo::Active { ... }`

### 5.4 manifest

`manifest.json` は `docs/EXPORT_DATA_SPEC.md` に従う。
実装では次の補助情報をメモリ上だけで持つ。

```rust
struct ManifestContext {
    export_type: ExportType,
    export_root: String,
    relocate_prefix: Option<String>,
}
```

`relocate_prefix` は import 時のみ利用し、ZIP 内には格納しない。

### 5.5 import 時の作業モデル

検証後は以下の作業用モデルに変換する。

```rust
struct ImportPlan {
    manifest: ExportManifest,
    users: Vec<ResolvedUserImport>,
    pages: Vec<ResolvedPageImport>,
    revisions: Vec<ResolvedRevisionImport>,
    assets: Vec<ResolvedAssetImport>,
    link_actions: Vec<LinkRewriteAction>,
    warnings: Vec<ImportWarning>,
}
```

目的は次の2点である。

1. 検証済みの確定値のみを DB 反映フェーズへ渡す
2. `--fix-broken-link` の書換え内容を反映前に固定する

## 6. エクスポート設計

### 6.1 共通フロー

1. CLI 引数解釈
2. 実行モード決定
   - `backup`
   - `migrate`
3. 対象ページ集合の確定
4. 関連リビジョン収集
5. 関連ユーザ収集
6. 関連アセット収集
7. 中間モデル組立
8. `dry-run` なら検証結果のみ出力して終了
9. ZIP 出力
10. `migrate` の場合のみ移送元削除

### 6.2 backup export

#### 対象選定

1. ルート `"/"` 配下の通常ページを全件対象とする
2. 削除済みページは含めない
3. ドラフトは含めない
4. 削除済みアセットは含めない
5. ゾンビアセットは含めない

#### path 出力

1. `manifest.export_root = "/"`
2. `pages.jsonl.path` は `"/"` 基準の相対パス
3. ルートページ自身は `path = ""` とする
   - import 時に `normalize_path(prefix + "/")` で `"/"` へ戻せるため

#### rename 出力

1. `rename_revisions` は保持する
2. `revision.rename` は `RenameInfo::Active` を保持する
3. `rename.from` / `rename.to` は絶対パスのまま出力する

### 6.3 migrate export

#### 対象選定

1. `--subtree <PREFIX>` の絶対正規化パスを基点とする
2. `"/"` 指定は拒否する
3. 通常ページのみエクスポート対象とする
4. 削除済みページは含めない
5. ドラフトはエクスポート対象外とする
6. ただし移送元削除対象には、同一サブツリー配下のドラフトを含める

#### path 出力

1. `manifest.export_root = <PREFIX>`
2. `pages.jsonl.path` は `<PREFIX>` 基準の相対パス
3. 起点ページ自身は `path = ""`

#### rename 出力

1. `rename_revisions` は出力しない
2. rename リビジョン自体は削除しない
3. `revision.rename` は `RenameInfo::RemovedByMigrate` として出力する
4. `from` / `to` / `link_refs` は出力しない

理由:

1. rename リビジョン自体を削除するとリビジョン番号の連続性を壊すため
2. ページ履歴上、rename が存在した事実だけは保持した方が説明可能性が高いため

### 6.4 収集実装

既存 API を直接使い回すのではなく、エクスポート専用の DB 読取 API を追加する。
理由は次の通りである。

1. 収集対象を 1 つの read/write transaction で整合させたい
2. 既存の `list_*` は表示用途であり、必要情報が不足する
3. `migrate export` では列挙と削除対象確定を同一境界で扱いたい

追加候補:

1. `collect_export_pages_in_txn(base_path, mode)`
2. `collect_export_revisions_in_txn(page_ids, mode)`
3. `collect_export_users_in_txn(user_ids)`
4. `collect_export_assets_in_txn(page_ids)`
5. `collect_draft_page_ids_in_txn(base_path)`

## 7. ZIP 出力設計

### 7.1 エントリ構成

ZIP 内のエントリ名は固定する。

1. `manifest.json`
2. `users.jsonl`
3. `pages.jsonl`
4. `revisions.jsonl`
5. `assets.jsonl`
6. `assets/<asset_id>`

### 7.2 出力順序

1. `manifest.json`
2. `users.jsonl`
3. `pages.jsonl`
4. `revisions.jsonl`
5. `assets.jsonl`
6. `assets/<asset_id>`

出力順序を固定する理由:

1. デバッグ容易性
2. ストリーム読取実装の単純化
3. テスト容易性

### 7.3 ZIP 書込戦略

`migrate export` の原子性要件を満たすため、最終出力先へ直接書かず一時ファイルを使う。

1. `<OUTPUT>` が通常ファイルの場合
   - 同一ディレクトリに一時ファイルを作成
   - ZIP 完成後に `rename` で置換
2. `<OUTPUT> = "-"` の場合
   - 一時ファイルに ZIP を完成させた後に標準出力へ転送
   - 標準出力転送失敗時は削除フェーズへ進まない

### 7.4 暗号化

本設計では ZIP 暗号化を抽象化し、アーカイブ層へ閉じ込める。

```rust
trait ZipCipher {
    fn writer(...);
    fn reader(...);
    fn method_name(&self) -> &'static str;
}
```

実装方針:

1. AES-256 を優先する
2. 利用不能なら Standard ZIP 2.0 へフォールバックする
3. フォールバック時は警告を標準エラーへ出す
4. 暗号方式は manifest へ保存しない

## 8. インポート設計

### 8.1 共通フロー

1. CLI 引数解釈
2. ZIP 読取開始
3. 復号
4. 必須エントリ確認
5. JSON / JSONL パース
6. 形式検証
7. 参照整合性検証
8. 配置・ユーザ・リンク検証
9. `ImportPlan` 構築
10. `--user-list` ならユーザ一覧を出して終了
11. `dry-run` なら検証結果を出して終了
12. DB 反映

### 8.2 backup import

1. `manifest.export_type == "backup"` であること
2. `--migrate` 指定時はエラー
3. 既存 DB が存在する場合はエラー
4. 復元先プレフィクスは常に `"/"`
5. rename 情報は保持して反映する
6. 旧形式 `PageSourceV1` を読み込んだ場合も、内部では新 `PageSource` に正規化して扱う

### 8.3 migrate import

1. `manifest.export_type == "migrate"` であること
2. `--migrate <PREFIX>` が必須
3. `<PREFIX>` は絶対パスへ正規化する
4. 最終配置は `normalize_path(PREFIX + "/" + rel_path)` で決定する
5. rename 情報が含まれていた場合
   - 通常モードでは警告し、`RenameInfo::RemovedByMigrate` へ正規化して継続する
   - `strict-mode` ではエラー
6. `pages.rename_revisions` が混入していた場合
   - 通常モードでは警告し、空配列として扱う
   - `strict-mode` ではエラー

ここでのポイントは、rename リビジョン自体は削除しないことである。
`migrate import` では、rename メタデータの意味だけを落とし、
履歴番号・タイムスタンプ・編集者情報・本文は保持する。

### 8.4 短縮URL安定性

短縮URLは `page_id` を基準に導出されるため、export / import においても `page_id` を維持することが安定性の前提となる。
本設計では `backup import` / `migrate import` のいずれにおいても、ページの識別子は再採番せず、エクスポートデータに含まれる `page_id` をそのまま反映する。

このため、短縮URLの復元に追加の保存項目や変換テーブルは不要である。
インポート後の環境では、保持された `page_id` から短縮用パス断片を再導出することで、
短縮URLを再構成できる。

rename / move は current path を変化させても `page_id` を変更しないため、
既存の短縮URLは同一ページを指し続ける。
また、subtree migrate においても import 後に `page_id` が維持される限り、
短縮URLは移送先環境で再構成可能である。

## 9. 検証設計

### 9.1 検証段階

検証は以下の順で行う。

1. 形式検証
2. 件数検証
3. ID 検証
4. 参照整合性検証
5. 配置検証
6. ユーザ検証
7. リンク検証

前段で失敗した場合は後段へ進まない。

### 9.2 形式検証

検証項目:

1. 必須エントリの存在
2. JSON / JSONL のパース可否
3. `manifest.version == 1`
4. `manifest.export_type` と CLI 指定の整合
5. `pages.path` が相対・正規化済みであること
6. `backup` データでは `rename.from` / `rename.to` が絶対・正規化済みであること
7. `migrate` データで `rename` が混入した場合は通常モードでは警告対象、
   `strict-mode` ではエラー対象とする

### 9.3 件数検証

1. `manifest.page_count == pages.jsonl の行数`
2. `manifest.revision_count == revisions.jsonl の行数`
3. `manifest.asset_count == assets.jsonl の行数`
4. `assets.jsonl 件数 == assets/ 実ファイル数`

### 9.4 ID 検証

1. エクスポートデータ内の `page_id` 重複
2. エクスポートデータ内の `asset_id` 重複
3. エクスポートデータ内の `user_id` 重複
4. インポート先既存データとの同種 ID 重複

### 9.5 参照整合性検証

1. `revisions.page -> pages.id`
2. `revisions.user -> users.id`
3. `assets.page -> pages.id`
4. `assets.user -> users.id`
5. `assets/<asset_id>` 実体存在
6. `assets.size == 実ファイルサイズ`

### 9.6 配置検証

マイグレート時は特に以下を確認する。

1. 最終配置パスの重複
2. 既存ページとの衝突
3. 移送先に子ページを持つ既存パスの存在
4. ルート `"/"` への不正再配置の有無

「移送先に子ページを持つ既存パスの存在」とは、例えば `/dst` 配下へ投入する際に
既存 `/dst/a` が存在しつつ `/dst` 自体が対象外であるようなケースを指す。
この条件は再帰操作の説明可能性を損ねるためエラーとする。

### 9.7 `username` 検証

1. `--user-map` 適用後の最終ユーザ集合を求める
2. 既存 DB 内ユーザ名と照合する
3. インポート後に `username` が重複するなら常にエラー

補助ルール:

1. `--user-map` で既存ユーザへ寄せたリビジョンは、その既存 `user_id` へ置換する
2. 未マップのユーザは追加対象とする
3. `backup import` でも同じ検証を行う

### 9.8 `PageSource` 互換検証

1. 新形式 `PageSource` と旧形式 `PageSourceV1` の双方を受理できること
2. 旧形式の `rename = None` を `RenameInfo::None` へ変換できること
3. 旧形式の `rename = Some(...)` を `RenameInfo::Active` へ変換できること
4. `migrate import` 時に `RenameInfo::Active` が混入した場合、
   通常モードでは `RemovedByMigrate` へ変換されること

## 10. リンク設計

### 10.1 背景

現在の内部リンク解決は、保存時に Markdown 中のリンク文字列を
`src/database/link_refs.rs` で正規化・解決している。
ただし export/import データでは Markdown ソースそのものを保持するため、
マイグレート時には再配置後のソースを検証し直す必要がある。

### 10.2 リンク種別

マイグレート時に対象とするのは次のリンクである。

1. 相対ページリンク
2. 絶対ページリンク
3. アセット相対リンク
4. アセット絶対リンク

対象外:

1. `page_id` 直リンク
2. 外部 URL
3. アンカーリンク
4. 画像リンクのうち Wiki ページ解決を伴わないもの

### 10.3 検証結果の分類

各リンクは以下へ分類する。

1. `Resolvable`
   - 再配置後も解決可能
2. `BrokenByMigration`
   - ツリー外参照や絶対パス参照により未解決化
3. `Ignored`
   - 対象外リンク

### 10.4 strict-mode

`migrate` では以下を少なくともエラー対象にする。

1. ツリー外ページリンク
2. 絶対パスページリンク

通常モードでは warning として記録し、未解決リンクとして扱う。

### 10.5 `--fix-broken-link`

1. `BrokenByMigration` に分類されたページリンクを `about:invalid` へ置換する
2. 書換えは import 前に実施し、書換え後ソースを DB へ保存する
3. `dry-run` では書換え件数のみ報告する

置換ロジックは `link_plan.rs` に閉じ込め、元の解析器と同じ字句走査方式を採る。

### 10.6 rename 正規化フェーズ

`migrate import` ではリンク解析前に rename 正規化フェーズを挿入する。

処理内容:

1. `pages.rename_revisions` を空配列へ正規化する
2. 各 `PageSource.rename` を次の規則で正規化する
   - `RenameInfo::None` はそのまま
   - `RenameInfo::Active { .. }` は `RenameInfo::RemovedByMigrate` へ変換
   - `RenameInfo::RemovedByMigrate` はそのまま
3. `strict-mode` では上記変換を行わずエラーにする

このフェーズは「rename 情報の削除」ではなく、
「rename の意味を失効状態へ変換する」フェーズとして扱う。

## 11. DB 反映設計

### 11.1 反映順序

検証済み `ImportPlan` を以下の順に投入する。

1. ユーザ
2. ページ
3. リビジョン
4. アセットメタデータ
5. アセット実体ファイル

### 11.2 理由

1. `PageSource.user` がユーザ存在を前提とするため
2. アセットが `page_id` と `user_id` の両方を参照するため
3. アセット実体はメタデータ確定後に配置した方がロールバック補償しやすいため

### 11.3 実装上の方針

既存の高水準 API をそのまま使うと、現在時刻の再採番や新規 ID 発行が混入する。
そのため import 用には「値をそのまま投入する」低水準 API を追加する。

追加候補:

1. `insert_user_raw_in_txn(UserInfo)`
2. `insert_page_index_raw_in_txn(PageIndex)`
3. `insert_page_source_raw_in_txn(PageId, u64, PageSource)`
4. `insert_asset_info_raw_in_txn(AssetInfo)`
5. `insert_asset_blob(asset_id, bytes)`

### 11.4 トランザクションとファイル配置

アセット実体ファイルは redb トランザクション外でファイルシステムへ書く必要がある。
このため 2 段階反映にする。

1. 一時ディレクトリへ全アセットを書き出す
2. DB `WriteTransaction` でメタデータを反映して commit
3. commit 成功後に一時ファイルを最終配置へ rename
4. 途中失敗時は一時ディレクトリを破棄する

これにより、少なくとも以下を避ける。

1. DB 登録済みだがファイル未配置
2. ファイル配置済みだが DB 未登録

## 12. migrate export の原子性設計

### 12.1 成功条件

`migrate export` の成功は、次の全てが完了した時点とする。

1. 対象データ収集完了
2. ZIP 生成完了
3. 最終出力先への確定完了
4. 移送元通常ページ削除完了
5. 移送元ドラフト削除完了
6. 対象ページに紐付くロック削除完了

### 12.2 実装方式

redb transaction の中で ZIP を直接書くのではなく、以下の順で処理する。

1. read transaction で対象収集
2. 一時 ZIP 作成
3. write transaction 開始
4. 対象再確認
5. 再確認結果が収集時と一致した場合のみ削除
6. write transaction commit
7. ZIP を最終配置へ確定

この方式では「ZIP 出力」と「DB 削除」が単一 redb transaction には入らない。
ただし、CLI 仕様の要求する実運用上の原子性を満たすため、
削除前に ZIP を完成させ、削除 commit 前に不整合を再検出する方式を採る。

### 12.3 再確認項目

1. 起点ページの存在
2. 対象ページ ID 集合の一致
3. 各ページの `latest` と `path` の一致
4. 対象ドラフト ID 集合の一致

一致しない場合は中断する。

### 12.4 削除順序

1. 対象通常ページを再帰ハード削除
2. 対象ドラフトを個別削除
3. 残存ロックを削除

備考:

1. 通常ページは export 後に再利用不能な残骸を残さないためハード削除が妥当
2. 仕様文上の「無条件に削除」は、検証済み成功ケースでの方針を指す
3. 実装では途中失敗時ロールバック容易性を優先し、削除は commit 前に完結させる

## 13. CLI 設計

### 13.1 `cmd_args`

`src/cmd_args/mod.rs` に以下を追加する。

1. `Command::Export(ExportOpts)`
2. `Command::Import(ImportOpts)`
3. `ShowOptions`
4. `Validate`
5. `ApplyConfig`

### 13.2 `ExportOpts`

保持項目:

1. `subtree: Option<String>`
2. `dry_run: bool`
3. `password: Option<String>`
4. `yes: bool`
5. `strict_mode: bool`
6. `output: String`

### 13.3 `ImportOpts`

保持項目:

1. `migrate: Option<String>`
2. `user_map: Vec<String>`
3. `user_list: bool`
4. `dry_run: bool`
5. `fix_broken_link: bool`
6. `yes: bool`
7. `password: Option<String>`
8. `strict_mode: bool`
9. `input: String`

### 13.4 バリデーション

#### export

1. `--subtree` は絶対パスであること
2. `--subtree /` を禁止
3. `--dry-run` 時は `<OUTPUT>` の実書込検証を行わない

#### import

1. `--user-list` と `--fix-broken-link` の併用は許可する
   - ただし `--user-list` が優先され、実反映は行わない
2. `--migrate` 指定時は絶対パスであること
3. `--fix-broken-link` は `--migrate` 時のみ意味を持つ

## 14. テスト設計

### 14.1 単体テスト

対象:

1. path 相対化と再配置
2. manifest 件数検証
3. ID 重複検証
4. `username` 衝突検証
5. リンク分類
6. `--fix-broken-link` の置換結果

### 14.2 結合テスト

`tests/` に以下を追加する。

1. `export_backup_cli.rs`
2. `import_backup_cli.rs`
3. `export_migrate_cli.rs`
4. `import_migrate_cli.rs`
5. `export_import_roundtrip_cli.rs`

### 14.3 重点シナリオ

1. backup export -> empty DB へ import
2. migrate export -> 別 prefix へ import
3. migrate export でツリー外リンク検出
4. migrate import で `--fix-broken-link`
5. migrate import で `username` 衝突
6. assets.jsonl と実体ファイル不一致
7. rename 情報付き backup import
8. rename 情報混入 migrate import
9. 旧形式 `PageSourceV1` を含む DB の読込
10. `RenameInfo::RemovedByMigrate` を含む round-trip

## 15. 実装順序

1. `cmd_args` と `command` の器を追加する
2. `model.rs` と `errors.rs` を定義する
3. ZIP 読書きの薄い層を追加する
4. export 収集ロジックを実装する
5. import 検証ロジックを実装する
6. link 検証・書換えを実装する
7. import 反映ロジックを実装する
8. migrate export の削除連携を実装する
9. CLI 結合テストを追加する

## 16. 残課題

以下は実装前に最終判断を入れるべき項目である。

1. ZIP 暗号化ライブラリの選定
   - 現在の `Cargo.toml` には ZIP 用依存が未追加
2. `migrate export` の削除をソフト削除ではなくハード削除で確定してよいか
3. `path = ""` によるルート表現を `EXPORT_DATA_SPEC.md` へ追記するか
4. アセットリンク解析をページリンク解析と同一モジュールで扱うか分離するか

上記 2 は実装方式と運用復旧性に直接影響するため、着手前に確定させることが望ましい。
