# Windowsサービス対応 実装設計書

本書は、`docs/REQUIREMENTS.md` で定義された既存要求、および `docs/CLI_SPECS.md` に定義された `run` コマンド仕様を前提として、Windows 環境でのサービス実行対応を追加するための実装設計を整理する文書である。

対象は Windows ビルド時に限定した `run` コマンドの拡張であり、他 OS での実行フローや既存の HTTP サーバ実装への影響は最小限に抑えることを目的とする。

---

## 1. 文書の目的

- Windows 環境で luwiki を Windows サービスとして起動・停止できるようにするための内部設計を定義する
- `run --win-service` の外部仕様と内部実装の責務分割を明確化する
- SCM への状態報告、停止要求処理、既存 HTTP サーバ起動フローとの接続方式を整理する
- 非 Windows 環境では本機能が存在しないことを明示し、プラットフォーム差異を説明可能にする

## 2. 対象範囲

本書の対象範囲は以下の通りとする。

- Windows ビルド時の `run` コマンドに対する `--win-service` オプション追加
- `windows-service` クレートを利用した SCM 連携
- Windows サービス実行時の開始報告、停止要求受付、停止完了報告
- 既存 `http_server::run()` に対する外部停止通知の接続
- 通常実行時とサービス実行時の分岐設計

本書の対象外は以下の通りとする。

- サービス登録・削除を行う専用 CLI の追加
- Windows サービス固有のインストーラ設計
- Linux systemd や macOS launchd など他 OS のサービス管理対応
- Windows サービス実行時の詳細な運用手順書

## 3. 前提と設計上の扱い

### 3.1 参照仕様

本書は少なくとも以下の文書を前提とする。

- `docs/REQUIREMENTS.md`
- `docs/BASE_DESIGN.md`
- `docs/CLI_SPECS.md`
- `docs/REST_API_SPECS.md`
- `docs/PROJECT_CONSTRAINTS.md`

### 3.2 前提条件

- Windows サービス機能は Windows ビルド時のみ有効とする
- 非 Windows ビルドでは `--win-service` オプション自体を定義しない
- 既存の通常実行フローは維持し、サービス実行時のみ SCM 連携を有効にする
- HTTP サーバ本体の待受・終了待機は現行の `http_server::run()` を基本とし、全面的な起動方式の変更は行わない
- サービス状態遷移の責務は Windows サービス層に持たせ、HTTP サーバ層には持ち込まない

### 3.3 設計方針

- Windows 固有依存は `cfg(windows)` で閉じ込め、他 OS 向けコードパスへ漏らさない
- CLI 上の Windows 固有オプションも `cfg(windows)` で制御し、他 OS から見えない状態にする
- SCM の開始報告と停止報告は `service_main()` 側で管理し、サーバ層は停止要求の実行に集中させる
- 停止要求は軽量な通知へ変換し、実際のサーバ停止は `ServerHandle.stop(true)` で graceful shutdown を行う
- 既存の Windows コンソールイベントフックは通常起動向けに残し、サービスモード時は使用しない

## 4. 外部仕様

### 4.1 `run --win-service` の追加

Windows ビルド時の `run` サブコマンドに、以下のオプションを追加する。

| オプション | 意味 | 対象環境 |
|:--|:--|:--|
| `--win-service` | Windows サービス実行モードで起動する | Windows のみ |

このオプションは以下の条件を満たすものとする。

- Windows ビルド時のみ CLI に存在する
- 非 Windows ビルドではヘルプ表示、引数解析、補完対象のいずれにも現れない
- 指定時のみ SCM と連携したサービス実行フローへ入る
- 未指定時は従来どおりの通常実行を行う

### 4.2 期待動作

- `run --win-service` 指定時、プロセスは Windows サービスとして起動されることを前提に SCM と連携する
- SCM からの `Stop` または `Shutdown` 要求により HTTP サーバを停止する
- サーバ起動準備完了後、SCM へ `Running` を報告する
- サーバ停止完了後、SCM へ `Stopped` を報告する

## 5. 全体構成

Windows サービス対応後の `run` コマンド実行経路は、概念上以下の 2 系統に分かれる。

### 5.1 通常実行

```text
main
  -> command::run::RunCommandContext::exec()
    -> 共通サーバ起動関数
      -> http_server::run()
        -> server.await
```

### 5.2 Windows サービス実行

```text
main
  -> command::run::RunCommandContext::exec()
    -> service_dispatcher::start(...)
      -> SCM が service_main() を別実行文脈で起動
        -> SCM ハンドラ登録
        -> 共通サーバ起動関数
          -> http_server::run()
            -> server.await
```

この構成では、`server.await` を廃止しない。サービスモードでは `command::run()` が直接 `http_server::run()` を呼ぶのではなく、SCM によって起動される `service_main()` 内で共通サーバ起動関数を呼ぶため、終了待機はそのまま成立する。

## 6. モジュール責務

