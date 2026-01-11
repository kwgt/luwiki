# コマンドライン仕様

本書ではアプリケーションで実装するコマンドライン仕様について定義する。

---
## コマンドライン

```sh
luwiki [OPTIONS] <SUB-COMMAND> [COMMAND-OPTIONS]

```

### グローバルオプション
グローバルオプションとして以下の物を指定できる。

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-c`, `--config-path FILE` | デフォルト設定ファイルのパスを指定する | $XDG_CONFIG_HOME/luwiki/config.toml
| `-l`, `--log-level LEVEL`  | ログレベルを指定する | "info"
| `-L`, `--log-output PATH`  | ログレベルの出力先を指定する | $XDG_DATA_HOME/luwiki/log/
| `-T`, `--tls` | サーバをHTTPSで起動させる |
| `-C`, `--cert FILE` | HTTPS使用時の証明書ファイルのパスを指定する | $XDG_DATA_HOME/luwiki/server.pem
| `-d`, `--db-path FILE` | データベースファイルのパスを指定する | $XDG_DATA_HOME/luwiki/database.redb
| `-I`, `--fts-index DIR` | 全文検索インデックスの格納パスを指定する | $XDG_DATA_HOME/luwiki/index
| `-a`, `--assets-path` | アセットデータ格納パスを指定する | $XDG_DATA_HOME/luwiki/assets
|       `--show-options` | 設定情報の表示 |
|       `--save-config` | config.tomlへの設定情報の保存指示 |
| `-h`, `--help`          | ヘルプメッセージの表示 |
| `-v`, `--version`       | プログラムのバージョン番号の表示 |

`--tls`オプションを指定した場合、サーバはHTTPSでの通信を行う。このとき`--cert`オプションで指定されたサーバ証明書を用いる。`--cert`が指定されていない場合は規定のパスに置かれた証明書を使用するが、このファイルも存在しない場合は証明書を自動的に生成する(`--cert`オプションが指定され、そのファイルが存在しない場合はエラー)。

`--cert`で指定するファイルはPEM形式とする。PEMにはサーバ証明書と秘密鍵を含めるものとする。
証明書を自動生成する場合、生成物は`$XDG_DATA_HOME/luwiki/server.pem`（PEM）に保存する。PEM以外の補助ファイルが必要な場合は、`$XDG_DATA_HOME/luwiki/cert/`配下に保存する。

`--log-level`オプションの`<LEVEL>`には以下の値が設定可能。

  - none : ログを記録しない
  - error : エラーの場合のみを記録
  - warn : 警告以上の場合を記録
  - info : 一般情報レベルを記録
  - debug : デバッグ用メッセージも記録
  - trace : トレース情報も記録

`--log-output`にはログの出力先を指定できるが、ファイルのパスを指定した場合は単一ファイルへの出力となり、ディレクトリパスを指定した場合はログローテション付きで10本のファイルに自動切り替えを行いながら記録を行う(一本あたりのサイズ制限は2Mバイト)。

---
## サブコマンド
以下のサブコマンドが使用できる。

  - [run](#run) : サーバーの起動
  - [commands](#commands) : サブコマンドの一覧表示
  - [help-all](#help-all) : 全サブコマンドのヘルプ表示
  - user : ユーザ管理
    - [add](#subcmd-user-add) : ユーザの追加
    - [delete](#subcmd-user-delete) : ユーザの削除
    - [list](#user-list) : ユーザ情報の一覧表示
    - [edit](#user-edit) : ユーザ情報の変更
  - page : ページの管理
    - [add](#page-add) : ページの追加
    - [list](#page-list) : ページ一覧の表示
    - [delete](#page-delete) : ページの削除
    - [unlock](#page-unlock) : ページのロック解除
    - [undelete](#page-undelete) : ページの回復(削除の取消)
    - [move_to](#page-move-to) : ページの移動
  - lock : ページの管理
    - [list](#lock-list) : ロックの一覧
    - [delete](#lock-delete) : ロックの削除(アンロック)
  - asset : アセットの管理
    - [add](#asset-add) : アセットの追加
    - [list](#asset-list) : アセット一覧の表示
    - [delete](#asset-delete) : アセットの削除
    - [undelete](#asset-undelete) : アセットの回復(削除の取消)
    - [move_to](#asset-move-to) : アセットの所有ページの付け替え
  - fts : 全文検索の管理
    - [rebuild](#rebuild-index) : インデックスの再構築
    - [merge](#merge-segment) : セグメントの強制マージ
    - [search](#fts-search) : 検索の実施

サブコマンドのエイリアスは以下の通り。

  - run : `r`
  - commands : (なし)
  - help-all : (なし)
  - user : `u`
    - add : `a`, `add`
    - delete : `d`, `del`
    - list : `l`, `ls`
    - edit : `e`, `ed`
  - page : `p`
    - add : `a`
    - list : `l`
    - delete : `d`, `del`
    - unlock : `ul`
    - undelete : `ud`
    - move_to : `m`, `mv`
  - lock : `l`
    - list : `l`, `ls`
    - delete : `d`, `del`
  - asset : `a`
    - add : `a`
    - list : `l`, `ls`
    - delete : `d`, `del`
    - undelete : `ud`
    - move_to : `m`, `mv`
  - fts : `i`
    - rebuild : `r`
    - merge : `m`
    - search : `s`

<a id="run"></a>
### runコマンド
HTTP/HTTPSサーバを起動する。

#### コマンドライン
```sh
luwiki [OPTIONS] run [COMMAND-OPTIONS] [BIND-ADDR[:PORT]]
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-b`, `--open-browser` | サーバ起動時にブラウザを起動する |
 
#### 概要
引数`BIND-ADDR:PORT`でアドレスにバインドしHTTP/HTTPSサーバを起動する(デフォルトは"0.0.0.0:8080")。

`--open-browser`オプションが指定された場合は、同時に規定のブラウザを起動する（デスクトップ環境でのみ有効）。

ユーザ未登録の状態で`run`コマンドを実行した場合はエラーとする。

<a id="commands"></a>
### commandsコマンド
サブコマンドの一覧表示

#### コマンドライン
```sh
luwiki [OPTIONS] commands
```

#### 概要
サブコマンドの一覧を `{コマンド} : {説明}` の1行形式で表示する。

出力にはルートコマンド(`luwiki`)は含めない。

<a id="help-all"></a>
### help-allコマンド
全サブコマンドのヘルプ表示

#### コマンドライン
```sh
luwiki [OPTIONS] help-all
```

#### 概要
各サブコマンドのヘルプを `{コマンド} : {説明}` の区切りで連結出力する。

ヘルプ本文の各行には、行頭に空白2文字を付与して表示する。

出力にはルートコマンド(`luwiki`)を含める。

<a id="user-add"></a>
### user addコマンド
ユーザ情報の登録

#### コマンドライン
```sh
luwiki [OPTIONS] user add [Option] <USER-NAME>
```
#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-d`, `--display-name <NAME>` | 表示名の指定 |
 
