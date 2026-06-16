# MCPランタイム・transport設計

本書は、MCP内部設計のうち、
公開条件、起動条件、CLI / 設定反映、HTTP サーバ統合、transport / endpoint 構成を整理するための文書である。

本書は、共通部である `docs/MCP_INTERNAL_DESIGN.md` を前提とし、
現行 `docs/MCP_INTERNAL_DESIGN.md` の以下の章を移設する受け皿として用いる。

- 12. MCP の公開条件と起動条件
- 13. MCP の transport / endpoint 構成

関連する設計文書は以下の通り。

- `docs/MCP_ARCHITECTURE_DESIGN.md`
  - HTTPサーバ統合層、MCP公開層、共通サービス層の責務境界を確認する場合に参照する
- `docs/MCP_SERVICE_AND_STORAGE_DESIGN.md`
  - transport から呼び出す path ベースサービス層と DB API の前提を確認する場合に参照する
- `docs/MCP_INTERFACE_AND_ERROR_DESIGN.md`
  - 公開ツール、入出力契約、MCP エラー応答の公開面を確認する場合に参照する
- `docs/MCP_RESOURCE_SPECS.md`
  - resources capability、標準resources操作、固定組み込みresource導線の外部契約を確認する場合に参照する
- `docs/MCP_AUDIT_LOG_DESIGN.md`
  - 監査ログ関連のグローバル設定、起動時初期化、終了時 flush、保守タスク連携を確認する場合に参照する
- `docs/MCP_CLI_TEST_VIEWPOINTS.md`
  - CLI 拡張のテスト観点を確認する場合に参照する
- `docs/MCP_REGRESSION_TEST_SCOPE.md`
  - `run` 起動経路および既存 CLI / HTTP サーバ統合の回帰確認範囲を確認する場合に参照する

---

## 1. 対象範囲

本書では以下を対象とする。

- `run` コマンドでの MCP 有効化
- CLI / config / 起動経路への反映
- HTTPサーバへの組み込み
- transport / endpoint 構成
- transport adapter の責務

## 2. MCP の公開条件と起動条件

本章では、MCP の公開条件と起動条件として、
MCP を常時公開せず、`run` コマンド実行時に明示的に有効化された場合のみ
公開するための条件を定義する。

### 2.1 基本方針

- MCP はデフォルトで無効とする
- `run` コマンドで明示的に有効化された場合のみ公開する
- MCP が無効な場合は、HTTP サーバ起動中でも MCP endpoint は登録しない
- MCP の有効 / 無効はサーバ起動時に確定し、起動後の動的切替は初期実装では扱わない

### 2.2 設定の流れ

MCP 有効化の設定は、少なくとも以下の流れで伝播させる。

1. `src/cmd_args/run.rs`
   - `run` コマンドの CLI オプションとして MCP 有効化指定を受け取る
2. `src/command/run.rs`
   - CLI / config で解決した MCP 設定を起動コンテキストへ集約する
3. `src/http_server/mod.rs`
   - HTTP サーバ生成時に MCP 公開口を登録するか判定する
4. `src/http_server/app_state.rs`
   - 必要に応じて MCP 関連設定や依存情報を共有状態へ保持する

この流れにより、MCP の公開有無は HTTP サーバ初期化の責務として扱う。

### 2.3 CLI と設定ファイルの優先順位

MCP 有効化条件は、既存の `run` 設定解決方針に合わせ、
次の優先順位で決定する。

1. CLI 明示指定
2. 設定ファイル
3. 既定値

既定値は `false` とする。

これにより、通常起動では MCP は露出せず、
利用者が意図的に有効化した場合のみ公開される。

#### 2.3.1 CLI 引数名と基本動作

CLI 引数名は、既存 `docs/CLI_SPECS.md` に合わせて
`run` サブコマンドの `--mcp` を採用する。

初期実装での基本動作は以下とする。

- `luwiki run --mcp`
  - MCP を有効化して起動する
- `luwiki run`
  - CLI で有効化を明示していない状態として扱う

初期実装では、無効化専用の `--no-mcp` は追加しない。

理由は以下の通りとする。

- 要求仕様上、MCP はデフォルト無効である
- 明示有効化だけあれば、最小要件を満たせる
- 既存 CLI 仕様でも `--mcp` のみが定義されている

このため、CLI 層では `--mcp` の有無を
「true を明示したか」「未指定か」の二値として保持し、
最終的な有効 / 無効の決定は config と既定値を合わせて解決する。

#### 2.3.2 設定ファイルキー

設定ファイルキーは、既存 `docs/CLI_SPECS.md` に合わせて
`run.use_mcp` を採用する。

意味は以下の通りとする。

- `run.use_mcp = true`
  - `run` コマンド実行時の既定値として MCP を有効化する
- `run.use_mcp = false`
  - `run` コマンド実行時の既定値として MCP を無効化する
- `run.use_mcp` 未設定
  - 既定値 `false` を適用する

監査ログ設定はグローバル設定として分離しつつ、
MCP 起動に直接関わる設定は `run` セクションへ集約する。

#### 2.3.3 優先順位の具体化

4.8.1 時点での具体的な解決規則は以下とする。

1. CLI で `--mcp` が指定された場合
   - 常に有効
2. CLI で `--mcp` が指定されていない場合
   - `run.use_mcp` を参照する
3. `run.use_mcp` も未設定の場合
   - `false`

この解決規則により、
`--mcp` は config を上書きする明示有効化として扱える。

一方で、初期実装では `--no-mcp` を持たないため、
config で `run.use_mcp = true` を設定した状態から
その起動だけ無効化したい場合は、config を変更する運用とする。
この点は制約としてヘルプ文言または実装注記へ反映してよい。

#### 2.3.4 `RunOpts` 上の表現方針

`src/cmd_args/run.rs` では、
MCP 有効化指定を `bool` の既定 `false` で保持するのではなく、
「CLI で明示指定があったか」を区別できる形で保持する方針とする。

理由は以下の通りとする。

- `bool` 既定 `false` では、CLI 未指定と明示無効を区別できない
- 今回は `--mcp` しか無いが、config 優先順位解決には未指定判定が必要である

そのため概念上は以下のいずれかの形を取る。

- `use_mcp: Option<bool>`
- `enable_mcp: bool` と `mcp_specified: bool` の組

初期実装では単純さを優先し、
`Option<bool>` で `Some(true)` または `None` を持つ形を第一候補とする。

#### 2.3.5 config 読込と保存の反映方針

`src/cmd_args/config.rs` および `src/cmd_args/mod.rs` への反映方針は以下とする。

