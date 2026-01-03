# 設計概要

本ドキュメントは、Wiki システムの内部設計およびデータモデルを定義する。
要求仕様（REQUIREMENTS.md）を満たすための構造・責務分離・設計判断を記述する。

---

## 1. 設計方針

- 安定性・説明可能性を最優先
- 自動で賢く修正しすぎない
- 人が理解・修復できる余地を残す
- redb の特性（KVS / B+tree）に適合させる

---

## 2. モジュール構成

以下は主要モジュールの概要であり、責務の境界を示す。

- `src/http_server/` : HTTP サーバの起動と共有状態の管理
- `src/rest_api/` : REST API のルーティングと各エンドポイント実装
- `src/database/` : redb への永続化とトランザクション操作
- `src/command/` : CLI サブコマンドの実装
- `src/cmd_args/` : CLI 引数解析と設定読み込み
- `frontend/` : Web フロントエンド実装一式

### REST API モジュール構成

REST API はパス単位でサブモジュールを構成し、
各モジュールは HTTP メソッド名に対応する公開関数を持つ。

例:
- `/api/pages/{page_id}/source`
  - `src/rest_api/pages/source.rs`
  - `pub async fn get(...)` / `pub async fn put(...)`

---

## 3. データモデル

### 3.1 ページパスインデックステーブル

```text
Table<String, PageId>
```

- key: 現在のページパス（絶対パス）
- value: page_id
- パスはユニークとする
- ソフトリムーブ時はエントリを削除し、パスは再利用可能とする
- ハードリムーブ時に削除する
- ドラフト作成時にもエントリを登録し、ドラフト削除時に削除する

---

### 3.1.1 削除済みパスインデックステーブル

```text
MultimapTable<String, PageId>
```

- key: 削除時点のページパス（絶対パス）
- value: page_id
- 同一 path に複数 page_id を許容する（復活候補一覧）
- ソフトリムーブ時に登録し、復活またはハードリムーブ時に削除する

---

### 3.2 ページインデックステーブル

```text
Table<PageId, PageIndex>
```

```rust
enum PageIndex {
    PageInfo(PageInfo),
    DraftInfo(DraftInfo),
}

struct PageInfo {
    /// ページ固有のID
    page_id: PageId,

    /// 現在パス／削除時パス
    path_state: PagePathState,

    /// 最新リビジョン
    latest: u64,

    /// GC 下限リビジョン
    gc_min: u64,

    /// ロック情報(Some()の場合ロック中)
    lock_token: Option<LockToken>,

    /// path が確定・変更されたリビジョン番号の一覧（昇順）
    /// ページ作成時の初期パス割り当ても必ず含める
    rename_revisions: Vec<u64>,
}

struct DraftInfo {
    /// ページ固有のID
    page_id: PageId,

    /// 現在のパス
    path: String,
}

enum PagePathState {
    /// 現在のパス
    Current(String),

    /// 削除時点のパス
    LastDeleted(String),
}
```

- rename_revisions は「path が決定・変更されたイベント列」を表す
- 通常編集リビジョンは含まれない
- 作成時のリビジョンを必ず含めることで、任意リビジョン時点の path 解決が可能になる
- DraftInfo はドラフトページを表し、ソースやリビジョンを持たない

---

### 3.3 ページソーステーブル

```text
Table<(PageId, u64), PageSource>
```

```rust
struct PageSource {
    /// リビジョン番号
    revision: u64,

    /// 作成日時
    created_at: Timestamp,

    /// このリビジョンを作成したユーザ識別子
    /// ローカル運用を前提とし、人が識別可能な文字列とする
    user_name: String,

    /// path が割り当て／変更されたリビジョンのみ Some
    rename: Option<RenameInfo>,

    /// ページのソース(Markdown形式)
    source: String,
}
```

- PageRevision は「そのリビジョンで起きた事実」のみを保持する
- rename 情報は説明・注記用途であり、通常の内容 diff とは独立して扱う

---

### 3.3 RenameInfo（リネーム履歴情報）

```rust
struct RenameInfo {
    /// 旧パス（作成時は None 相当として扱う）
    from: Option<String>,

    /// 新パス
    to: String,

    /// リネーム直前時点でのページ中リンク解決状態（1段分）
    /// key: 正規化済み path
    /// value: 解決された page_id（未作成等で解決できなかった場合 None）
    link_refs: BTreeMap<String, Option<PageId>>,
}
```