#### 概要
引数 `USER-NAME` で指定されたユーザ名でユーザ登録を行う。このコマンドを実行するとパスワード登録用のプロンプトが表示され、パスワード入力が求められる。入力されたパスワードに問題が無ければユーザの登録が行われる。

同名のユーザが既に登録されていた場合はエラーとする。

<a id="user-delete"></a>
### user deleteコマンド
ユーザ情報の削除

#### コマンドライン
```sh
luwiki [OPTIONS] user delete <USER-NAME>
```

#### 概要
引数 `USER-NAME` で指定されたユーザ名のユーザ情報を削除する。

同名のユーザが登録されていない場合はエラーとする。

<a id="user-edit"></a>
### user editコマンド
ユーザ情報の変更

#### コマンドライン
```sh
luwiki [OPTIONS] user edit <USER-NAME>
```
#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-d`, `--display-name <NEW-NAME>` | 表示名の指定 |
| `-p`, `--password` | パスワードの指定 |
 
#### 概要
引数 `USER-NAME` で指定されたユーザ名のユーザ情報を変更する。

`--display-name`オプションが指定された場合は表示名を`NEW-NAME`指定された表示名に更新する。

`--password`オプションが指定された場合はパスワード入力用プロンプトを表示しユーザに新パスワードの入力を促し、その入力内容でパスワードの更新を行う。