### 6.1 `src/cmd_args/run.rs`

責務:

- `run` サブコマンド用 CLI オプション定義
- `--win-service` の Windows 限定公開
- `RunOpts` から Windows サービス実行フラグを取得できるようにする

設計方針:

- `RunOpts` に `win_service: bool` を追加する
- 当該フィールドおよび accessor は `#[cfg(windows)]` で囲む
- 非 Windows ビルドではフィールドを定義しない

### 6.2 `src/command/run.rs`

責務:

- `run` コマンド実行時の前処理
- 通常実行と Windows サービス実行の分岐
- DB オープン、FTS 設定、MCP endpoint 解決などの共通処理集約

設計方針:

- `RunCommandContext` に Windows サービス実行フラグを保持する
- 既存 `exec()` の本体を「共通サーバ起動関数」へ切り出す
- Windows かつ `--win-service` 指定時のみサービス起動関数へ委譲する
- それ以外は既存の通常起動を継続する

### 6.3 Windows 専用サービスモジュール

想定配置例:

- `src/command/windows_service.rs`

責務:

- `windows-service` クレートを用いた SCM 連携
- `service_dispatcher::start(...)` の呼び出し
- `service_main()` の定義
- `ServiceStatusHandle` の保持と状態遷移
- 停止通知チャネルの生成と HTTP サーバへの受け渡し

設計方針:

- Windows 専用モジュールとして分離し、`cfg(windows)` でのみコンパイルする
- SCM 制御と HTTP サーバ制御をこの層で橋渡しする

### 6.4 `src/http_server/mod.rs`

責務:

- HTTP サーバ本体の生成と終了待機
- 補助タスクの起動
- 外部停止通知を受けた際の graceful shutdown 実行

設計方針:

- `run()` に「外部停止通知」を表すオプション引数を追加する
- 停止通知が渡された場合のみ、通知待ちタスクを起動する
- 通常 Windows 起動時のコンソールイベントフックと、サービスモード時の SCM 停止通知は併用しない

## 7. 起動シーケンス

### 7.1 通常実行

1. `run` コマンドのオプションを解釈する
2. DB、FTS、MCP などの起動前処理を行う
3. `http_server::run()` を呼ぶ
4. Windows 通常実行時のみコンソールイベントフックを有効にする
5. `server.await` で終了待機する

### 7.2 Windows サービス実行

1. `run --win-service` を解釈する
2. `command::run()` は `service_dispatcher::start(...)` を呼ぶ
3. SCM が `service_main()` を起動する
4. `service_main()` は SCM 制御ハンドラを登録する
5. `service_main()` は `StartPending` を SCM へ報告する
6. `service_main()` は共通サーバ起動関数を呼ぶ
7. `http_server::run()` が `server.await` へ到達する直前に `Running` を SCM へ報告する
8. 停止要求が来るまで `server.await` で待機する
9. 停止要求受信後に graceful shutdown を実行する
10. 停止完了後に `Stopped` を SCM へ報告する

## 8. SCM への状態報告

### 8.1 状態遷移方針

SCM へのサービス状態報告は `service_main()` 側で明示的に行う。HTTP サーバ層には `Running` や `Stopped` の意味を持たせず、必要最小限の通知フックだけを受け取る。

状態遷移は以下を基本とする。

1. `StartPending`
2. `Running`
3. `StopPending`
4. `Stopped`

### 8.2 `StartPending`

`service_control_handler::register(...)` で `ServiceStatusHandle` を取得した直後に報告する。

設計意図:

- SCM に対して起動中であることを早期に伝える
- 起動に失敗した場合でも、無応答扱いではなく起動中断として扱いやすくする

### 8.3 `Running`

`http_server::run()` が `server.await` の箇所まで到達する直前に報告する。

このタイミングを採用する理由は以下の通り。

- DB オープンや設定解決など、主要な起動前処理が完了している
- サーバインスタンス生成に成功している
- 停止通知の処理経路が有効になっている
- サービスとして「開始済み」と見なせる状態にある

### 8.4 `StopPending`

SCM から `Stop` または `Shutdown` を受けた時点で報告する。

設計意図:

- サーバ停止処理中であることを SCM に伝える
- 停止要求受理後の状態を明確にする

### 8.5 `Stopped`

`http_server::run()` が正常に復帰し、停止処理が完了した後に報告する。

## 9. SCM 停止要求の処理

### 9.1 基本方針

SCM の制御ハンドラでは重い処理を直接行わず、停止要求を軽量な通知へ変換する。実際のサーバ停止は `http_server::run()` 側で行う。

### 9.2 処理フロー

1. `service_main()` で停止通知用チャネルを生成する
2. SCM ハンドラで `Stop` / `Shutdown` を受ける
3. ハンドラは `StopPending` 報告と停止通知送信だけを行う
4. `http_server::run()` 側の通知待ちタスクが受信する
5. `ServerHandle.stop(true)` を呼ぶ
6. `server.await` が復帰する
7. `service_main()` へ制御が戻り、`Stopped` を報告する