- RenameInfo は rename（および作成時の初期 path 割り当て）という
  構造イベントを表現するための補助メタ情報
- diff 表示や履歴表示での説明可能性を目的とする

---

## 4. トランザクション設計

- 以下の操作は必ず同一 WriteTransaction で行う
  - ページ更新
  - rename
  - GC
  - ハードリムーブ

---

## 5. rename 設計

### 5.1 rename 処理

1. ページパスインデックスを更新（旧 path を削除、新 path を登録）
2. PageInfo の path_state を更新
3. 新しい revision を追加
4. rename 情報（from / to）を PageRevision に付与

### 5.2 rename を履歴に含める理由

- 過去バージョン参照時の説明可能性を確保
- diff 表示・履歴表示と自然に統合

---

## 6. 内部リンク設計

### 6.1 表現の二層構造

- 入力・表示：path
- 内部保存：page_id

### 6.2 保存時の正規化

- 相対パスを絶対パスに正規化
- path → page_id 解決
- 解決できたリンクのみ page_id に変換

---

## 7. rename 時のリンクの扱い

- 他ページのリンク：放置（page_id により解決）
- rename 対象ページ内リンク：
  - 絶対パス：放置
  - 相対パス：可能な範囲で page_id 維持

変換結果が変化する場合は UI で警告を通知

---

## 8. 差分表示（diff）

- すべての revision で diff 表示をサポート
- rename revision も通常 diff と同様に扱う
- diff とは別枠で rename 情報を表示

---

## 9. 履歴表示

- 各 revision を時系列で列挙
- rename revision は説明文を付与
- Markdown で生成し HTML にレンダリング

---

## 10. GC 設計

- 基本は手動 GC
- 任意 revision 以前を一括削除
- revision の歯抜けは発生させない

---

## 11. 削除設計

### 11.1 ソフトリムーブ

- PageInfo.path_state を LastDeleted に更新する
- ページパスインデックスのエントリを削除し、パスは再利用可能とする
- 削除済みパスインデックスに登録する

### 11.2 ソフトリムーブからの復帰

- `restore_to` で指定された path に復帰する
- PageInfo.path_state を Current に更新する
- ページパスインデックスに登録する
- 削除済みパスインデックスから削除する
- 復帰操作では revision を増加させない

### 11.3 ハードリムーブ

- 全 revision を削除
- PageInfo を削除
- ページパスインデックスのエントリも削除
- 削除済みパスインデックスのエントリも削除


## 11.4 ルートページの保護

- ルートページのパスは "/" とする
- 初回起動時の最初のユーザ登録後に、埋め込み済み雛形データから自動生成する
- ルートページは削除・リネームを禁止し、DB層でガードする
  - 削除要求／リネーム要求が "/" を対象とする場合はエラーで拒否する

## 11.5 付随アセットの扱い

- ページに付随するアセットが存在する場合は、アセットも同時に削除する
- ページの削除形態(ソフトデリート／ハードデリート)に関わらずアセットデータはソフトデリートとする
- 所有ページがハードデリートされたアセットはゾンビとして扱われる
- ゾンビアセットは管理者コマンドによりハードデリートされ完全消滅するか、所有ページの付け替えにより復活させることができる
- ドラフト削除時のアセットはハードデリートとし、ゾンビ化させない

---

## 12. 補助機能設計

- page_id の表示・コピー機能
- page_id → path 解決 API
- page_id 記法リンクのサポート

---

## 13. 拡張余地

- rename 履歴の多段化
- ゴミ箱 UI
- ページ統合・分割
- 内部リンク解析の高度化

---

## 14. 設計上の割り切り

- rename の完全再現は行わない
- 自動修復より説明可能性を重視
- UI はシンプルさを優先

---

## 15. アセット管理設計

### 15.1 データモデル

#### アセット情報テーブル
```rust
Table<AssetId, AssetInfo>
```

```rust
struct AssetInfo {
    original_name: String,
    mime: String,
    size: u64,
    created_at: Timestamp,
}
```

#### アセットID特定テーブル
```rust
Table<(PageId, String), AssetId>
```

第1型パラメータのStringはファイル名を指定。