- `RunInfo` に `use_mcp: Option<bool>` を追加する
- `Config` に `set_run_use_mcp(bool)` と `run_use_mcp() -> Option<bool>` を追加する
- `RunOpts::apply_config()` では、CLI 未指定時のみ `run.use_mcp` を補完する
- `--save-config` 時は、`run.use_mcp` へ保存する

保存時の方針は以下とする。

- `luwiki run --mcp --save-config`
  - `run.use_mcp = true` を保存する
- `luwiki run --save-config`
  - MCP 明示指定が無い場合でも、解決済みの実効値を保存してよい

後者の扱いは既存 `run` の `use_tls` 保存方針と同様に、
「実効設定を保存する」方針へ合わせる。

#### 2.3.6 ヘルプ文言の方針

`run --help` および関連文書での文言は、
少なくとも以下の内容を含む方針とする。

- `--mcp`
  - MCP 機能を有効化して起動する
- config 既定値
  - `run.use_mcp` を既定値として用いる
- 既定状態
  - 未指定かつ config 未設定時は無効

文言の粒度は既存 `CLI_SPECS.md` と揃え、
長い設計説明は help 文へ持ち込まない。
具体文面としては、既存 CLI 仕様にある
「MCP機能を有効化して起動する」を踏襲してよい。

#### 2.3.7 監査ログのグローバル設定との連動方針

4.7.5 で定義した監査ログのグローバル設定との関係は以下の通りとする。

- MCP が無効な場合
  - 監査ログ設定が存在しても、MCP 起点の監査ログ基盤は起動しない
- MCP が有効な場合
  - `global.audit_path`、`global.audit_retention`、`global.audit_rotate_size` を解決して監査ログ基盤初期化へ渡す

この順序により、
MCP 非有効時に監査ログだけが単独起動する状態を避ける。

#### 2.3.8 4.8.1 の設計結論

以上より、4.8.1 の設計結論は以下とする。

- `run` コマンドの MCP 有効化オプションは `--mcp` とする
- config キーは `run.use_mcp` とする
- 既定値は `false` とする
- 優先順位は `CLI --mcp` → `run.use_mcp` → `false` とする
- `RunOpts` は CLI 未指定を識別できる表現を採る
- `--save-config` では解決済み MCP 有効化設定を `run.use_mcp` へ保存する
- help 文言は「MCP機能を有効化して起動する」を基準に簡潔に保つ

#### 2.3.9 4.8.2 の対象範囲

4.8.2 では、4.7.5 で整理した監査ログ設定のうち、
CLI グローバルオプションと config で外部指定可能とする項目の指定方法を確定する。

本節で対象とするのは以下の 3 項目とする。

- 監査ログ出力ディレクトリ
- 監査ログ保持期間
- ローテーション閾値サイズ

`enabled` については、
MCP 起動そのものの有効化と強く連動するため、
初期実装では 4.8.1 の `--mcp` / `run.use_mcp` を主入口とし、
4.8.2 では追加の専用 CLI オプションを設けない。

#### 2.3.10 CLI オプション名

CLI オプション名は、グローバルオプションとして以下を採用する。

- `--audit-log-dir DIR`
  - 監査ログ出力ディレクトリを指定する
- `--audit-log-retention DURATION`
  - 保持期間を指定する
- `--audit-log-rotate-size SIZE`
  - ローテーション閾値サイズを指定する

短縮オプションは追加しない。

理由は以下の通りとする。

- 監査ログ設定は特定サブコマンド固有ではなく運用全体に関わるため
- 監査ログ設定は運用者向けであり、長いオプション名でも十分実用的である
- 名前に `audit-log-` 接頭辞を付けることで、通常ログ設定 `--log-output` と混同しにくい

#### 2.3.11 config キー

config 側のキーは、グローバル設定として以下を置く。

- `global.audit_path`
- `global.audit_retention`
- `global.audit_rotate_size`

`CLI_SPECS.md` 側へ反映する際も、
CLI 名と config キーの対応は以下の表を基準とする。

| config キー | CLI オプション | 意味 |
|:--|:--|:--|
| `global.audit_path` | `--audit-log-dir` | 監査ログ出力ディレクトリ |
| `global.audit_retention` | `--audit-log-retention` | 保持期間 |
| `global.audit_rotate_size` | `--audit-log-rotate-size` | ローテーション閾値サイズ |

#### 2.3.12 保持期間の入力書式

保持期間 `retention` の入力書式は、
既存 `token create --ttl` の設計に揃えて、
末尾単位付きの簡潔な文字列表現を採用する。

初期実装で許可する形式は以下とする。

- `Nd`
  - 日単位
- `Nh`
  - 時間単位
- `Nm`
  - 分単位

例は以下の通りとする。

- `90d`
- `72h`
- `1440m`

既定値は、要求仕様上の「3か月」を
実装上は `90d` として扱う。

月単位の `3mo` や `3M` を採用しない理由は以下とする。

- 月長の揺れを持ち込まず、固定期間として扱いたい
- 既存の `ttl` パーサと同系統の実装へ寄せやすい
- config / CLI /内部表現の往復が単純になる

#### 2.3.13 保持期間のバリデーション方針

`--audit-log-retention` および `global.audit_retention` の検証は
グローバルオプション読取側で行う。

検証規則は以下とする。

- 空文字は不正
- 数値部が存在しない場合は不正
- 単位が `d` / `h` / `m` 以外なら不正
- 0 以下は不正
- 解決済み内部表現は `chrono::Duration` 相当とする

エラー文言の方針は、既存 `ttl` と同様に
簡潔な「format is invalid」「must be greater than zero」系に揃えてよい。

#### 2.3.14 ローテーション閾値サイズの入力書式

`rotate_size` の入力書式は、
既存 `asset_limit_size` の表現に寄せた
単位付きサイズ文字列とする。

初期実装で許可する形式は以下とする。

- `N`
  - バイト
- `Nk` / `NK`
  - KiB
- `Nm` / `NM`
  - MiB

例は以下の通りとする。

- `2097152`
- `512K`
- `2M`

既定値は、`MCP_AUDIT_LOG_DESIGN.md` の候補に合わせて
通常ログと同程度の運用感を持つ `2M` を第一候補とする。

`G` 単位を初期実装で含めない理由は以下の通りとする。

- ローカル運用前提の初期要件では MiB 単位で十分である
- 既存 `asset_limit_size` の実装資産へ寄せやすい
- 異常に大きな単一監査ログファイル化を避けやすい

#### 2.3.15 ローテーション閾値サイズのバリデーション方針

`--audit-log-rotate-size` および `global.audit_rotate_size` の検証は
グローバルオプション読取側で行う。

検証規則は以下とする。

