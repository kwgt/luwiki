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
| `-d`, `--db-path FILE` | データベースファイルのパスを指定する | $XDG_DATA_HOME/luwiki/database.redb
| `-I`, `--fts-index DIR` | 全文検索インデックスの格納パスを指定する | $XDG_DATA_HOME/luwiki/index
| `-a`, `--assets-path` | アセットデータ格納パスを指定する | $XDG_DATA_HOME/luwiki/assets
| `-t`, `--template-root` | テンプレートページのパスを指定する | 
| `-T`, `--wiki-title` | Wiki名を指定する | LUWIKI
| `-S`, `--asset-limit-size` | アップロード可能なアセットのサイズの上限 | 10Miバイト 
|       `--audit-log-dir DIR` | 監査ログ出力ディレクトリを指定する | $XDG_DATA_HOME/luwiki/audit
|       `--audit-log-retention DURATION` | 監査ログ保持期間を指定する | "90d"
|       `--audit-log-rotate-size SIZE` | 監査ログローテーション閾値サイズを指定する | "2M"
|       `--show-options` | 設定情報の表示 |
|       `--save-config` | config.tomlへの設定情報の保存指示 |
| `-h`, `--help`          | ヘルプメッセージの表示 |
| `-v`, `--version`       | プログラムのバージョン番号の表示 |

`--log-level`オプションの`<LEVEL>`には以下の値が設定可能。

  - none : ログを記録しない
  - error : エラーの場合のみを記録
  - warn : 警告以上の場合を記録
  - info : 一般情報レベルを記録
  - debug : デバッグ用メッセージも記録
  - trace : トレース情報も記録

`--log-output`にはログの出力先を指定できるが、ファイルのパスを指定した場合は単一ファイルへの出力となり、ディレクトリパスを指定した場合はログローテション付きで10本のファイルに自動切り替えを行いながら記録を行う(一本あたりのサイズ制限は2Mバイト)。

`--template-root`にはテンプレートとして使用するページが格納されるWiki上のパスを指定する。このオプションが指定された場合はページ編集時に、このオプションで指定されたページの子ページをテンプレートとして使用することができる(`--template-root`未指定時はテンプレート機能自体が無効化される)。

`--asset-limit-size`にはアップロード可能なアセットのサイズの上限を指定するが、"10K", "10M"などの補助単位を指定可能とする(いずれも2進接頭辞のKi,Miを意味する)。また最大は100Miまで指定可能。

`--audit-log-retention` には監査ログの保持期間を指定し、`Nd`, `Nh`, `Nm` の形式を許可する。