`--display-name`, `--password`オプションのいずれも指定されなかった場合はエラーとなる。

同名のユーザが登録されていない場合はエラーとする。

<a id="user-list"></a>
### user listコマンド
ユーザ情報の一覧表示

#### コマンドライン
```sh
luwiki [OPTIONS] user list [OPTIONS]
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `--sort-by` | 一覧のソート方法 |
| `-r`, `--reverse-sort` | ソートを逆順で行う |
 
#### 概要
登録されているユーザの一覧を表示する。

`--sort-by`にはソート順序を指定できる。以下の指定が可能。

  - `default`: ID順
  - `user_name`: ユーザ名順
  - `display_name`: 表示名順
  - `last_update`: 更新日時順

`--reverse-sort`で順序を反転させる。

<a id="page-add"></a>
### page addコマンド
ページの追加

#### コマンドライン
```sh
luwiki [OPTIONS] page add <FILE-PATH> <PAGE-PATH>
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-u`, `--user <USER-NAME>` | 登録ユーザ名の指定 | `page.add.default_user`

#### 概要
`FILE-PATH`で指定されたMarkdownファイルを`PAGE-PATH`として取り込む。

以下の場合はエラーとする。

  - `FILE-PATH`の拡張子が`.md`ではなかった
  - `FILE-PATH`のMarkdown構文解析に失敗した
  - `--user`オプションが指定されておらず、`page.add.default_user`が未設定だった

追加に成功した場合は、作成されたページのページIDを標準出力に出力する。

<a id="page-list"></a>
### page listコマンド
ページ情報の一覧表示

#### コマンドライン
```sh
luwiki [OPTIONS] page list [OPTIONS]
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-l`, `--long-info` | 詳細情報で表示 |
| `--sort-by` | 一覧のソート方法 |
| `-r`, `--reverse-sort` | ソートを逆順で行う |
 
#### 概要
登録されているページの一覧を表示する。`--long-info`オプションを指定していない場合の表示項目はID, パスのみを表示する。

`--long-info`オプションを指定した場合はID, 更新日時, ユーザ, リビジョン, ページパスを表示する。また、表示行先頭にページ状態を表示する領域を設ける。この領域には状態に応じて以下の表示を行うようにする。ドラフト状態のページはユーザ名とリビジョンは"***"と表示される。

  - 通常状態 : 空白文字
  - ドラフト状態のページ : "d"
  - 削除されているページ : "D"
  - ロックされているページ : "L"

削除状態のページパスは`[]`で囲んで表示する。`--long-info`の有無に関わらず適用する。

`--sort-by`にはソート順序を指定できる。以下の指定が可能。

  - `default`: ID順
  - `user_name`: ユーザ名順 
  - `page_path`: ページパス順
  - `last_update`: 更新日時順

`--reverse-sort`で順序を反転させる。

<a id="page-delete"></a>
### page deleteコマンド
ページの削除

#### コマンドライン
```sh
luwiki [OPTIONS] page delete [OPTIONS] <PAGE-ID|PAGE-PATH>
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-H`, `--hard-delete` | ハードデリートを行う |
| `-f`, `--force` | ロック中でも強制的に削除を行う |

#### 概要
`PAGE-ID`または`PAGE-PATH`で指定されたページの削除を行う。デフォルトはソフトデリートを行う。

`--hard-delete`オプションが指定されている場合はハードデリートする。

`--force`オプションが指定されている場合は、ロック中のページでも強制的に削除を行う。削除成功時はロックを解除する。

以下の場合はエラーとする。

  - ルートページ("/")が指定された
  - 存在しないページが指定された
  - 削除済み(ソフトデリート中)のページが指定され、`--hard-delete`が指定されていない
  - ロック中のページが`--force`オプション無しで指定された

ハードデリートされた場合は同一のパスの再利用を許可し、DB上では存在しないページとして扱う。

#### 注記
  - ドラフト状態のページをデリートした場合は`--hard-delete`オプションが指定されていない場合でもハードデリートされる。また付随しているアセットが存在する場合も全てハードデリートされる。

<a id="page-unlock"></a>
### page unlockコマンド
ページのロック解除

#### コマンドライン
```sh
luwiki [OPTIONS] page unlock <PAGE-ID|PAGE-PATH>
```

#### 概要
`PAGE-ID`または`PAGE-PATH`で指定されたページのロックを解除する。

以下の場合はエラーとする。

  - 指定されたページが存在しない
  - 指定されたページにロックが存在しない

#### 注記
  - ドラフト状態のページをアンロックした場合はその時点でハードデリートされる。また付随しているアセットが存在する場合も全てハードデリートされる。

<a id="page-undelete"></a>
### page undeleteコマンド
ページの回復(削除の取消)

#### コマンドライン
```sh
luwiki [OPTIONS] page undelete <PAGE-ID> <PAGE-PATH>
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `--without-assets` | 付随アセットの復旧を行わない | `page.undelete.with_assets`
| `-r`, `--recursive` | 配下ページを含めて復帰する | false