- 空文字は不正
- 数値部が存在しない場合は不正
- 単位が未指定、`K` / `k`、`M` / `m` 以外なら不正
- 0 は不正
- 乗算オーバーフローは不正
- 解決済み内部表現は `u64` バイト数とする

初期実装では最大値の上限制約は設けず、
`u64` 範囲内かつ正値であることだけを必須条件とする。
運用上の推奨値は help や文書で補足してよい。

#### 2.3.16 出力ディレクトリ指定の方針

`--audit-log-dir` および `global.audit_path` は、
監査ログの出力先ディレクトリを指定する。

解決規則は以下とする。

- CLI で指定された path はそのまま raw 値として受け取り、後段で `PathBuf` 化する
- config の相対 path は既存 `Config::resolve_path()` 規則で解決する
- CLI で相対 path を指定した場合の基準は、既存グローバル path オプションと同様にプロセス作業ディレクトリでよい
- 未指定時の既定値は `DEFAULT_DATA_PATH.join("audit")` 相当とする

また、ディレクトリ作成自体は `src/audit/` の初期化責務とし、
`cmd_args` 層では path の文字列表現と空文字検証までに留める。

#### 2.3.17 優先順位と保存方針

`path` / `retention` / `rotate_size` の優先順位は、
MCP 有効化フラグと同様に以下とする。

1. CLI 明示指定
2. config (`global.audit_*`)
3. 内部既定値

`--save-config` 時の保存方針は以下とする。

- CLI で指定された raw 値は、その raw 文字列表現を保存する
- CLI 未指定で config 由来の値を使った場合は、その実効値を再保存してよい
- path は解決済み絶対 path ではなく、入力された相対 / 絶対表現を優先して保存する

これにより、既存 config 保存方針と同様に
手編集しやすい TOML を維持できる。

#### 2.3.18 4.8.2 の設計結論

以上より、4.8.2 の設計結論は以下とする。

- CLI オプションは `--audit-log-dir`、`--audit-log-retention`、`--audit-log-rotate-size` のグローバルオプションとする
- config キーは `global.audit_path`、`global.audit_retention`、`global.audit_rotate_size` とする
- `retention` は `Nd` / `Nh` / `Nm` を許可し、既定値は `90d` とする
- `rotate_size` はバイト、`K`、`M` を許可し、既定値は `2M` を第一候補とする
- バリデーションはグローバルオプション読取側で行い、内部表現は `Duration` と `u64` に解決する
- `enabled` の専用 CLI オプションは初期実装では追加せず、MCP 有効化に従属させる

#### 2.3.19 4.8.3 の対象範囲

4.8.3 では、Bearerトークン管理 CLI のうち
`token create` / `token list` / `token info`
を、新しい分解スコープ体系と path prefix 制約へ合わせて設計反映する。

本節の対象は以下とする。

- `token create`
  - 分解スコープ指定
  - path prefix 指定
  - 完了表示
- `token list`
  - 一覧表示
  - `--long-info`
  - 状態表示
- `token info`
  - 単一トークンの完全表示

以下は本節の直接対象に含めない。

- `token add_path`
- `token remove_path`
- `token revoke`
- `token purge`

これらは path prefix 制約の内部モデルに依存するが、
本タスクの完了条件である create / list / info の設計反映とは分けて扱う。

#### 2.3.20 `token create` の入力設計

`token create` の入力は、既存 CLI 仕様を基準に以下とする。

- `--scope <PERMISSION>`
  - カンマ区切り指定
- `--ttl <DURATION>`
  - 既存 `30d` / `12h` / `90m` 形式
- `--name <TOKEN-NAME>`
  - 任意名
- `--path-prefix <PATH>`
  - 複数指定可
- `<USER-NAME>`
  - 発行対象ユーザ名

`--scope` で許可する値は以下とする。

- `read`
- `write`
- `create`
- `update`
- `append`
- `delete`

保持方針は 9.4.2 に従い、
`write` 指定時も保存時に分解展開しない。
CLI は保存値としてのスコープを受け取り、
表示時にのみ実効権限を導出する。

`--path-prefix` は複数指定可能とし、
未指定時は全領域アクセス可とする。
`/` を含む指定も全領域アクセス可として扱う。

#### 2.3.21 `token create` のバリデーション方針

`src/cmd_args/token.rs` で行う検証は以下とする。

- `--scope`
  - 空要素を許可しない
  - 未定義スコープを許可しない
  - 重複は除去してよい
- `--ttl`
  - 既存 `parse_token_ttl()` に従う
- `--name`
  - trim 後に空なら不正
- `--path-prefix`
  - 正規化済み絶対パスのみ許可する
  - 複数指定時は 1 件でも不正があれば全体をエラーとする

path prefix の正規化・縮約規則は保存前処理または DB 側でも再適用してよいが、
CLI 層では少なくとも「正規化済み絶対パスであること」までは検証する。

#### 2.3.22 `token create` の完了表示

`token create` の成功時出力は、
既存の最小表示を拡張し、大文字ラベル形式で少なくとも以下を表示する。

- `TOKEN ID`
- `TOKEN NAME`
- `USERNAME`
- `SCOPES`
  - 指定スコープ
- `PERMISSIONS`
  - 実効権限
- `TTL`
- `PATH PREFIXES:`
  - 正規化済み path prefix 一覧
- `TIMESTAMPS:`
  - `create`
  - `expire`
- `TOKEN VALUE:`

表示方針は以下とする。

- `TOKEN NAME`
  - 未設定時は `-` を表示する
- `SCOPES`
  - 保存値としての指定内容をカンマ区切りで表示する
- `PERMISSIONS`
  - `read`, `create`, `delete`, `update`, `append` の順に導出表示する
- `TTL`
  - `30d` / `12h` / `90m` / `3600s` の短縮形式で表示する
- `PATH PREFIXES:`
  - 全領域アクセス可の場合は `- all` を表示する
- `TOKEN VALUE:`
  - 発行時のみ表示し、後から再表示しない

また、path prefix 未指定で全領域アクセス可となる場合は、
成功出力とは別に `WARNING:` を補助表示してよい。

#### 2.3.23 `token list` の列構成

`token list` は、`docs/MCP_DESIGN_INPUT_TASKS.md` の整理結果に従い、
一覧表示と詳細一覧表示で役割を分ける。

短縮表示の列順は以下とする。

- `SCOPE`
- `PATH`
- `ID`
- `USER`
- `NAME`
- `EXPIRES`

`--long-info` の列順は以下とする。

- `SCOPE`
- `PATH`
- `ID`
- `USER`
- `NAME`
- `EXPIRES`
- `CREATE`
- `STATUS`

ここでの表示意味は以下とする。

- `SCOPE`
  - 実効権限の 5 文字表示