`--audit-log-rotate-size` には監査ログのローテーション閾値サイズを指定し、バイト数または `K`, `M` の補助単位を指定可能とする。

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
    - [info](#user-info) : ユーザ情報の詳細表示
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
    - [purge](#asset-purge) : 削除済みアセットのパージ
    - [undelete](#asset-undelete) : アセットの回復(削除の取消)
    - [move_to](#asset-move-to) : アセットの所有ページの付け替え
- fts : 全文検索の管理
    - [rebuild](#rebuild-index) : インデックスの再構築
    - [merge](#merge-segment) : セグメントの強制マージ
    - [search](#fts-search) : 検索の実施
- token :  Bearerトークンの管理
    - [create](#token-create) : トークンの生成
    - [add_path](#token-add-path) : トークンのpath制約追加
    - [remove_path](#token-remove-path) : トークンのpath制約削除
    - [revoke](#token-revoke) : トークンの無効化
    - [purge](#token-purge) : トークンの削除
    - [list](#token-list) : トークン一覧の表示
    - [info](#token-info) : トークン情報の詳細表示
- [export](#export) : バックアップ／マイグレート用のエクスポートデータの作成
- [import](#import) : エクスポートデータの取り込み

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
    - purge : `p`
    - undelete : `ud`
    - move_to : `m`, `mv`
- fts : `f`
    - rebuild : `r`
    - merge : `m`
    - search : `s`
- token : `t`
    - create : `c`
    - add_path : `a`, `add`
    - remove_path : `rm`
    - revoke : `r`
    - purge : `p`
    - list : `l`
- export : `e`
- import : `i`

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
|       `--mcp` | MCP機能を有効化して起動する |
| `-T`, `--tls` | サーバをHTTPSで起動させる |
| `-C`, `--cert FILE` | HTTPS使用時の証明書ファイルのパスを指定する | $XDG_DATA_HOME/luwiki/server.pem
 
#### 概要
引数`BIND-ADDR:PORT`でアドレスにバインドしHTTP/HTTPSサーバを起動する(デフォルトは"0.0.0.0:8080")。

`--tls`オプションを指定した場合、サーバはHTTPSでの通信を行う。このとき`--cert`オプションで指定されたサーバ証明書を用いる。`--cert`が指定されていない場合は規定のパスに置かれた証明書を使用するが、このファイルも存在しない場合は証明書を自動的に生成する(`--cert`オプションが指定され、そのファイルが存在しない場合はエラー)。

`--cert`で指定するファイルはPEM形式とする。PEMにはサーバ証明書と秘密鍵を含めるものとする。
証明書を自動生成する場合、生成物は`$XDG_DATA_HOME/luwiki/server.pem`（PEM）に保存する。PEM以外の補助ファイルが必要な場合は、`$XDG_DATA_HOME/luwiki/cert/`配下に保存する。

`--open-browser`オプションが指定された場合は、同時に規定のブラウザを起動する（デスクトップ環境でのみ有効）。

`--mcp`オプションが指定された場合は、HTTP/HTTPSサーバに加えてMCP機能を有効化する。

`--mcp`オプションが指定されていない場合は、設定ファイルの`run.use_mcp`を既定値として扱う。`run.use_mcp=true`であればMCP機能を有効化し、`run.use_mcp=false`または未設定であればMCP機能は無効とする。

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
|       `--attribute <ATTRIBUTE>` | 初期ユーザ属性の指定 |
 
#### 概要
引数 `USER-NAME` で指定されたユーザ名でユーザ登録を行う。このコマンドを実行するとパスワード登録用のプロンプトが表示され、パスワード入力が求められる。入力されたパスワードに問題が無ければユーザの登録が行われる。

`--attribute` を指定した場合は、作成時にユーザ属性を付与する。複数指定を許可する。初期実装では以下を指定可能。

  - `no_basic_auth`
  - `read_only`

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
|       `--add-attribute <ATTRIBUTE>` | ユーザ属性の追加 |
|       `--remove-attribute <ATTRIBUTE>` | ユーザ属性の削除 |
|       `--clear-attributes` | ユーザ属性の全消去 |
 
#### 概要
引数 `USER-NAME` で指定されたユーザ名のユーザ情報を変更する。

`--display-name`オプションが指定された場合は表示名を`NEW-NAME`指定された表示名に更新する。

`--password`オプションが指定された場合はパスワード入力用プロンプトを表示しユーザに新パスワードの入力を促し、その入力内容でパスワードの更新を行う。

`--add-attribute` と `--remove-attribute` には複数指定を許可する。初期実装では以下を指定可能。

  - `no_basic_auth`
  - `read_only`

`--clear-attributes` が指定された場合は、既存属性を一旦すべて取り除いた上で、`--add-attribute` による追加を適用する。

ユーザ属性の追加・削除は本コマンドで行う。`user add_attr` や `user remove_attr` のような専用コマンドは導入しない。

`--display-name`, `--password`, `--add-attribute`, `--remove-attribute`, `--clear-attributes` のいずれも指定されなかった場合はエラーとなる。

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

<a id="user-info"></a>
### user infoコマンド
ユーザ情報の詳細表示

#### コマンドライン
```sh
luwiki [OPTIONS] user info <USER-NAME>
```

#### 概要
引数 `USER-NAME` で指定されたユーザ名の詳細情報を大文字ラベル形式で表示する。

少なくとも以下の項目を表示する。

  - `USER ID`
  - `USERNAME`
  - `DISPLAY NAME`
  - `BASIC AUTH`
  - `ATTRIBUTES:`
  - `TIMESTAMPS:`
    - `update`

`BASIC AUTH` は `allowed` または `denied` を表示する。

`ATTRIBUTES:` には表示上の正式名称を用いる。初期実装では `NoBasicAuth` および `ReadOnly` を表示対象に含める。
属性が存在しない場合は `- none` を表示する。

以下の場合はエラーとする。

  - 指定されたユーザが存在しない

#### 注記
  - パスワード平文、パスワードハッシュ、ソルトは表示しない
  - Bearer トークン情報は表示しない

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

#### 概要
アセットID(`ASSET-ID`)かアセットパス(`ASSET-PATH`)で指定されたアセットの削除を行う。アセットパスはページパスとファイル名をパスセパレータで連結して表現する。
ページパス(`PAGE-PATH`)またはページID(`PAGE-ID`)が指定された場合は、ページに付随しているアセット全てが削除対象になる。

`--hard-delete`オプションが指定されている場合はハードデリートする(指定されていない場合はソフトデリートを行う)。

`--hard-delete`は削除済みアセットに対しても使用でき、DB上から完全に消去する。

指定されたアセットが存在しない場合はエラーとする。

削除済みアセットに対する削除は、`--hard-delete`指定時のみ許可する。

<a id="asset-purge"></a>
### asset purgeコマンド
アセットの削除済みデータをパージ

#### コマンドライン
```sh
luwiki [OPTIONS] asset purge [PAGE-PATH|PAGE-ID]
```

#### 概要
削除済みアセットをハードデリートする。引数が指定されない場合は全ページが対象となる。ページパス(`PAGE-PATH`)またはページID(`PAGE-ID`)を指定した場合は、そのページに付随する削除済みアセットのみを削除する。

<a id="asset-undelete"></a>
### asset undeleteコマンド
アセットの回復(削除の取消)

#### コマンドライン
```sh
luwiki [OPTIONS] asset undelete <ASSET-ID> [ASSET-NAME]
```
#### 概要
`ASSET-ID`で指定されたアセットを削除状態から通常状態に復活させる。`ASSET-NAME`が指定された場合は、その名前にリネームして復帰する。

以下の場合はエラーとする。

  - 指定されたIDのアセットが存在しなかった
  - 指定されたIDのアセットが削除状態ではなかった
  - 同名の生存アセットが存在する

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
英字を含む検索は大文字小文字を区別しない。

デフォルトでは検索対象は以下の通り。

  - 削除済みのページは検索対象に含めない
  - 最新リビジョンのみを検索対象とする

`--target`オプションで検索対象を指定できる。`--target`には以下の対象が指定できる。

  - headings : 見出し
  - body : 本文
  - code : コードブロック

`--with-deleted`オプションを指定した場合は削除済みページを検索対象に含める。

`--all-revision`オプションを指定した場合は全リビジョンを検索対象とする。

<a id="token-create"></a>
### token createコマンド 
トークンの生成

#### コマンドライン
```sh
luwiki [OPTIONS] token create [OPTIONS] <USER-NAME>
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-s`, `--scope <PERMISSION>` | スコープの指定 | `write`
| `-t`, `--ttl <DURATION>` | TTLの指定 | "30d"
| `-n`, `--name <TOKEN-NAME>` | トークン名の指定 |
|       `--path-prefix <PATH>` | 操作可能なpath prefix制約を追加する |

#### 概要
`<USER-NAME>` で指定した登録済みユーザに対して Bearer トークンを新規発行する。

`--scope` にはカンマ区切りでスコープを指定できる。初期実装では以下を指定可能。

  - `read`
  - `write`
  - `create`
  - `update`
  - `append`
  - `delete`

`write` を指定したトークンは `read` / `create` / `update` / `append` / `delete` 相当の操作も許可する。

`--path-prefix` には正規化済みの絶対パスを指定する。複数指定を許可し、指定された prefix のいずれかに一致する path のみを操作可能とする。

`--path-prefix` が指定されなかった場合は、全領域へのアクセスを許可するトークンを生成する。この場合、コマンド成功時に全領域アクセスである旨の警告を表示する。

`--ttl` には `30d`, `12h`, `90m` などの期間指定を受け付ける。指定がない場合は30日を使用する。

トークン本体の平文は発行時にのみ生成され、DBには保存しない。コマンド成功時は標準出力に以下を大文字ラベル形式で出力する。

  - `TOKEN ID`
  - `TOKEN NAME`
  - `USERNAME`
  - `SCOPES`
  - `PERMISSIONS`
  - `TTL`
  - `PATH PREFIXES:`
  - `TIMESTAMPS:`
    - `create`
    - `expire`
  - `WARNING:`
    - 全領域アクセス可の場合のみ表示
  - `TOKEN VALUE:`

`TOKEN NAME` は未指定時に `-` を表示する。

`SCOPES` は保存値としての指定内容をカンマ区切りで表示する。

`PERMISSIONS` は導出値としての実効権限を `read, create, delete, update, append` の順に完全名のカンマ区切りで表示する。

`TTL` は `30d` / `12h` / `90m` / `3600s` の短縮形式で表示する。

`PATH PREFIXES:` はセクション形式で表示し、path 制約がない場合は `- all` を表示する。

`TIMESTAMPS:` は `create` と `expire` を表示する。

`TOKEN VALUE:` は末尾に独立したセクションとして表示する。

以下の場合はエラーとする。

  - 指定されたユーザが存在しない
  - `--scope` に未定義のスコープが含まれている
  - `--path-prefix` に正規化済み絶対パスではない値が含まれている
  - `--ttl` の形式が不正
  - `--ttl` に0以下の期間が指定された

#### 注記
  - 発行されたトークン文字列はこのコマンドの実行時にのみ確認可能であり、後から再表示できない
  - 管理用識別子としてULID形式の `token_id` を付与する
  - スライディング期限は実際の認証成功時にのみ延長される
  - `--path-prefix /` を含む指定は全領域アクセスとして扱う

<a id="token-add-path"></a>
### token add_pathコマンド
トークンのpath制約追加

#### コマンドライン
```sh
luwiki [OPTIONS] token add_path <TOKEN-ID> <PATH-PREFIX>
```

#### 概要
`TOKEN-ID` で指定した Bearer トークンに、操作可能な path prefix 制約を1件追加する。

`PATH-PREFIX` には正規化済みの絶対パスのみを指定できる。

以下の場合はエラーとする。

  - 指定された `TOKEN-ID` が存在しない
  - `PATH-PREFIX` が正規化済みの絶対パスではない

#### 注記
  - `PATH-PREFIX` に `/` を指定した場合は全領域アクセス可として扱う
  - 包含関係にある複数 prefix は、より広い側へ縮約して保持してよい

<a id="token-remove-path"></a>
### token remove_pathコマンド
トークンのpath制約削除

#### コマンドライン
```sh
luwiki [OPTIONS] token remove_path <TOKEN-ID> <PATH-PREFIX>
```

#### 概要
`TOKEN-ID` で指定した Bearer トークンから、指定した path prefix 制約を1件削除する。

以下の場合はエラーとする。

  - 指定された `TOKEN-ID` が存在しない
  - `PATH-PREFIX` が正規化済みの絶対パスではない
  - 指定された path prefix 制約が存在しない

#### 注記
  - path prefix 制約が全て取り除かれた場合は、全領域アクセス可の状態へ戻る
  - 全領域アクセス可の状態へ戻った場合は、その旨の警告を表示する

<a id="token-revoke"></a>
### token revokeコマンド 
トークンの強制無効化

#### コマンドライン
```sh
luwiki [OPTIONS] token revoke [OPTIONS] [TOKEN-ID]
```

#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-u`, `--user <USER-NAME>` | 無効化ユーザ指定 |
| `-a`, `--all` | 全トークンの無効化指定 |
| `-y`, `--yes` | 確認プロンプトの回避を指定 |

#### 概要
Bearer トークンを失効状態に変更し、以後の認証に利用できないようにする。トークン本体や管理情報は削除せず保持する。

`[TOKEN-ID]` を指定した場合は、そのトークンのみを失効させる。

`--user` を指定した場合は、対象ユーザに紐づくトークンを失効対象に含める。

`--all` を指定した場合は、全ユーザの全トークンを失効対象に含める。

`--yes` が指定されていない場合は、実行前に確認プロンプトを表示する。確認プロンプトでは対象条件および対象件数を表示する。

以下の場合はエラーとする。

  - `[TOKEN-ID]` と `--user` と `--all` のいずれも指定されていない
  - `[TOKEN-ID]` と `--user` を同時に指定した
  - `[TOKEN-ID]` と `--all` を同時に指定した
  - `--user` と `--all` を同時に指定した
  - 指定された `TOKEN-ID` が存在しない
  - `--user` で指定されたユーザが存在しない
  - 非対話環境で `--yes` が指定されておらず、確認が必要になった

#### 注記
  - 既に失効済みまたは期限切れのトークンを対象に含めてもエラーにはしない
  - 成功時は失効対象件数を標準出力に表示する

<a id="token-purge"></a>
### token purgeコマンド 
トークンの削除

```sh
luwiki [OPTIONS] token purge [OPTIONS] [TOKEN-ID]
```
#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-e`, `--expired` | 期限切れトークンの指定 |
| `-r`, `--revoked` | 無効化トークン指定 |
| `-y`, `--yes` | 確認プロンプトの回避を指定 |

#### 概要
Bearer トークンの管理情報をDBから物理削除する。

`[TOKEN-ID]` を指定した場合は、そのトークンを削除する。

`--expired` を指定した場合は、期限切れとなっているトークンを削除対象に含める。

`--revoked` を指定した場合は、失効済みトークンを削除対象に含める。

`--expired` と `--revoked` の両方を指定した場合は、その和集合を削除対象とする。

`--yes` が指定されていない場合は、実行前に確認プロンプトを表示する。確認プロンプトでは対象条件および対象件数を表示する。

以下の場合はエラーとする。

  - `[TOKEN-ID]` と `--expired` を同時に指定した
  - `[TOKEN-ID]` と `--revoked` を同時に指定した
  - `[TOKEN-ID]` と `--expired` と `--revoked` のいずれも指定されていない
  - 指定された `TOKEN-ID` が存在しない
  - 非対話環境で `--yes` が指定されておらず、確認が必要になった

#### 注記
  - `token purge` は管理情報自体を削除するため、監査や一覧表示の対象からも消える
  - 成功時は削除対象件数を標準出力に表示する

<a id="token-list"></a>
### token listコマンド 
トークン一覧の表示

```sh
luwiki [OPTIONS] token list [OPTIONS] [USER-NAME]
```
#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-l`, `--long-info` | 詳細情報で表示 |
| `-u`, `--user <USER-NAME>` | 対象ユーザでのフィルタリングを指定 |
| `-r`, `--revoked` | 無効化トークンでのフィルタリングを指定 |
| `-e`, `--expired` | 期限切れトークンでのフィルタリングを指定 |

#### 概要
Bearer トークンの一覧を表示する。

`[USER-NAME]` を指定した場合は、そのユーザに紐づくトークンのみを表示する。`--user` も同様の意味を持ち、両者を同時に指定することはできない。

`--revoked` を指定した場合は失効済みトークンのみを表示する。`--expired` を指定した場合は期限切れトークンのみを表示する。両方を指定した場合は、いずれかの条件に一致するトークンを表示する。

`--long-info` を指定していない場合の表示項目は以下とする。

  - `SCOPE`
  - `PATH`
  - `ID`
  - `USER`
  - `NAME`
  - `EXPIRES`

`--long-info` を指定していない場合は、一覧の先頭に実効権限表示欄 `SCOPE` を設ける。この欄では、実効権限を `r` / `c` / `d` / `u` / `a` の各文字で表す。各文字の意味は以下の通りとする。

  - 1文字目 : `read` スコープを持つ場合は `r` 、持たない場合は `-`
  - 2文字目 : `create` スコープを持つ場合は `c` 、持たない場合は `-`
  - 3文字目 : `delete` スコープを持つ場合は `d` 、持たない場合は `-`
  - 4文字目 : `update` スコープを持つ場合は `u` 、持たない場合は `-`
  - 5文字目 : `append` スコープを持つ場合は `a` 、持たない場合は `-`

`write` スコープを持つ場合は、`SCOPE` 欄では `rcdua` として表示する。

`PATH` 欄では、一覧では詳細な制約内容ではなく、path制約の有無のみを表示する。

  - `*` : 全領域アクセス可
  - `L` : path制約あり

`--long-info` を指定した場合は、上記に加えて以下を表示する。

  - `CREATE`
  - `STATUS`

`STATUS` は以下の何れかを表示する。

  - `alive`
  - `expired`
  - `revoked`

状態判定では、`revoked` を `expired` より優先する。

以下の場合はエラーとする。

  - `[USER-NAME]` と `--user` を同時に指定した
  - 指定されたユーザが存在しない

#### 注記
  - 一覧表示ではトークン平文は表示しない
  - 期限切れかどうかはコマンド実行時点の現在時刻で判定する

<a id="token-info"></a>
### token infoコマンド
トークン情報の詳細表示

#### コマンドライン
```sh
luwiki [OPTIONS] token info <TOKEN-ID>
```

#### 概要
引数 `TOKEN-ID` で指定された Bearer トークンの詳細情報を大文字ラベル形式で表示する。

少なくとも以下の項目を表示する。

  - `TOKEN ID`
  - `TOKEN NAME`
  - `USERNAME`
  - `STATUS`
  - `SCOPES`
  - `PERMISSIONS`
  - `PATH PREFIXES:`
  - `TTL`
  - `TIMESTAMPS:`
    - `create`
    - `update`
    - `expire`

`TOKEN NAME` は未設定時に `-` を表示する。

`STATUS` は以下の何れかを表示する。

  - `alive`
  - `expired`
  - `revoked`

状態判定では、`revoked` を `expired` より優先する。

`PERMISSIONS` は保存値ではなく導出値を `read, create, delete, update, append` の順に完全名のカンマ区切りで表示する。

`TTL` は `30d` / `12h` / `90m` / `3600s` の短縮形式で表示する。

`PATH PREFIXES:` は詳細一覧としてセクション表示し、全領域アクセス可の場合は `- all` を表示する。

以下の場合はエラーとする。

  - 指定された `TOKEN-ID` が存在しない

#### 注記
  - トークン平文は表示しない
  - ユーザ属性は表示しない

<a id="export"></a>
### exportコマンド
バックアップ／マイグレート用のエクスポートデータの作成

#### コマンドライン
```sh
luwiki [OPTIONS] export [OPTIONS] <OUTPUT>
```
#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-s`, `--subtree <PREFIX>` | ページツリーのマイグレートを指定 |
| `-d`, `--dry-run` | 試験実行の指定 |
| `-p`, `--password <PASSWORD>` | エクスポートデータへのパスワード設定 |
| `-y`, `--yes` | 確認プロンプトの回避を指定 |
| `-S`, `--strict-mode` | 厳格モードでの動作を指定 |
 
#### 概要
バックアップまたはマイグレート用のエクスポートデータを作成する。`<OUTPUT>`にはファイルパス(ZIPファイル)もしくは"-"を指定する。"-"を指定した場合は標準出力への出力を行う。
なお、エクスポートには削除済みページ及び削除済みのアセットは含まれない。
ドラフトページはエクスポート対象に含めない。ロック中のページはエクスポート対象に含めるが、ロック情報自体はエクスポートデータに含めない。
アセットは編集不能オブジェクトとして扱い、孤立アセットはページパスを軸とした対象選定の結果、エクスポート対象に含めない。

`--subtree`オプションが指定された場合は、`<PREFIX>`で指定したパス以降を対象にマイグレート用データを作成する。データ作成後は`<PREFIX>`以降のページは削除される。`--subtree`オプションが指定されていない場合は、バックアップ用としてルートページ以降の全てのページを対象にエクスポートを行う。
`--subtree`を指定したエクスポートはサーバ間移動を目的としたマイグレートとして扱い、エクスポート処理と対象ページツリー削除は単一トランザクションで実行する。処理が成功した場合は対象ページツリーを無条件に削除し、このとき対象ページに紐付くロック情報も同時に削除する。途中で失敗した場合はロールバックする。
対象ページツリー削除時には、エクスポート対象外であるドラフトページも削除対象に含める。
`--subtree`を指定した場合でもrenameリビジョン自体は履歴として保持する。ただし、rename情報は有効なpath変更履歴としては扱わず、`revisions.jsonl` の `rename` には失効状態を表す `"removed_by_migrate"` を出力する。`pages.jsonl.rename_revisions` は出力しない。
ルートページ `"/"` を対象とする `--subtree` の指定はエラーとする。

`--dry-run`オプションを指定した場合は作成データの出力やデータベースへの書き込みを行わずデータのチェックなどを行うリハーサルモードで動作する。
`backup` の `--dry-run` では追加の検証は行わない。`migrate` の `--dry-run` では、少なくともツリー外へのページリンクおよびアセットリンクの有無、絶対パスによるページリンクの有無、インポート完了後の `username` 重複の有無、無効リンクへの置換対象の有無、移送先に子ページを持つ既存パスが存在しないことを検証する。

`--password`オプションを指定した場合は出力されるパスワード設定を行い暗号化されたZIPファイルが出力される。暗号方式はAES-256を優先し、利用できない場合はStandard ZIP 2.0へフォールバックしてよい。このフォールバックが発生した場合は警告を表示する。

`--yes`オプションを指定した場合は、破壊的操作前の確認プロンプトを回避し強制実行をおこなう。

`--strict-mode`オプションを指定した場合は、マイグレート後に問題が発生するページを検出した時点で処理を中断する。少なくともツリー外へのページリンクおよび絶対パスによるページリンクをエラー対象に含める。

<a id="import"></a>
### importコマンド
エクスポートデータの取り込み

#### コマンドライン
```sh
luwiki [OPTIONS] import [OPTIONS] <INPUT>
```
#### オプション

| オプション | 意味 | デフォルト値
|:--|:--|:--
| `-m`, `--migrate <PREFIX>` | マイグレート先のページパスの指定 |
| `-u`, `--user-map <MAPPING>` | ユーザマッピングの指定 |
| `-l`, `--user-list` | 編集者の一覧 |
| `-d`, `--dry-run` | 試験実行の指定 |
| `-f`, `--fix-broken-link` | 破損リンクの不正リンク化 |
| `-y`, `--yes` | 破壊的操作前の確認プロンプト回避を指定 |
| `-p`, `--password <PASSWORD>` | エクスポートデータに対するパスワード指定 |
| `-S`, `--strict-mode` | 厳格モードでの動作を指定 |
 
#### 概要
`export`コマンドで作成されたエクスポートデータをインポートする。バックアップ用データをインポートする場合は、既存データベースが存在しない場合のみインポートが可能となる(データベースファイルが存在する状態でバックアップ用データのインポートを指定するとエラーとして扱う)。
`<INPUT>`にはファイルパス(ZIPファイル)もしくは"-"を指定する。"-"を指定した場合は標準入力から入力を行う。

`--migrate`オプションを指定した場合はマイグレート用データの受け入れを行う。`<PREFIX>`でマイグレートされるツリーの配置場所を指定する。ページの衝突が発生した場合はエラーとなる。また、バックアップ用データに`--migrate`オプションを指定した場合もエラーとなる。
マイグレート時の最終配置パスは、エクスポートデータの`manifest.export_root`を基準とした相対パスを`rel_path`として、`<PREFIX>`に`rel_path`を連結した正規化パス（`normalize_path(PREFIX + \"/\" + rel_path)`）で決定する。
バックアップ用データのインポートでは復元先プレフィクスは`\"/\"`固定で扱う。
マイグレート時は、移送先に子ページを持つ既存パスが存在する場合もエラーとして扱う。
エクスポートデータ中のパス型フィールドは正規化済み表現を前提とし、`revisions.jsonl` に含まれるMarkdownソース中のリンク文字列は正規化しない。
マイグレート用データに有効なrename情報が含まれていた場合は、warningを出力した上でrename情報を失効状態へ正規化して処理を継続する。具体的には、`revisions.jsonl` の `rename` は `"removed_by_migrate"` として扱う。
マイグレート用データに `pages.jsonl.rename_revisions` が含まれていた場合も、warningを出力した上で空配列として扱う。
ユーザ情報は、バックアップ復元時・マイグレート時のいずれにおいても認証情報を含めてインポート対象とする。`--user-map`未指定ユーザも追加対象に含める。
インポート完了後に`username`が重複する状態になる場合は、`--strict-mode`の有無に関わらずエラーとして処理を中断する。
インポート時は`manifest`の件数と各JSONLの実件数の一致を検証する。アセットについては、`assets.jsonl`の件数と実体ファイル数の一致も検証する。
インポート時は、エクスポートデータ中のIDおよびインポート先の同種IDとの重複を検出した場合はエラーとして処理を中断する。
インポート時はエクスポートデータ内の参照整合性を検証し、少なくともJSONL間の参照切れ、アセット実体ファイルの欠落、アセットサイズ不一致を検出した場合はエラーとして処理を中断する。

`--user-map`オプションは、特定のページの編集者をインポート先のユーザに変更する場合に指定を行う。`<MAPPING>`には`{ページ編集ユーザ}={サーバ上のユーザ}`の形式でユーザマッピングを指定する。このオプションは複数指定が可能。

`--user-list`オプションを指定した場合は、エクスポートデータに含まれるページの編集者の一覧表示を行う。このオプションを指定した場合は、一覧表示のみを行い実際のインポートは行わない。

`--yes`オプションを指定した場合は、破壊的操作前の確認プロンプトを回避し強制実行をおこなう。

`--password`オプションは暗号化されたエクスポートデータを受け入れる場合のパスワード指定を行う。パスワード仕様はエクスポートデータで使用されたZIP暗号方式に従う。復号失敗、不正パスワード、未対応形式はすべてエラーとして処理を中断する。

`--dry-run`オプションを指定した場合はデータベースへの書き込みを行わずデータのチェックを行うリハーサルモードで動作する。
マイグレートの `--dry-run` では、実インポート時に無効リンクへ置換されるエントリが存在する場合、その有無を報告する。

`--fix-broken-link`を指定した場合は、マイグレート後に未解決となるページリンクを`about:invalid`に置き換えてインポートする。少なくともツリー外へのページリンクおよび絶対パスによるページリンクに起因する未解決リンクを対象に含める。

`--strict-mode`オプションを指定した場合は、マイグレート時に問題(例えばツリー外へのページリンクや絶対パスによるページリンクを含むなど)を検出した時点で処理を中断する。rename情報および`rename_revisions`の混入はwarningを出した上で正規化して処理を継続する。

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
| `db_path` | データベースファイルのパス | `--db-path` | `$XDG_DATA_HOME/luwiki/database.redb`
| `assets_path` | アセットデータ格納ディレクトリのパス | `--assets-path` | `$XDG_DATA_HOME/luwiki/assets/`
| `fts_index` | 全文検索インデックス格納ディレクトリのパス | `--fts-index` | `$XDG_DATA_HOME/luwiki/index/`
| `template_root` | テンプレートページの格納パス(Wiki上のパス) | `--template-root` |
| `wiki_title` | Wiki名 | `--wiki-title` |
| `wiki_icon` | Wikiアイコン画像ファイルのパス | `--wiki-icon` |
| `asset_limit_size` | アップロード可能なアセットサイズの上限 | `--asset-limit-size` |
| `audit_path` | 監査ログ出力ディレクトリのパス | `--audit-log-dir` | `$XDG_DATA_HOME/luwiki/audit`
| `audit_retention` | 監査ログ保持期間 | `--audit-log-retention` | "90d"
| `audit_rotate_size` | 監査ログローテーション閾値サイズ | `--audit-log-rotate-size` | "2M"

#### `wiki_icon` の注記
- 画像ファイルのパスを指定する
- 相対パス指定時は `config.toml` の親ディレクトリ基準で解決する
- 存在しないファイルまたは非画像ファイルを指定した場合は起動時エラーとする

<a id="config-run"></a>
### runテーブル
`run` サブコマンドのオプションに対するデフォルト値を設定し、以下のキーを定義する。

| キー | 設定内容 | 対応オプション/引き数 | デフォルト値
|:--|:--|:--|:--
| `bind` | サーバがバインドするアドレスを指定する | `BIND-ADDR` | "0.0.0.0"
| `port` | サーバがバインドするポートを指定する | `PORT` | 8080
| `use_mcp` | MCP機能を有効化するか否か | `--mcp` | false
| `use_tls` | TLSの使用 | `--tls` | false
| `server_cert` | 使用するサーバ証明書 | `--cert` | `$XDG_DATA_HOME/luwiki/server.pem`
 
#### 注記
- 互換性のために、`run`テーブルに値が無い場合は`global.use_tls`/`global.server_cert`を読み取って補完する。

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