#### 概要
`PAGE-ID`で指定されたページを削除状態から通常状態に復活させる。復帰先のパスは`PAGE-PATH`で指定する。
`--recursive`が指定された場合は配下ページもまとめて復帰する。

デフォルトでは付随アセットも復旧する。`--without-assets`が指定された場合はアセットの復旧を行わない。

以下の場合はエラーとする。

  - 指定されたIDのページが存在しなかった
  - 指定されたIDのページが削除状態ではなかった
  - 指定されたパスにページが既に存在する
  - `--recursive`指定時、配下にロック中のページが存在する

<a id="page-move-to"></a>
### page move_toコマンド
ページの移動

#### コマンドライン
```sh
luwiki [OPTIONS] page move_to [OPTIONS] <SRC_PAGE_PATH|SRC_PAGE-ID> <DST_PAGE_PATH>
```

#### オプション
| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-f`, `--force` | ロック中でも強制的に移動を行う |
| `-r`, `--recursive` | 配下ページを含めて移動を行う |

#### 概要
`SRC_PAGE_PATH`または`SRC_PAGE-ID`で指定されたページを`DST_PAGE_PATH`へ移動する。

`--force`オプションが指定された場合はロック中でも強制的に移動を行う(ロック自体は解除されない)。

`--recursive`が指定された場合は配下ページもまとめて移動する。この場合、配下にロック中のページが存在すると失敗する。

以下の場合はエラーとする。

  - `DST_PAGE_PATH`のページが既に存在する
  - `SRC_PAGE_PATH`または`SRC_PAGE-ID`で指定されたページが存在しない
  - ルートページ("/")が指定された
  - ロック中のページが`--force`オプション無しで指定された

<a id="lock-list"></a>
### lock listコマンド
ロック情報の一覧表示

#### コマンドライン
```sh
luwiki [OPTIONS] lock list [OPTIONS]
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-l`, `--long-info` | 詳細情報で表示 |
| `--sort-by` | 一覧のソート方法 |
| `-r`, `--reverse-sort` | ソートを逆順で行う |
 
#### 概要
登録されているユーザの一覧を表示する。`--long-info`オプションを指定していない場合の表示項目はID, ターゲットページのパスのみを表示する。

`--long-info`オプションを指定した場合はID, 有効期限, ロックしているユーザ, ページパスを表示する。

`--sort-by`にはソート順序を指定できる。以下の指定が可能。

  - `default`: ID順
  - `expire`: 有効期限順
  - `user_name`: ユーザ名順 
  - `page_path`: ページパス順

`--reverse-sort`で順序を反転させる。

<a id="lock-delete"></a>
### lock deleteコマンド
ロック情報の削除

#### コマンドライン
```sh
luwiki [OPTIONS] lock delete <LOCK-ID>
```

#### 概要
`LOCK-ID`で指定されたロック情報の削除を行う。ページに対するアンロックと同じ処理を行う。

#### 注記
  - ドラフト状態のページに対するロックをアンロックした場合はその時点で対象ページがハードデリートされる。またそのページに付随しているアセットが存在する場合も全てハードデリートされる。

<a id="asset-add"></a>
### asset addコマンド
アセットの追加

#### コマンドライン
```sh
luwiki [OPTIONS] asset add [OPTIONS] <FILE-PATH> <PAGE-ID|PAGE-PATH>
```
#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-t`, `--mime-type <TYPE>` | MIME種別の指定 | `FILE-PATH`の拡張子で自動的に判断
| `-u`, `--user <USER-NAME>` | 登録ユーザ名の指定 | `asset.add.default_user`
 