- `PATH`
  - path 制約なしは `*`
  - path 制約ありは `L`
- `STATUS`
  - `alive` / `expired` / `revoked`

`updated_at` は `token list` から外し、
一覧責務を優先する。

#### 2.3.24 `token list` の実効権限表示

`SCOPE` 欄の並び順は以下で固定する。

1. `read` → `r`
2. `create` → `c`
3. `delete` → `d`
4. `update` → `u`
5. `append` → `a`

表示規則は以下とする。

- 対応スコープを実効的に持つ場合は文字を表示する
- 持たない場合は `-`
- `write` を保持する場合は `rcdua`
- 分解済みスコープのみを保持する場合は、その実効権限だけを表示する

この欄は保存値の再表示ではなく、
「そのトークンで何ができるか」の一覧表示であることを優先する。

#### 2.3.25 `token list` の path 制約表示

`PATH` 欄は、詳細内容ではなく有無のみを表示する。

規則は以下とする。

- 全領域アクセス可
  - `*`
- path prefix 制約あり
  - `L`

ここで全領域アクセス可とは、
`path_prefixes` 未設定または `/` を含む場合を指す。

一覧では詳細 prefix 群を表示せず、
詳細確認は `token info` へ委ねる。

#### 2.3.26 `token list` の状態表示

`STATUS` 欄の値は以下の 3 種に限定する。

- `alive`
- `expired`
- `revoked`

優先順位は以下とする。

1. `revoked = true`
   - `revoked`
2. `expire_at <= now`
   - `expired`
3. 上記以外
   - `alive`

これにより、
失効と期限切れの区別を短く安定した文字列で表せる。

#### 2.3.27 `token info` の導入方針

詳細表示は `token list --long-info` に過剰な責務を持たせず、
単一トークンを完全表示する `token info <TOKEN-ID>` を新設する。

役割分担は以下とする。

- `token list`
  - 一覧比較用
- `token info`
  - 単一トークンの完全表示

`token info` は `ls` に対する `stat` 相当の位置付けとし、
表形式ではなく大文字ラベルによる完全表示を第一候補とする。

#### 2.3.28 `token info` の表示項目

`token info` の表示項目は、トークン管理情報に含まれる内容を
責務どおりすべて表示する。

少なくとも以下を表示する。

- `TOKEN ID`
- `TOKEN NAME`
- `USERNAME`
- `STATUS`
  - `revoked` と `expire_at` から導出する状態値
- `SCOPES`
  - 保存値
- `PERMISSIONS`
  - 導出値
- `PATH PREFIXES:`
  - 詳細一覧
- `TTL`
- `TIMESTAMPS:`
  - `create`
  - `update`
  - `expire`

表示しない項目は以下とする。

- トークン平文
  - 発行時以外は再表示しない
- ユーザ属性
  - `user info` の責務とする

`TOKEN NAME` は未設定時に `-` を表示する。

`PATH PREFIXES:` は複数行表示してよく、
全領域アクセス可の場合は `- all` を表示する。

`PERMISSIONS` は一覧表示の `SCOPE` 欄と異なり、
`token info` では完全名の権限列として表示する。
例えば `read,append` は `read, append`、
`write` は `read, create, delete, update, append` とする。

`status` の優先順位は以下とする。

1. `revoked = true`
   - `revoked`
2. `expire_at <= now`
   - `expired`
3. 上記以外
   - `alive`

#### 2.3.29 `token create` / `list` / `info` と内部保持項目の責務分離

表示項目の責務分離は 9.4.4 に従い、以下を維持する。

- 保存するもの
  - `token_id`
  - `user_id`
  - `scopes`
  - `path_prefixes`
  - `created_at`
  - `updated_at`
  - `ttl`
  - `expire_at`
  - `revoked`
  - `name`
- 導出するもの
  - `user_name`
  - `effective_permissions`
  - path 制約有無
  - 全領域アクセス可表示
  - 期限切れ状態
  - `STATUS`

この整理により、
create / list / info いずれの CLI も
表示専用キャッシュ列へ依存しない。

#### 2.3.30 4.8.3 の設計結論

以上より、4.8.3 の設計結論は以下とする。

- `token create` は分解スコープ指定と複数 `--path-prefix` 指定を受け付ける
- `token create` の完了表示では指定スコープと実効権限を分けて表示する
- `token list` の短縮表示は `SCOPE`, `PATH`, `ID`, `USER`, `NAME`, `EXPIRES` とする
- `token list --long-info` は `CREATE`, `STATUS` を追加し、`updated_at` は表示しない
- `SCOPE` 欄は `rcdua` 順の実効権限表示、`PATH` 欄は `*` / `L`、`STATUS` 欄は `alive` / `expired` / `revoked` とする
- `token info <TOKEN-ID>` を新設し、単一トークンの完全表示出口とする
- token 系 CLI はユーザ属性を表示せず、`user info` と責務を分離する

#### 2.3.31 4.8.4 の対象範囲

4.8.4 では、user 系 CLI のうち
`user add` / `user edit` / `user info`
を、`NoBasicAuth` を含むユーザ属性モデルへ合わせて設計反映する。

本節の対象は以下とする。

- `user add`
  - 初期属性指定
  - パスワード入力要否
- `user edit`
  - 属性追加
  - 属性削除
  - 属性置換
  - Basic 認証再有効化時のパスワード更新条件
- `user info`
  - 単一ユーザの完全表示

以下は本節の直接対象に含めない。

- `user list`
  - 一覧責務は現状維持とする
- export / import 表示
  - データモデル側責務として別タスクで扱う

#### 2.3.32 `user add` の入力設計

`user add` の入力は、既存 CLI 仕様を基準に以下とする。

- `--display-name <NAME>`
  - 任意表示名
- `--attribute <ATTRIBUTE>`
  - 複数指定可
- `<USER-NAME>`
  - 登録対象ユーザ名

`--attribute` の初期実装で許可する値は以下とする。

- `no_basic_auth`
  - 表示上の正式属性名 `NoBasicAuth` に対応する CLI 入力値

入力方針は以下とする。

- `--attribute` 未指定時
  - 属性なしユーザとして作成する
- `--attribute no_basic_auth` を含む場合
  - Basic 認証不可ユーザとして作成する
- 同一属性の重複指定
  - CLI 層で除去してよい

パスワード入力要否は属性集合に応じて切り替える。

- `NoBasicAuth` を含まない場合
  - 既存どおりパスワード入力プロンプトを要求する
- `NoBasicAuth` を含む場合
  - パスワード入力を要求しない

ここで「パスワード入力を要求しない」とは CLI 入力要件の話であり、
内部保存形式で未使用パスワード欄をどう扱うかは
9.5 のユーザ属性モデル拡張側で吸収する。