### 9.3 停止実行主体

サーバ停止の実行主体は `http_server::run()` とする。

理由:

- `ServerHandle` はサーバ生成直後に `http_server` 層で自然に取得できる
- SCM ハンドラ側で Actix サーバの内部詳細を意識せずに済む
- 停止経路を通常停止と同じ `ServerHandle.stop(true)` に統一できる

## 10. `http_server::run()` の拡張

### 10.1 追加する責務

`http_server::run()` は既存のサーバ起動・終了待機に加え、任意の外部停止通知を受け取れるようにする。

### 10.2 想定挙動

- 通常実行時は停止通知引数を `None` とする
- Windows サービス実行時は停止通知受信口を `Some(...)` とする
- 停止通知がある場合、別タスクで待機し、受信時に `ServerHandle.stop(true)` を実行する

### 10.3 `server.await` の扱い

`server.await` は維持する。

理由:

- サーバ本体のライフサイクル終端を一箇所へ集約できる
- 通常実行とサービス実行で同じ終了待機モデルを使える
- サービスモードでは `service_main()` が別実行文脈で動作するため、待機自体は問題にならない

## 11. Windows コンソールイベントとの関係

現行実装には Windows のコンソール終了イベントを拾ってサーバ停止を行う処理が存在する。この処理は通常実行時のために残すが、サービスモードでは使用しない。

方針:

- 通常 Windows 実行時
  - `ctrl_close`
  - `ctrl_logoff`
  - `ctrl_shutdown`
  を監視する既存フックを有効化する
- Windows サービス実行時
  - 上記フックは無効化する
  - SCM 停止要求だけを正式な停止入口とする

これにより、サービスモードでの停止責務を SCM に一本化する。

## 12. 依存関係

### 12.1 Cargo 依存

Windows サービス対応には `windows-service` クレートを追加する。

追加先は Windows 限定 target dependency とする。

```toml
[target.'cfg(windows)'.dependencies]
windows-service = "..."
```

### 12.2 依存追加方針

- 非 Windows ビルドで `windows-service` を引き込まない
- Windows 専用コードからのみ参照する
- 既存の `windows-sys` 依存との役割を混同しない

## 13. エラー処理方針

- `service_dispatcher::start(...)` 失敗時は `run` コマンド失敗として扱う
- SCM ハンドラ登録失敗時はサービス起動失敗として扱う
- サーバ起動前処理失敗時は `Running` を報告せず、起動失敗として終了する
- サーバ停止中のエラーは可能な限りログへ記録し、SCM へは `Stopped` または適切な終了コードを報告する余地を残す

初期実装では、詳細な Windows 固有終了コード設計までは固定せず、まずは開始・停止の正常系を優先する。

## 14. テスト観点

### 14.1 CLI 表示

- Windows ビルドでは `run --help` に `--win-service` が表示される
- 非 Windows ビルドでは `run --help` に `--win-service` が表示されない
- 非 Windows ビルドで `--win-service` 指定時は、未知オプションとして扱われる

### 14.2 起動分岐

- Windows かつ `--win-service` 指定時のみサービス起動経路へ入る
- Windows でも未指定時は通常起動経路へ入る
- 非 Windows では常に通常起動経路のみ存在する

### 14.3 SCM 状態遷移

- `service_main()` 起動後に `StartPending` が報告される
- `server.await` 到達直前に `Running` が報告される
- `Stop` / `Shutdown` 受信時に `StopPending` が報告される
- 停止完了後に `Stopped` が報告される

### 14.4 停止処理

- SCM 停止要求で `ServerHandle.stop(true)` が呼ばれる
- `server.await` が正常に復帰する
- 通常実行時の終了処理に回帰がない

## 15. 実装ステップ

1. `Cargo.toml` に Windows 限定で `windows-service` を追加する
2. `RunOpts` に Windows 限定 `--win-service` オプションを追加する
3. `RunCommandContext` にサービス実行フラグを取り込む
4. `run` コマンドの共通サーバ起動処理を切り出す
5. Windows 専用サービスモジュールを追加する
6. `http_server::run()` に外部停止通知引数を追加する
7. SCM 状態報告フックを組み込む
8. サービスモード時に既存 Windows コンソールイベントフックを無効化する
9. CLI 仕様書と必要なテストを更新する

## 16. まとめ

本設計では、Windows サービス対応を以下の原則で導入する。

- `--win-service` は Windows 環境でのみ存在する
- SCM 連携は Windows サービス層へ閉じ込める
- HTTP サーバ層は停止通知の受信と graceful shutdown 実行に専念する
- `server.await` は維持し、サービスモードでも同じ終了待機モデルを使う
- 通常実行の既存挙動は極力変えない

これにより、プラットフォーム依存機能を局所化しつつ、既存起動フローへ無理なく Windows サービス対応を追加できる。