#### 概要
`FILE-PATH`で指定されたローカルファイルを`PAGE-ID`または`PAGE-PATH`で指定されたページのアセットとして追加する。アセット名は`FILE-PATH`のファイル名を使用する。

以下の場合はエラーとする。

  - `FILE-PATH`で指定されたファイルが存在しなかった
  - `FILE-PATH`で指定されたファイルのサイズが10MiBバイトを超えていた
  - `PAGE-ID`または`PAGE-PATH`で指定されたページが存在しなかった
  - `--user`オプションが指定されておらず、`asset.add.default_user`が未設定だった

追加に成功した場合は、追加されたアセットのアセットIDを標準出力に出力する。

<a id="asset-list"></a>
### asset listコマンド
アセット一覧の表示

#### コマンドライン
```sh
luwiki [OPTIONS] asset list [OPTIONS]
```
#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-l`, `--long-info` | 詳細情報で表示 |
| `--sort-by` | 一覧のソート方法 |
| `-r`, `--reverse-sort` | ソートを逆順で行う |
 
#### 概要
登録されているアセットの一覧を表示する。`--long-info`オプションを指定していない場合の表示項目はID, ファイル名, MIME種別, サイズのみを表示する。

`--long-info`オプションを指定した場合は以下を表示する。

  - ID
  - アップロード日時
  - アップロードユーザ名
  - MIME種別
  - サイズ
  - 所有ページのパスとファイル名(ゾンビの場合はパス部分は"?????"とする)

日時の表示形式は `YYYY-MM-DDTHH:MM:SS` とし、タイムゾーンは表示しない。

サイズはカンマ区切りで `B` / `Ki` / `Mi` / `Gi` の単位を付与して表示する（数値と単位は連結する）。

また、表示行先頭にページ状態を表示する領域を設ける。この領域には状態に応じて以下の表示を行うようにする。

  - 通常状態 : 空白文字
  - 削除されているアセット : "D"
  - ゾンビ状態のアセット : "Z"
  - 削除かつゾンビ状態のアセット : "B"

`--sort-by`にはソート順序を指定できる。以下の指定が可能。

  - `default`: ID順
  - `upload`: アップロード日時順
  - `user_name`: アップロードユーザ名順
  - `mime_type`: MIME種別順
  - `size` : サイズ順
  - `path` : ページパス順

`--reverse-sort`で順序を反転させる。

<a id="asset-delete"></a>
### asset deleteコマンド
アセットの削除

#### コマンドライン
```sh
luwiki [OPTIONS] asset delete [OPTIONS] <ASSET-ID|ASSET-PATH|PAGE-PATH|PAGE-ID>
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-H`, `--hard-delete` | ハードデリートを行う |
| `-p`, `--purge` | ソフトデリートされているファイルをハードデリートする |

#### 概要
アセットID(`ASSET-ID`)かアセットパス(`ASSET-PATH`)で指定されたアセットの削除を行う。アセットパスはページパスとファイル名をパスセパレータで連結して表現する。
ページパス(`PAGE-PATH`)またはページID(`PAGE-ID`)が指定された場合は、ページに付随しているアセット全てが削除対象になる。

`--hard-delete`オプションが指定されている場合はハードデリートする(指定されていない場合はソフトデリートを行う)。

`--hard-delete`は削除済みアセットに対しても使用でき、DB上から完全に消去する。

`--purge`は指定されたページに付随するアセットのうち、ソフトデリート中の物を選択的にハードデリートする。このオプションを指定した場合はページパスかページID以外は指定できない。