#### 2.3.33 `user edit` の入力設計

`user edit` では、表示名変更、パスワード変更、属性操作を
同一コマンドで扱えるようにする。

入力は以下を基本とする。

- `--display-name <NEW-NAME>`
  - 表示名変更
- `--password`
  - 新パスワード入力プロンプトを要求する
- `--add-attribute <ATTRIBUTE>`
  - 属性追加、複数指定可
- `--remove-attribute <ATTRIBUTE>`
  - 属性削除、複数指定可
- `--clear-attributes`
  - 現在属性を全消去する
- `<USER-NAME>`
  - 更新対象ユーザ名

属性操作の適用順は以下を基本とする。

1. `clear`
2. `remove`
3. `add`

この規則により、
`--clear-attributes --add-attribute no_basic_auth`
のような指定で「完全置換」を表現できる。

`src/cmd_args/user.rs` での検証方針は以下とする。

- `display_name` / `password` / 属性操作のいずれも無い場合はエラー
- 属性値は `user add` と同じ列挙に従う
- 同一オプション内の重複属性は除去してよい
- 未定義属性は許可しない

#### 2.3.34 `user edit` の属性遷移とパスワード条件

`NoBasicAuth` は Basic 認証可否へ影響するため、
`user edit` では属性遷移に応じた入力条件を定める。

方針は以下とする。

- 通常ユーザへ `NoBasicAuth` を追加する場合
  - `--password` の同時指定は不要
  - 既存パスワードが存在していても Basic 認証では拒否される
- `NoBasicAuth` ユーザから同属性を除去する場合
  - `--password` の同時指定を必須とする
  - Basic 認証再有効化と同時に有効な資格を明示設定する
- `NoBasicAuth` ユーザのまま表示名だけを更新する場合
  - `--password` は不要
- `NoBasicAuth` ユーザに対する `--password` 単独指定
  - 初期実装では不正入力として扱う

最後の規則により、
Basic 認証不可ユーザへ未使用のパスワード更新経路を持ち込まない。

#### 2.3.35 `user info` の導入方針

ユーザ属性の詳細表示は `user list` へ詰め込まず、
単一ユーザを完全表示する `user info <USER-NAME>` を新設する。

役割分担は以下とする。

- `user list`
  - 一覧比較用
- `user info`
  - 単一ユーザの詳細表示

`user info` は token 系 CLI と同様に、
一覧と詳細を分離して責務を明確化するための出口とする。

#### 2.3.36 `user info` の表示項目

`user info` は、ユーザ情報およびユーザ属性の運用確認に必要な内容を
大文字ラベル形式で表示する。

少なくとも以下を表示する。

- `USER ID`
- `USERNAME`
- `DISPLAY NAME`
- `BASIC AUTH`
- `ATTRIBUTES:`
  - 属性なしの場合は `- none`
- `TIMESTAMPS:`
  - `update`

表示方針は以下とする。

- `BASIC AUTH`
  - `allowed` / `denied` を表示する
- `ATTRIBUTES:`
  - 表示上は正式名称 `NoBasicAuth` を用いる
  - 複数属性時は複数行表示してよい
- `TIMESTAMPS:`
  - 既存 user 系日時表示と同じローカル時刻 ISO8601 表記に合わせる

表示しない項目は以下とする。

- パスワード平文
- パスワードハッシュ
- ソルト
- Bearer トークン情報
  - `token list` / `token info` の責務とする

`user info` は「内部保存値の生ダンプ」ではなく、
運用上必要な管理情報を完全表示する出口として扱う。

#### 2.3.37 4.8.4 の設計結論

以上より、4.8.4 の設計結論は以下とする。

- `user add` は `--attribute <ATTRIBUTE>` の複数指定を受け付け、初期実装の属性値は `no_basic_auth` とする
- `user add` は `NoBasicAuth` を含む場合に限り、パスワード入力プロンプトを要求しない
- `user edit` は `--add-attribute` / `--remove-attribute` / `--clear-attributes` により属性追加・削除・置換を扱う
- `user edit` では `NoBasicAuth` を除去して Basic 認証を再有効化する場合、`--password` の同時指定を必須とする
- `user edit` では `NoBasicAuth` ユーザへの `--password` 単独指定を不正入力として扱う
- `user info <USER-NAME>` を新設し、属性集合を含む完全表示の出口とする
- `user list` は一覧責務を維持し、属性詳細は表示しない

#### 2.3.38 4.8.5 の対象範囲

4.8.5 では、MCP 関連拡張に伴う CLI および保存データの後方互換影響を整理する。

本節で主に確認する観点は以下とする。

- `token create --scope write` の入力互換
- 旧 Bearer トークン管理情報の読取互換
- 旧 `UserInfo` データの読取互換
- `token list` / `user` 系コマンドの表示変更影響
- 新設 `token info` / `user info` が既存運用へ与える影響

#### 2.3.39 `write` 入力互換の扱い

分解スコープ体系導入後も、
既存運用との互換のため `write` は入力値として維持する。

互換方針は以下とする。

- `token create --scope write`
  - 引き続き受理する
- `token create --scope read,write`
  - 引き続き受理する
- 既存トークン管理情報中の `write`
  - 保存値としてそのまま読めるようにする
- required scope 判定
  - `write` は `read` / `create` / `update` / `append` / `delete` を包含する

一方で、以下は互換維持の対象に含めない。

- `write` の表示を `write` 1 文字で一覧表示すること
  - 一覧では実効権限表示 `rcdua` を優先する
- `write` だけで `token info` の導出表示を省略すること
  - 詳細表示では保存値と実効権限を分けて示す

この整理により、
入力互換は維持しつつ、表示上は分解後モデルを前提とした理解へ移行できる。

#### 2.3.40 既存 Bearer トークン管理情報との互換影響

Bearer トークン管理情報は本体永続化の例外として拡張を許容するが、
旧データの読取互換は確保する。

方針は 9.4.6 に従い、以下を基本とする。

- 旧データの `scopes = read/write`
  - 新列挙へ読めること
- `path_prefixes` を持たない旧データ
  - 全領域アクセス可として扱う
- 旧データに `name` 以外の追加保持項目が無い場合
  - 既定値補完または互換デシリアライズで吸収する

運用影響は以下とする。

- 既存トークンを再発行せずに一覧・認証へ使えることを優先する
- 旧 `write` トークンは、新しい read / create / update / append / delete 要求にも利用可能とする
- path 制約未対応時代のトークンは、制約なしトークンとして表示される

つまり、旧トークンは「より緩い時代の権限設定を保持したまま読まれる」ため、
破壊的変更ではなく、既定で広い権限を維持する互換方針になる。