#### ページ所属アセット群取得テーブル
```rust
MultimapTable<PageId, AssetId>
```

#### アセット所属ページ特定テーブル
```rust
Table<AssetId, PageId>
```

### 15.2 データ保管場所

- アセット本体はファイルシステムに保存
- パス規則：
```sh
$XDG_DATA_HOME/<app>/asset/{XX}/{YY}/{asset_id}
```
- XX = ULID先頭2文字
- YY = 続く3文字

### 15.3 参照解決

- Markdownソースのリンクには`asset:`記法で記載して保持
- フロントエンド側のmarkdown-itプラグインで`asset:xxx` → `/api/asset?path=xxx&file=yyy`への変換を行う
- サーバ側での解決手順
  1. 相対パスの正規化
  2. パス文字列をパスIDに変換
  3. パスIDとファイル名からアセットIDに変換
  4. `/api/asset?id={アセットID}`にリダイレクト
  5. `/api/asset?id={アセットID}`は実体を返却

---
## 16. ページ表示方式（ブラウザ・クライアント処理）

本システムでは、ページ閲覧時の表示処理をサーバとクライアントで分離する。
ブラウザでのアクセス URL は以下の形式とする：

```
/wiki/{page_path}
```

サーバ側は当該パスに対して、Markdown本文を含まないフレーム HTML を返却し、この HTML 内に`page_path`に対応するページIDとそのページのリビジョン番号を meta タグとして埋め込む。

```html
<meta name="wiki-page-id" content="{page_id}" />
<meta name="wiki-page-revision" content="{revision}" />
<div id="wiki-root"></div>
<script src="/static/app.js"></script>
```

クライアント側（JavaScript）は以下の手順でページ内容を表示する：

  1. meta タグから `page_id`と`revision` を取得する
  2. REST API `GET /api/page?id={page_id}&rev={revision}` により Markdown ソースを取得する
  3. markdown-it を用いて Markdown → HTML に変換する
  4. asset 記法 asset:... に対して独自プラグインにより /api/asset?... へ変換する
  5. 変換結果をフレーム HTML 上に描画する

この構成により、以下の利点を得る：

  - URL 表現 /wiki/{page_path} により相対リンクの自然な解決を実現
  - Markdown レンダリングの拡張（asset マクロ等）をフロントで柔軟に対応できる
  - サーバ側は REST API に責務を限定でき、テンプレート処理を持たない
  - ブラウザの「戻る」「ブックマーク」「URL共有」が自然に機能する
  - ローカル運用においても SPA 型 UI に近い軽量実装を維持できる

また、将来的にページ編集機能（Markdown エディタ／ライブプレビュー）を実装する際も、同様の API 構成・フロントレンダリング方式を流用できる。

---
## 17. ページ編集
### 17.1 編集URLの暫定仕様

ページ編集画面のURLは暫定的に以下の形式とする。

```
/edit/{page_path}
```

`/wiki/{page_path}` が存在しないページを参照した場合は、サーバ側で
`/edit/{page_path}` にリダイレクトし、新規作成と同等の扱いとする。
ルートページ `/` の場合は `/edit/` にリダイレクトする。

### 17.2 編集手順制御
ページロックで競合を避ける方法を取る。編集は以下の手順で制御を行う。

  1. 閲覧画面で編集を開始をトリガ
  2. 対象ページに対しREST API `POST /api/pages/{page_id}/lock`でロック取得をトライ
      - すでにロックが取得されている場合やその他のエラーが発生した場合はエラー表示を行い編集画面へ遷移させない
  3. セッションストレージにロック操作で得られたトークンを保存
  4. 編集ページに遷移させる

セッションストレージへの保存キーはページIDとタブ固有IDを連結して生成する。タブ固有IDの求め方は別章に記述する。
編集ページへの遷移後は以下の手順で制御を行う。

  1. ページオープン時にセッションストレージを参照
      - セッションストレージにトークンがない場合はエラー表示を行い編集させない(URLコピーによる複数タブでの編集への対策)
  2. インターバルタイマーを設定し、タイマーハンドラでREST API `PUT /api/pages/{page_id}/lock`を発行させてページロックのTTL延長とトークンの更新を継続的に行うようにする。
  3. 保存またはキャンセル前のタブクローズに備え、タブクローズをフックしてREST API `DELETE /api/pages/{page_id}/lock` を発行するように設定 (ただしこのフックでのREST API実行の確実性は低いので、保険としてロックのTTLによるロック解除でフォールバックする)
  4. REST API `GET /api/pages/{page_id}/source[?rev={revision}]`でページソースを取得
  5. ユーザにソースを編集させる
      - 保存操作が行われたら REST API `/api/pages/{page_id}/source[?amend={boolean}]`でページソースを保存(このAPIでロックは解除される)
      - キャンセル操作が行われたら REST API `DELETE /api/pages/{page_id}/lock`でロックを解除
  6. セッションストレージのトークンを削除する
  7. 閲覧ページに遷移する