指定されたアセットが存在しない場合はエラーとする。

削除済みアセットに対する削除は、`--hard-delete`指定時のみ許可する。

<a id="asset-undelete"></a>
### asset undeleteコマンド
アセットの回復(削除の取消)

#### コマンドライン
```sh
luwiki [OPTIONS] asset undelete <ASSET-ID>
```
#### 概要
`ASSET-ID`で指定されたアセットを削除状態から通常状態に復活させる。`ASSET-ID`のみを受け付ける。

以下の場合はエラーとする。

  - 指定されたIDのアセットが存在しなかった
  - 指定されたIDのアセットが削除状態ではなかった

<a id="asset-move-to"></a>
### asset move_toコマンド
アセットの所有ページの付け替え

#### コマンドライン
```sh
luwiki [OPTIONS] asset move_to <ASSET-ID> <PAGE-ID|PAGE-PATH>
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-f`, `--force` | 競合時や削除済みページでも移動を行う |

#### 概要
指定されたアセットの所有ページをページID(PAGE-ID)かページパス(PAGE-PATH)で指定されたページに切り替える（ファイル名は変更できない）。

以下の場合はエラーとする。

  - 指定されたIDのアセットが存在しなかった
  - 指定されたページが存在しなかった
  - 移動先ページが削除済みで`--force`が指定されていない
  - 移動先に同名アセットが存在し`--force`が指定されていない

`--force`が指定された場合、移動先の削除済みページへの移動と同名アセットの上書きを許可する。

<a id="fts-rebuild"></a>
### fts rebuildコマンド
全検索インデックスの再構築

#### コマンドライン
```sh
luwiki [OPTIONS] fts rebuild
```

#### 概要
全文検索用インデックスの削除を行いインデックスの再構築を行う。

<a id="merge-segment"></a>
### fts mergeコマンド
インデックスセグメントの強制マージ

#### コマンドライン
```sh
luwiki [OPTIONS] fts merge
```

#### 概要
全文検索インデクス中のマージが済んでいないセグメントのマージを強制的に行う。

<a id="fts-search"></a>
### fts searchコマンド
テスト検索の実施

#### コマンドライン
```sh
luwiki [OPTIONS] fts search [OPTIONS] <SEARCH-EXPRESSION>
```
#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-t`, `--target <TARGET>` | 検索対象を指定する | BODY
| `-d`, `--with-deleted` | 削除済みページを検索対象に含める |
| `-a`, `--all-revision` | 全てのリビジョンを表示する |
 
#### 概要
テスト用に`<SEARCH-EXPRESSION>`で指定された検索式を用いて全文検索を行う。検索式はtantivyの検索式仕様に則る。
ページID,リビジョン,スコア,テキスト を表示する。

デフォルトでは検索対象は以下の通り。

  - 削除済みのページは検索対象に含めない
  - 最新リビジョンのみを検索対象とする

`--target`オプションで検索対象を指定できる。`--target`には以下の対象が指定できる。

  - headings : 見出し
  - body : 本文
  - code : コードブロック

`--with-deleted`オプションを指定した場合は削除済みページを検索対象に含める。

`--all-revision`オプションを指定した場合は全リビジョンを検索対象とする。

---
## コンフィギュレーションファイル
各種オプション(サブコマンドのオプションを含む)のデフォルト値が定義できる設定ファイル(toml形式)が置かれる。デフォルトパスは `$XDG_CONFIG_HOME/luwiki/config.toml` とする（グローバルオプションの `--config-path`で変更可能）。