#### 2.3.41 既存ユーザデータとの互換影響

`UserInfo` への `attributes` 追加後も、
既存ユーザデータは無移行で継続利用できることを優先する。

方針は 9.5.5 に従い、以下を基本とする。

- `attributes` を持たない旧 `UserInfo`
  - 空集合として解釈する
- 旧ユーザ
  - `NoBasicAuth` 未設定ユーザとして扱う
- 既存 `user add`
  - `--attribute` 未指定なら従来同様に Basic 利用可能ユーザを作成する
- 既存 `user edit`
  - `--display-name` と `--password` だけを使う運用は引き続き成立する

互換影響として明示すべき点は以下とする。

- `NoBasicAuth` を導入しない限り、既存ユーザ運用は変わらない
- `user list` は属性列を追加しないため、既存一覧確認の見え方は維持される
- 新設 `user info` は追加機能であり、既存コマンド呼び出しを置き換えない

#### 2.3.42 `token list` / `user` 系表示変更の互換影響

CLI の後方互換は、入力互換と出力互換を分けて扱う。

`token list` の影響は以下とする。

- 既存の人手運用
  - 一覧表示の情報量は増えるが、用途は維持される
- 既存の表示列
  - `AUTH` 中心の旧説明から、`SCOPE` / `PATH` / `STATUS` を持つ新列構成へ変わる
- `--long-info`
  - `updated_at` を外し `CREATE` / `STATUS` を表示するため、表形式出力は後方互換ではない

したがって、`token list` の標準出力を機械解析している運用がある場合は、
新列構成への追従が必要である。
本コマンドは人間向け一覧表示を主責務とするため、
出力形式の完全固定互換までは保証しない。

`user` 系の影響は以下とする。

- `user list`
  - 現状の一覧責務を維持し、属性列は追加しない
- `user add`
  - 属性未指定時の挙動は従来どおりパスワード入力あり
- `user edit`
  - 既存の `--display-name` / `--password` は維持する
- `user info`
  - 新設コマンドであり、既存運用への破壊的変更ではない

#### 2.3.43 4.8.5 の設計結論

以上より、4.8.5 の設計結論は以下とする。

- `write` は `token create --scope` の後方互換入力として維持し、保存値としても読み続ける
- 旧 Bearer トークン管理情報は、`read` / `write` スコープおよび `path_prefixes` 欠落を読取互換で吸収する
- 旧 `UserInfo` データは、`attributes` 欠落時に空集合として解釈し、既存ユーザを `NoBasicAuth` 未設定ユーザとして継続利用できるようにする
- `token list` は表示改善のため列構成が変わるため、機械解析出力としての完全後方互換は保証しない
- `user list`、`user add`、`user edit` の既存基本操作は維持し、`token info` / `user info` は追加コマンドとして導入する

### 2.4 HTTP サーバへの反映

HTTP サーバ側では、MCP 有効化フラグを受け取ったうえで、
Actix のルーティングへ MCP endpoint を登録するかどうかを分岐させる。

- 有効時
  - MCP endpoint を登録する
  - MCP が必要とする依存を注入する
- 無効時
  - MCP endpoint を登録しない
  - 既存 REST API / UI ルートのみを公開する

この設計により、無効時に認証だけ通る幽霊 endpoint を残さない。

### 2.5 起動失敗条件

MCP が有効化されている場合に限り、
MCP 公開に必要な設定や依存が満たせないときは起動失敗として扱う。

初期実装で少なくとも考慮する失敗条件は以下とする。

- MCP transport の初期化に失敗する
- MCP 公開口の登録に必要な構成値が不正である
- 監査ログ出力先など、MCP 有効時に必須となる依存の初期化に失敗する

一方で MCP が無効な場合は、これらの失敗条件を評価対象に含めない。

### 2.6 公開条件と起動条件に関する設計判断

本章の公開条件・起動条件設計では、以下を基本方針として採用する。

- MCP は `run` コマンドの明示有効化時のみ公開する
- 既定値は無効とする
- CLI、設定ファイル、既定値の順で有効化条件を解決する
- HTTP サーバ初期化時に endpoint 登録可否を決定する
- 有効化時のみ MCP 固有依存の初期化失敗を起動失敗として扱う

## 3. MCP の transport / endpoint 構成

本章では、MCP の transport / endpoint 構成として、
初期実装で採用する transport 方式と、Actix への組み込み位置を定義する。

### 3.1 基本方針

transport 再構築後の方針として、以下の構成を採用する。

- MCP の protocol / message / transport 仕様処理は公開クレートへ委譲する
- `src/mcp/transport.rs` では、Actix との接続、HTTP 認証境界、アプリ依存注入だけを扱う
- transport 仕様は Streamable HTTP を前提とする
- `initialize`を含む標準MCP handshake、SSE response、session管理を使用する
- endpoint は `/mcp` の単一 endpoint とする
- `initialize.instructions`はBearer認証前提と固定組み込みresourceの導線を案内する

この方針の目的は、MCP 互換性をアプリ固有実装より優先し、
一般的な AI エージェントが `initialize` から接続できる状態を最小条件として固定することにある。

旧実装の独自`tool` / `arguments` JSONは廃止済みであり、
公開クレートによる標準JSON-RPCとStreamable HTTPを使用する。

### 3.2 採用する HTTP メソッド

`/mcp` に対する HTTP メソッドの扱いは、採用する公開クレートが要求する
Streamable HTTP 仕様へ合わせる。

- `POST`
  - `initialize`、通常 request、notification の受理に使用する
- `GET`
  - Streamable HTTPのSSE responseに使用する
- `DELETE`
  - session終了に使用する

HTTP メソッドの扱いは自前都合で固定せず、仕様適合を優先して決定する。

### 3.2.1 `initialize.instructions` で案内する事項

`initialize.instructions`は以下を案内する。

- すべてのMCP HTTP要求でBearer認証を使用すること
- front matter詳細仕様は固定組み込みresourceとして参照できること
- MCP prompts仕様は固定組み込みresourceとして参照できること
- resource集合の最新状態が必要な場合は`resources/list`を再取得すること

### 3.2.2 固定組み込みresourceの公開方針

固定組み込みresourceは、MCP標準`resources/list`および`resources/read`で
公開するLuWiki組み込みの読み取り専用resourceとする。

初期版では以下を公開する。

- `luwiki://local.luwiki/builtin/front-matter-spec`
  - 内容は `docs/FRONT_MATTER_SPECS.md` と整合する
  - write系tool利用前の事前参照、およびfront matter起因失敗後の再参照先として使える
- `luwiki://local.luwiki/builtin/mcp-prompt-spec`
  - 内容は `docs/MCP_PROMPT_SPECS.md` と整合する
  - MCP promptsを利用する前の事前参照先として使える