### 17.2.1 新規ページ作成手順

`/edit/{page_path}` が存在しないページを指している場合は、新規ページ作成として扱う。

  1. REST API `POST /api/pages?path={page_path}` を発行し、ドラフト作成とロック取得を同一トランザクションで行う
  2. レスポンスヘッダ`X-Page-Lock`のトークンをセッションストレージに保存する
  3. ドラフトはソースを持たないため、`GET /api/pages/{page_id}/source` は呼び出さない
  4. 編集・保存操作で REST API `PUT /api/pages/{page_id}/source` を呼び出し、通常ページとして登録する
      - 初回保存はリビジョン番号1として登録する
      - 保存成功時にロックは解除される
  5. キャンセル操作では REST API `DELETE /api/pages/{page_id}/lock` を呼び出し、ドラフトを削除する
  6. セッションストレージのトークンを削除し、閲覧ページに遷移する

### 17.3 ロック管理テーブル
```rust
Table<LockToken, LockInfo>
```

```rust
struct LockInfo {
    /// ロック解除トークン
    token: LockToken,

    /// ロック対象ページのId
    page: PageId,

    /// ロックを行ったユーザのID
    user: UserId,

    /// 有効期限
    expire: Timestamp,
}
```

LockTokenはULIDで付与する(ULIDのタイムスタンプ部分はロックの開始時刻を表す)。

ロックの有効期限は振り出し時刻から5分間とする(LockTokenのタイムスタンプ部分をそのまま振り出し時刻として扱う)。

ロックの有効期限管理は10秒ごとのロック管理テーブルへのポーリングで行い有効期限切れエントリの探索を行う。有効期限が切れたロック情報に対しては、ターゲットのページがPageInfoの場合はページ情報に登録されたトークンを削除しテーブルからエントリを削除する。対象のページがドラフトの場合は、ドラフトページを削除した上でテーブルからエントリを削除する。

ロックの解除を行う場合も有効期限切れのエントリと同じく、ターゲットのページがPageInfoの場合はページ情報に登録されているロック解除トークンを削除し、テーブルからエントリを削除する。対象のページがドラフトの場合は、ドラフトページを削除した上でテーブルからエントリを削除する。

ロックの延長を受け付けた場合は、ロック解除トークンの振り出しを行い更新したエントリを登録する(ターゲットのページ情報の更新も行う)。あわせて旧エントリは削除する。

---
## 18. タブ固有ID
タブの複製を行われるとセッションストレージも複製され、複数タブでのロック解除トークンの共有が発生する可能性がある。このため編集のロック解除トークンの複数タブでの共有を防ぎ編集の競合を避ける目的でタブ固有IDを用いる。

しかし、ブラウザ標準ではタブ固有IDを提供していないので、ロード完了イベントにおいて以下の処理を行うことにより、タブ固有IDの生成を行う。

  1. ブロードキャストチャネルを既定のキーで生成する。
      - ブロードキャストチャネルの受信ハンドラとして、受信したIDと自身のIDが一致する場合は「既存」を通知する処理を登録する
  2. セッションストレージにタブ固有キーが記録されていなければ、UUIDv4を生成し記録する。
  3. セッションストレージからタブ固有キーをロードする
  4. ブロードキャストチャネルで同一IDを持つタブがいないかブロードキャストを行う
  5. 一定時間待ちを行う(50ms程度)
      - 時間内に「既存」を受信した場合はIDの振り直しを行い、セッションストレージに保存したIDの上書きを行う

上記の処理完了時に、セションストレージに保存されているIDがタブ固有IDとなる。

なお、タブ固有ID生成処理は非同期関数化しUIを停滞をさせない実装にする。