コンフィギュレーションファイルには以下のサブコマンドに対応したテーブルを設ける。

  - [global](#config-global)
  - [run](#config-run)
  - user
      - [list](#config-user-list)
  - page
      - [add](#config-page-add)
      - [list](#config-page-list)
      - [undelete](#config-page-undelete)
  - lock
      - [list](#config-lock-list)
  - asset
      - [add](#config-asset-add)
      - [list](#config-asset-list)

  - fts
      - [search](#config-fts-search)

<a id="config-global"></a>
### globalテーブル
グローバルオプションに対するデフォルト値を定義し以下のキーを定義する。

| キー | 設定内容 | 対応オプション |デフォルト値
|:--|:--|:--|:--
| `log_level` | ログレベル | `--log-level` | "info"
| `log_output` | ログの出力先 | `--log-output` | `$XDG_DATA_HOME/luwiki/log`
| `use_tls` | TLSの使用 | `--tls` | false
| `server_cert` | 使用するサーバ証明書 | `--cert` | `$XDG_DATA_HOME/luwiki/server.pem`
| `db_path` | データベースファイルのパス | `--db-path` | `$XDG_DATA_HOME/luwiki/database.redb`
| `assets_path` | アセットデータ格納ディレクトリのパス | `--assets-path` | `$XDG_DATA_HOME/luwiki/assets/`
| `fts_index` | 全文検索インデックス格納ディレクトリのパス | `--fts-index` | `$XDG_DATA_HOME/luwiki/index/`

<a id="config-run"></a>
### runテーブル
`run` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `bind` | サーバがバインドするアドレスを指定する | `BIND-ADDR` | "0.0.0.0"
| `port` | サーバがバインドするポートを指定する | `PORT` | 8080

<a id="config-user-list"></a>
### user.listテーブル
`user list` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `sort_mode` | ソート指示 | `--sort-by` | "default"
| `reverse_sort` | ソート順序を逆順にするか否か | `--reverse-sort` | false

`sort_mode`に指定できる値は以下の何れかとする。

  - "default" : デフォルト(エントリIDでソート)
  - "user_name" : ユーザ名でソート
  - "display_name" : 表示名でソート
  - "last_update" : 更新日時でソート

<a id="config-page-list"></a>
### page.listテーブル
`page list` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `sort_mode` | ソート指示 | `--sort-by` | "default"
| `reverse_sort` | ソート順序を逆順にするか否か | `--reverse-sort` | false

`sort_mode`に指定できる値は以下の何れかとする。

  - "default" : デフォルト(エントリIDでソート)
  - "user_name" : ロックを行ったユーザの名前でソート
  - "page_path" : ページパスでソート
  - "last_update" : 更新日時でソート

<a id="config-page-add"></a>
### page.addテーブル
`page add` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `default_user` | 登録ユーザ名 | `--user` | (未設定)

<a id="config-page-undelete"></a>
### page.undeleteテーブル
`page undelete` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `with_assets` | アセットの復旧を行うか否か | `--without-assets` | true

<a id="config-lock-list"></a>
### lock.listテーブル
`lock list` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `sort_mode` | ソート指示 | `--sort-by` | "default"
| `reverse_sort` | ソート順序を逆順にするか否か | `--reverse-sort` | false

`sort_mode`に指定できる値は以下の何れかとする。

  - "default" : デフォルト(エントリIDでソート)
  - "user_name" : ユーザ名でソート
  - "display_name" : 表示名でソート
  - "last_update" : 更新日時でソート

<a id="config-asset-list"></a>
### asset.listテーブル
`asset list` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `sort_mode` | ソート指示 | `--sort-by` | "default"
| `reverse_sort` | ソート順序を逆順にするか否か | `--reverse-sort` | false

`sort_mode`に指定できる値は以下の何れかとする。

  - "default" : デフォルト(アセットIDでソート)
  - "upload" : アップロード日時でソート
  - "user_name" : アップロードユーザ名でソート
  - "mime_type" : MIME種別でソート
  - "size" : サイズでソート
  - "path" : ページパスでソート

<a id="config-asset-add"></a>
### asset.addテーブル
`asset add` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `default_user` | 登録ユーザ名 | `--user` | (未設定)

<a id="config-fts-search"></a>
### fts.searchテーブル
`fts search` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `target` | 検索対象の指定 | `--target` | "body"
| `with_deleted` | 検索対象に削除済みページを含めるか否かを指定 | `--with-deleted` | false
| `all_revision` | 全リビジョンを検索対象に含めるか否かを指定 | `--all-revision` | false

`target`に指定できる値は以下の何れかとする。

  - headings : 見出し
  - body : 本文
  - code : コードブロック