固定組み込みresourceはページpath、page ID、path prefix制約に依存しない。
resource公開のprotocol、handshake、標準message形式は公開クレートへ委譲し、
アプリ側ではresource内容の供給、認可、監査、`instructions`での案内を担当する。

### 3.3 session 管理方針

transport 再構築の stateful mode 導入後は、
`rmcp-actix-web` の Streamable HTTP 運用に合わせて
session 管理を有効化する。

ただし `rmcp` 1.3.0 の `LocalSessionManager` は
`DELETE /mcp` による明示 close を前提とした in-memory 管理であり、
idle TTL や最大 session 数制限を持たない。
このため、一般的な MCP クライアントが `DELETE` を送らない運用では、
session と再接続用 cache がプロセス寿命まで残留し得る。

以上を踏まえ、初期実装の session 管理は以下の方針を採る。

- `src/mcp/transport.rs` は `LocalSessionManager` を直接使わず、
  wrapper である `ManagedSessionManager` を注入する
- wrapper は `LocalSessionManager` を内部委譲先として保持し、
  `SessionManager` trait の表面だけを差し替える
- session ID の生成、`mcp-session-id` ヘッダ整合、
  Streamable HTTP の protocol 処理は引き続き公開クレートへ委譲する
- TTL / LRU の判断は transport 境界で完結させ、
  `service.rs` へ持ち込まない

`ManagedSessionManager` が保持する metadata は少なくとも以下とする。

- `created_at`
- `last_access_at`
- `closing`

`last_access_at` の更新契機は以下の通りとする。

- 更新する:
  `initialize_session`、`create_stream`、
  `create_standalone_stream`、`resume`、`accept_message`
  が成功した時点
- 更新しない:
  `has_session()`

`has_session()` を更新対象から除外する理由は、
`rmcp-actix-web` が `GET` / `POST` 前段の存在確認でも
`has_session()` を呼ぶためである。
ここで TTL を延命すると、
実処理を伴わない polling だけで session が維持されてしまう。

初期設定値は以下とする。

- idle TTL: 30 分
- sweep 間隔: 60 秒
- 最大 session 数: 64

TTL / LRU の具体動作は以下とする。

- background sweep task が sweep 間隔ごとに
  idle TTL 超過 session を close する
- `create_session()` の前に期限切れ sweep と
  上限制御を実施する
- session 上限超過時は、
  `closing == false` の session のうち
  `last_access_at` が最古のものから LRU eviction する
- LRU の同順位は `created_at` で安定化する
- forced close 後は metadata を除去し、
  以後の `GET` / `POST` / `DELETE` は
  backend 準拠で `session not found` 相当となる

background sweep task は
HTTP server 全体で共有する `ManagedSessionManager` の生成時に
1 回だけ開始し、server worker ごとに重複起動しないようにする。
停止は manager drop 時の task abort を基本とする。

### 3.4 Actix への組み込み位置

Actix への組み込みは `src/http_server/mod.rs` の `create_server()` 内で行う。

- 既存 `App::new()` に MCP endpoint を追加する
- 追加位置は REST API と同じ root 空間配下とし、`/mcp` を単独 route として登録する
- `rest_api::create_api_scope(...)` の内部へは入れない

この配置により、MCP は REST API と兄弟の公開面として扱われ、
認証・エラーハンドリング・ログの適用範囲も個別に調整しやすくなる。

### 3.5 transport adapter の責務

再構築後の `src/mcp/transport.rs` は、少なくとも以下を担当する。

- `/mcp` endpoint の request を公開クレートへ橋渡しする
- Actix の request / response と公開クレートの transport 型を接続する
- `Authorization` ヘッダを Bearer 認証入口へ接続する
- 必要最小限の `Origin` 制約や reverse proxy 配下での受理条件を適用する
- `AppState`、監査ログ入口、サービス実装を MCP サーバ実装へ注入する
- transport レベル異常とツール実行異常の責務境界を保つ

一方で、以下は transport adapter の責務に含めない。

- JSON-RPC request / response の独自 decode / encode
- `initialize`、`tools/list`、`tools/call`、`prompts/list`、
  `prompts/get`、`resources/list`、`resources/read` の message 形式解釈
- `MCP-Protocol-Version` や session 系ヘッダの独自運用
- path ベース業務処理
- Bearer スコープや path prefix 制約の業務判定
- 監査ログの業務内容決定

標準 MCP の envelope と handshake は公開クレートへ委譲し、
このプロジェクト固有の認証・認可・業務呼び出しだけを残す。

固定組み込みresourceやページ由来resourceを配信する場合も、
resource公開のprotocol / handshake自体は公開クレートへ委譲し、
アプリ側ではresource内容の供給、認可、監査、`instructions`での案内に責務を限定する。

### 3.6 transport レベルの検証方針

transport レベル検証は、公開クレートの標準処理とアプリ固有検証に分ける。

- 公開クレートへ委譲するもの
  - JSON-RPC 形式妥当性
  - `initialize` / `initialized` を含む protocol handshake
  - `tools/list` / `tools/call` の標準 message 形式
  - `prompts/list` / `prompts/get` の標準 message 形式
  - `resources/list` / `resources/read` の標準 message 形式
  - `MCP-Protocol-Version` の解釈
- アプリ側で保持するもの
  - `Authorization` の存在と Bearer 形式検証
  - `Origin` 制約
  - 運用上必要な HTTP メソッド制約

transport 失敗と業務エラーは引き続き分離するが、
標準 protocol 解釈をアプリ側へ再実装しないことを原則とする。

固定組み込みresourceおよびページ由来resourceの有無は
protocol解釈の独自拡張理由にせず、標準capability宣言の範囲で扱う。

### 3.7 既存 middleware との境界

MCP endpoint には、REST API 用 middleware を無条件には流用しない。

- Bearer 認証の共通利用余地はある
- ただし `WWW-Authenticate` を前提とした REST API の応答規約は、そのまま持ち込まない
- `X-Bearer-Expire` の扱いは MCP transport 側で個別に判断する
- Access log への記録は既存 `AccessLogger` の対象としつつ、必要に応じて MCP request と識別できるようにする

### 3.8 将来拡張余地

現行のSSE responseとsession管理を維持しつつ、以下を守る。

- transport adapter と service 層を密結合にしない
- `/mcp` endpoint 自体は維持し、内部実装だけを差し替え可能にする
- SSEやsession管理は公開クレートへ委譲する
- server-to-client notification の未実装を service 層の制約にしない

### 3.9 transport / endpoint 構成に関する設計判断

本章の transport / endpoint 設計では、以下を基本方針として採用する。

- `/mcp` は一般的な MCP クライアントと相互運用できることを最優先要件とする
- transport / protocol / message 処理は公開クレートへ委譲する
- `src/mcp/transport.rs` は Actix 統合と認証境界に責務を限定する
- Streamable HTTP準拠を維持し、同じhandshake済みsessionで
  tools、prompts、resourcesの標準methodを利用する
- resources capabilityはresource URI索引readinessと連動して公開する
- `initialize.instructions`はBearer認証前提と固定組み込みresourceの導線を案内する
- transport レベルの検証と業務エラーを明確に分離する
- 既存の独自 `tool` / `arguments` JSON は廃止する

具体的な再構築方針、移行順序、受け入れ条件は
`docs/MCP_TRANSPORT_REBUILD_PLAN.md` を参照する。

## 4. MCP promptsのruntime・transport統合

### 4.1 capability

`LuwikiMcpServer`は生成時にprimitive名前索引のreadinessを確認する。
構築状態がversion 1の場合だけrmcpの`.enable_prompts()`を使用し、
`initialize`応答へprompts capabilityを含める。

状態マーカーなし、未知version、`AppState` lock失敗、DB読取失敗では
安全側へ倒してprompts capabilityを公開しない。readinessにかかわらず
既存tools capabilityは維持する。

`.enable_prompts_list_changed()`は使用せず、`prompts.listChanged`を宣言しない。

### 4.2 標準handler

rmcp標準の`ServerHandler::list_prompts()`と`get_prompt()`を実装する。
両操作はtools用`tools/call`ではなく、rmcpのprompts標準routingから処理する。
実装済み操作をdefaultのmethod-not-foundへ落とさない。

各handlerは`RequestContext<RoleServer>`から認証文脈と入力元IP addressを取得し、
既存handler・serviceへ橋渡しする。

### 4.3 handshakeとsession

promptsは既存Streamable HTTP契約を使用する。

1. Bearer付き`initialize`を送信する
2. `notifications/initialized`を完了する
3. 同じsession IDとBearerで`prompts/list`または`prompts/get`を送信する

同じhandshake済みsession上で`tools/list`、`prompts/list`、
`tools/call`、`prompts/get`を交互に利用できる。既存のSSE response、
session期限、上限超過時のeviction、DELETEによるsession終了を維持する。

Authorization headerは認証middlewareで消費し、rmcp request contextへ
Bearer平文を転送しない。

### 4.4 server情報とinstructions

`initialize.serverInfo.version`は固定文字列ではなく
`env!("CARGO_PKG_VERSION")`を使用する。

`initialize.instructions`は、すべてのHTTP要求でBearer認証を使用する旨と、
front matter詳細仕様およびMCP prompts仕様を固定組み込みresourceとして
参照できる旨を案内する。

### 4.5 通知非対応

M3初期版はprompt集合変更通知を公開契約に含めない。

- prompt保存後同期から通知しない
- soft delete、hard deleteから通知しない
- `derived rebuild --target prompts|all`から通知しない
- MCP sessionまたはtransport peerへ通知しない

クライアントは最新状態が必要な場合に`prompts/list`を再取得する。
将来通知へ対応する場合は、capability、送信契機、commitとの原子性、
複数sessionへの配信、失敗処理を別途設計する。

### 4.6 transport失敗との境界

Authorization欠落、不正、失効はprompts handler到達前にHTTP 401として処理する。
認証とsession検証後のscope不足はHTTP 401ではなくJSON-RPC protocol errorとして返す。

prompts追加後もrequest bodyとAuthorization headerを通常ログへ出力せず、
既存toolsの認証、routing、session管理を変更しない。

## 5. MCP resourcesのruntime・transport統合

### 5.1 capability

`LuwikiMcpServer`は生成時にresource URI索引のreadinessを確認する。
構築状態がversion 1の場合だけrmcpの`.enable_resources()`を使用し、
`initialize`応答へresources capabilityを含める。

状態マーカーなし、未知version、`AppState` lock失敗、DB読取失敗では
安全側へ倒してresources capabilityを公開しない。readinessにかかわらず
既存tools capabilityは維持する。prompts readinessが満たされる場合は
prompts capabilityも独立して公開する。

`resources.listChanged`は宣言しない。

### 5.2 標準handler

rmcp標準の`ServerHandler::list_resources()`と`read_resource()`を実装する。
両操作はtools用`tools/call`ではなく、rmcpのresources標準routingから処理する。
実装済み操作をdefaultのmethod-not-foundへ落とさない。

各handlerは`RequestContext<RoleServer>`から認証文脈と入力元IP addressを取得し、
既存handler・serviceへ橋渡しする。

### 5.3 handshakeとsession

resourcesは既存Streamable HTTP契約を使用する。

1. Bearer付き`initialize`を送信する
2. `notifications/initialized`を完了する
3. 同じsession IDとBearerで`resources/list`または`resources/read`を送信する

同じhandshake済みsession上で`tools/list`、`tools/call`、`prompts/list`、
`prompts/get`、`resources/list`、`resources/read`を交互に利用できる。
既存のSSE response、session期限、上限超過時のeviction、
DELETEによるsession終了を維持する。

Authorization headerは認証middlewareで消費し、rmcp request contextへ
Bearer平文を転送しない。

### 5.4 固定組み込みresourceとページ由来resource

固定組み込みresourceとページ由来resourceは同じresources capabilityの下で公開する。
`resources/list`では両者を合流し、URI昇順で返す。
`resources/read`ではURIに応じて固定組み込み本文またはページ由来本文を解決する。

固定組み込みresourceにはページ用path prefix制約を適用しない。
ページ由来resourceにはread scopeとpath prefix制約を適用し、
範囲外の場合は一覧から除外し、取得ではnot foundとして扱う。

### 5.5 通知非対応

M4初期版はresource集合変更通知を公開契約に含めない。

- resource保存後同期から通知しない
- rename、soft delete、undelete、hard deleteから通知しない
- import、rollback、amendから通知しない
- `derived rebuild --target resources|all`から通知しない
- MCP sessionまたはtransport peerへ通知しない

クライアントは最新状態が必要な場合に`resources/list`を再取得する。
将来通知へ対応する場合は、capability、送信契機、commitとの原子性、
複数sessionへの配信、失敗処理を別途設計する。

### 5.6 transport失敗との境界

Authorization欠落、不正、失効はresources handler到達前にHTTP 401として処理する。
認証とsession検証後のscope不足はHTTP 401ではなくJSON-RPC protocol errorとして返す。

resources追加後もrequest bodyとAuthorization headerを通常ログへ出力せず、
既存toolsおよびpromptsの認証、routing、session管理を変更しない。
